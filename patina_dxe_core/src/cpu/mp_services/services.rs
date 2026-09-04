//! Provides the architecture agnostic wrapper for the architecture implementation
//! and AP state maintained for the component/protocol.
//!
//! ## License
//!
//! Copyright (c) Microsoft Corporation.
//!
//! SPDX-License-Identifier: Apache-2.0
//!

use alloc::{boxed::Box, vec::Vec};
use core::sync::atomic::{AtomicBool, Ordering};

use patina::{
    component::service::perf_timer::ArchTimerFunctionality,
    standard::efi::{self, protocols::mp_services},
};
use patina_internal_cpu::mp::{ApWorkItem, MpDispatcher, MpSupport, ProcessorState};

use super::notification::{NotificationRegistry, PendingDispatch, deadline_for};

/// Window for an AP to apply a state synchronization dispatch.
const AP_SYNC_TIMEOUT_US: usize = 100_000;

/// Reports the outcome of a non-blocking dispatch once the periodic poll resolves
/// it, taking the processors the procedure did not complete on.
pub(super) type DispatchCompletion = Box<dyn FnOnce(&[usize])>;

/// The processors a dispatch targets, resolved under the dispatch lock.
enum Targets {
    /// Every started, enabled AP that no pending dispatch already owns.
    AllEligible,
    /// One specific AP.
    One(usize),
}

/// The structure that tracks and manages MP services. Invoked by the protocol wrapper.
pub(super) struct MpServices {
    mp: MpSupport,
    processor_infos: Vec<mp_services::ProcessorInformation>,
    timer: &'static dyn ArchTimerFunctionality,
    notifications: NotificationRegistry,
    ready_to_boot: AtomicBool,
}

impl MpServices {
    pub(super) fn new(
        mp: MpSupport,
        processor_infos: Vec<mp_services::ProcessorInformation>,
        timer: &'static dyn ArchTimerFunctionality,
    ) -> Self {
        Self {
            mp,
            processor_infos,
            timer,
            notifications: NotificationRegistry::new(),
            ready_to_boot: AtomicBool::new(false),
        }
    }

    /// Returns the total number of logical processors in the system. (Including the BSP)
    pub(super) fn total_processors(&self) -> usize {
        self.mp.processor_count()
    }

    /// Returns the number of processors available for dispatch. (Including the BSP)
    pub(super) fn enabled_processors(&self) -> usize {
        self.mp.enabled_processor_count()
    }

    /// Enables or disables the AP at `processor_index`, optionally recording a new
    /// health status. Only callable from the BSP.
    pub(super) fn enable_disable_ap(
        &self,
        processor_index: usize,
        enable: bool,
        healthy: Option<bool>,
    ) -> Result<(), MpError> {
        self.bsp_check()?;
        if processor_index == MpSupport::BSP_INDEX {
            return Err(MpError::NotSupported);
        }
        if processor_index >= self.mp.processor_count() {
            return Err(MpError::NotFound);
        }
        if self.mp.set_ap_enabled(processor_index, enable, healthy) { Ok(()) } else { Err(MpError::NotFound) }
    }

    /// Returns the processor index for the calling processor.
    pub(super) fn who_am_i(&self) -> Option<usize> {
        self.mp.who_am_i()
    }

    /// Signals all APs to park in a defined state for OS handoff.
    pub(super) fn park(&self) {
        self.notifications.shutdown(&self.mp);
        self.mp.park();
    }

    /// Prevents new non-blocking requests once their timer-driven completion
    /// mechanism is no longer guaranteed to run.
    pub(super) fn mark_ready_to_boot(&self) {
        self.ready_to_boot.store(true, Ordering::Release);
    }

    /// Replicates the BSP's caching (MTRR) state to every AP that can accept it.
    pub(super) fn synchronize(&self) {
        let deadline = deadline_for(self.timer, AP_SYNC_TIMEOUT_US);
        for index in MpSupport::BSP_INDEX + 1..self.mp.processor_count() {
            if !self.mp.ap_healthy(index) {
                continue;
            }
            loop {
                // Selecting and dispatching under the lock stops the poll from taking
                // the processor while its synchronization payload is being written.
                let started = self.notifications.with_dispatch_lock(|pending| {
                    if pending.iter().any(|dispatch| dispatch.claims(index)) {
                        return None;
                    }
                    self.mp.sync_ap(index)
                });
                if let Some(work_id) = started {
                    if !self.wait_ap_until(index, work_id, deadline) {
                        self.fence_off(index);
                    }
                    break;
                }

                if !self.mp.ap_healthy(index)
                    || matches!(self.mp.ap_availability(index), ProcessorState::NotStarted | ProcessorState::Disabled)
                {
                    break;
                }
                if self.deadline_passed(deadline) {
                    self.fence_off(index);
                    break;
                }
                core::hint::spin_loop();
            }
        }
    }

    /// Returns the [`mp_services::ProcessorInformation`] for `index`.
    pub(super) fn processor_info(&self, index: usize) -> Result<mp_services::ProcessorInformation, MpError> {
        let processor_id = self.mp.processor_id(index).ok_or(MpError::NotFound)?;
        let mut info = self
            .processor_infos
            .iter()
            .find(|p| p.processor_id == processor_id as u64)
            .copied()
            .unwrap_or_else(|| topology_unknown(processor_id));
        info.status_flag = self.status_flag(index);
        Ok(info)
    }

    fn status_flag(&self, index: usize) -> u32 {
        let mut flags = 0;
        if index == MpSupport::BSP_INDEX {
            flags |= mp_services::PROCESSOR_AS_BSP_BIT;
        }
        if !matches!(self.mp.ap_availability(index), ProcessorState::NotStarted | ProcessorState::Disabled) {
            flags |= mp_services::PROCESSOR_ENABLED_BIT;
        }
        if self.mp.ap_healthy(index) {
            flags |= mp_services::PROCESSOR_HEALTH_STATUS_BIT;
        }
        flags
    }

    /// Signals `targets` immediately, then either blocks until they finish (no
    /// `completion`) or registers the dispatch for the periodic poll. A single shared
    /// deadline bounds the whole operation, so a hung AP cannot extend the timeout
    /// for the others.
    fn dispatch(
        &self,
        targets: Targets,
        single_thread: bool,
        work: ApWorkItem,
        timeout_us: usize,
        completion: Option<DispatchCompletion>,
    ) -> Result<(), MpError> {
        if completion.is_some() && self.ready_to_boot.load(Ordering::Acquire) {
            return Err(MpError::NotSupported);
        }
        let deadline = deadline_for(self.timer, timeout_us);

        // The registry lock is the dispatch lock: it raises to TPL_NOTIFY, which the
        // periodic poll also needs, so no processor can be taken between being selected
        // and being signaled.
        let started = self.notifications.with_dispatch_lock(|pending| {
            let mut queued = self.select(&targets, pending.as_slice())?;
            let mut active: Vec<(usize, u64)> = Vec::new();
            let mut failed: Vec<usize> = Vec::new();

            // Signal every AP concurrently, or just the first for a single-thread run
            // (the rest are launched one at a time as each finishes). Each signaled AP
            // is tracked with the work id returned by `signal_ap` so its completion can
            // be detected unambiguously. An AP that cannot be signaled is recorded as
            // failed rather than silently skipped.
            if single_thread {
                if !queued.is_empty() {
                    let first = queued.remove(0);
                    match self.mp.signal_ap(first, work) {
                        Some(id) => active.push((first, id)),
                        None => failed.push(first),
                    }
                }
            } else {
                for &i in &queued {
                    match self.mp.signal_ap(i, work) {
                        Some(id) => active.push((i, id)),
                        None => failed.push(i),
                    }
                }
                queued.clear();
            }

            // Non-blocking: hand the started dispatch to the periodic poll, which waits
            // for completion and reports it.
            match completion {
                Some(completion) => {
                    pending.push(PendingDispatch::new(
                        work,
                        single_thread,
                        queued,
                        active,
                        failed,
                        deadline,
                        completion,
                    ));
                    Ok(None)
                }
                None => Ok(Some((queued, active, failed))),
            }
        })?;

        let Some((queued, active, mut failed)) = started else { return Ok(()) };

        // Blocking: wait on the started AP(s), launching any remaining single-thread
        // APs as each finishes, all bounded by the shared deadline.
        for &(i, id) in &active {
            if !self.wait_ap_until(i, id, deadline) {
                self.fence_off(i);
                failed.push(i);
            }
        }
        for &i in &queued {
            if self.deadline_passed(deadline) {
                failed.push(i);
                continue;
            }
            match self.mp.signal_ap(i, work) {
                Some(id) if self.wait_ap_until(i, id, deadline) => {}
                Some(_) => {
                    self.fence_off(i);
                    failed.push(i);
                }
                None => failed.push(i),
            }
        }
        if failed.is_empty() { Ok(()) } else { Err(MpError::Timeout(failed)) }
    }

    /// Resolves the processors a dispatch will target.
    ///
    /// Must be called under the dispatch lock: a processor that a pending dispatch
    /// has queued but not yet signaled still looks idle, so it can only be excluded
    /// while that list cannot change.
    fn select(&self, targets: &Targets, pending: &[PendingDispatch]) -> Result<Vec<usize>, MpError> {
        let claimed = |index: usize| pending.iter().any(|dispatch| dispatch.claims(index));
        match *targets {
            Targets::One(index) => {
                if index == MpSupport::BSP_INDEX {
                    return Err(MpError::InvalidProcessor);
                }
                if index >= self.mp.processor_count() {
                    return Err(MpError::NotFound);
                }
                if claimed(index) {
                    return Err(MpError::Busy);
                }
                match self.mp.ap_availability(index) {
                    ProcessorState::NotStarted => Err(MpError::NotStarted),
                    ProcessorState::Disabled => Err(MpError::InvalidProcessor),
                    ProcessorState::Busy => Err(MpError::Busy),
                    ProcessorState::Ready => Ok(alloc::vec![index]),
                }
            }
            Targets::AllEligible => {
                let mut aps = Vec::new();
                for i in MpSupport::BSP_INDEX + 1..self.mp.processor_count() {
                    if claimed(i) {
                        return Err(MpError::Busy);
                    }
                    match self.mp.ap_availability(i) {
                        ProcessorState::Ready => aps.push(i),
                        ProcessorState::Busy => return Err(MpError::Busy),
                        ProcessorState::NotStarted | ProcessorState::Disabled => {}
                    }
                }
                if aps.is_empty() { Err(MpError::NotStarted) } else { Ok(aps) }
            }
        }
    }

    /// Excludes an AP that missed its deadline from future dispatches. Without this
    /// the AP stays busy forever and blocks every later request. Re-enabling it
    /// through `EnableDisableAP` restores it once its work actually completes.
    fn fence_off(&self, index: usize) {
        log::warn!("Processor {index} did not finish its dispatch in time; disabling it");
        self.mp.set_ap_enabled(index, false, Some(false));
    }

    fn deadline_passed(&self, deadline: Option<u64>) -> bool {
        deadline.is_some_and(|deadline| self.timer.cpu_count() >= deadline)
    }

    /// Waits for AP `index` to complete the dispatch identified by `work_id`,
    /// bounded by the shared `deadline` (timer ticks; `None` waits indefinitely),
    /// calling `wait_ap` with the time remaining.
    fn wait_ap_until(&self, index: usize, work_id: u64, deadline: Option<u64>) -> bool {
        let Some(deadline) = deadline else {
            return self.mp.wait_ap(index, work_id, 0);
        };
        let now = self.timer.cpu_count();
        if now >= deadline {
            // Deadline already elapsed; report only whether the AP happened to finish.
            return self.mp.ap_finished(index, work_id);
        }
        // `deadline` is `Some` only when the timer frequency is non-zero.
        let remaining_us = ((deadline - now) as u128 * 1_000_000 / self.timer.perf_frequency() as u128) as usize;
        self.mp.wait_ap(index, work_id, remaining_us.max(1))
    }

    /// Runs `work` on all started APs. Only callable from the BSP.
    ///
    /// Without a `completion` the call blocks until every AP finishes, failing with
    /// [`MpError::Timeout`] and the processors that did not. With one, it returns
    /// immediately after starting the APs and the periodic poll invokes `completion`.
    pub(super) fn startup_all_aps(
        &self,
        work: ApWorkItem,
        single_thread: bool,
        timeout_us: usize,
        completion: Option<DispatchCompletion>,
    ) -> Result<(), MpError> {
        self.bsp_check()?;
        self.dispatch(Targets::AllEligible, single_thread, work, timeout_us, completion)
    }

    /// Runs `work` on a single AP. Only callable from the BSP.
    ///
    /// Without a `completion` the call blocks until the AP finishes, failing with
    /// [`MpError::Timeout`] if it does not. With one, it returns immediately and the
    /// periodic poll invokes `completion`.
    pub(super) fn startup_this_ap(
        &self,
        work: ApWorkItem,
        processor_index: usize,
        timeout_us: usize,
        completion: Option<DispatchCompletion>,
    ) -> Result<(), MpError> {
        self.bsp_check()?;
        self.dispatch(Targets::One(processor_index), false, work, timeout_us, completion)
    }

    /// Resolves any pending non-blocking dispatches. Invoked by the component's
    /// periodic timer callback.
    pub(super) fn poll_notifications(&self) {
        self.notifications.poll(&self.mp, self.timer);
    }

    fn bsp_check(&self) -> Result<(), MpError> {
        if self.mp.who_am_i() != Some(MpSupport::BSP_INDEX) { Err(MpError::NotBsp) } else { Ok(()) }
    }
}

/// Placeholder information for a processor the PEI information HOB did not
/// describe. The topology fields are unknown, but the processor still exists and
/// its status flags are derived live.
fn topology_unknown(processor_id: u32) -> mp_services::ProcessorInformation {
    mp_services::ProcessorInformation {
        processor_id: processor_id as u64,
        status_flag: 0,
        location: mp_services::CpuPhysicalLocation { package: 0, core: 0, thread: 0 },
        extended_information: mp_services::ExtendedProcessorInformation {
            location2: mp_services::CpuPhysicalLocation2 { package: 0, module: 0, tile: 0, die: 0, core: 0, thread: 0 },
        },
    }
}

/// Errors from the MP Services operations, mapped to `efi::Status` by the wrappers.
pub(super) enum MpError {
    NotBsp,
    Busy,
    /// The dispatch did not complete on the listed processors before its deadline.
    Timeout(Vec<usize>),
    NotStarted,
    NotSupported,
    InvalidProcessor,
    NotFound,
}

impl From<MpError> for efi::Status {
    fn from(e: MpError) -> Self {
        match e {
            MpError::Timeout(_) => efi::Status::TIMEOUT,
            MpError::NotStarted => efi::Status::NOT_STARTED,
            MpError::Busy => efi::Status::NOT_READY,
            MpError::NotFound => efi::Status::NOT_FOUND,
            MpError::NotSupported => efi::Status::UNSUPPORTED,
            // The PI specification reports a call made from an AP as a device error.
            MpError::NotBsp => efi::Status::DEVICE_ERROR,
            MpError::InvalidProcessor => efi::Status::INVALID_PARAMETER,
        }
    }
}
