//! Non-blocking MP dispatch notification tracking.
//!
//! ## License
//!
//! Copyright (c) Microsoft Corporation.
//!
//! SPDX-License-Identifier: Apache-2.0
//!

use alloc::vec::Vec;

use patina::{component::service::perf_timer::ArchTimerFunctionality, standard::efi};
use patina_internal_cpu::mp::{ApWorkItem, MpDispatcher, MpSupport};

use crate::tpl_mutex::TplMutex;

use super::services::{DispatchCompletion, ap_to_processor_index};

/// A non-blocking dispatch awaiting completion, resolved by the periodic poll.
pub(super) struct PendingDispatch {
    work: ApWorkItem,
    single_thread: bool,
    /// APs not yet signaled.
    queued: Vec<usize>,
    /// APs currently signaled and monitored for completion.
    active: Vec<(usize, u64)>,
    /// APs that could not be signaled at all.
    unreachable: Vec<usize>,
    /// Performance-counter tick at which the dispatch times out.
    deadline_tick: Option<u64>,
    /// Reports the outcome to the caller once the dispatch resolves.
    completion: DispatchCompletion,
}

// SAFETY: the completion closure holds the caller's out-pointers, which are only
// touched while the registry `TplMutex` is held, and the MP dispatch model is
// single-BSP, so there is no concurrent cross-thread access to a `PendingDispatch`.
unsafe impl Send for PendingDispatch {}

impl PendingDispatch {
    /// Builds a pending dispatch from AP sets that have already been started.
    pub(super) fn new(
        work: ApWorkItem,
        single_thread: bool,
        queued: Vec<usize>,
        active: Vec<(usize, u64)>,
        unreachable: Vec<usize>,
        deadline_tick: Option<u64>,
        completion: DispatchCompletion,
    ) -> Self {
        Self { work, single_thread, queued, active, unreachable, deadline_tick, completion }
    }

    /// Whether this dispatch still owns `index`, either because it has not been
    /// signaled yet or because it has not finished. Such a processor must not be
    /// handed to another dispatch even when it looks idle.
    pub(super) fn claims(&self, index: usize) -> bool {
        self.queued.contains(&index) || self.active.iter().any(|&(i, _)| i == index)
    }

    /// Prunes completed APs and, for single-thread dispatches, launches the next
    /// queued AP once the current one is idle.
    fn advance(&mut self, mp: &MpSupport) {
        self.active.retain(|&(i, id)| !mp.ap_finished(i, id));
        if self.single_thread && self.active.is_empty() && !self.queued.is_empty() {
            let next = self.queued.remove(0);
            match mp.signal_ap(next, self.work) {
                Some(id) => self.active.push((next, id)),
                None => self.unreachable.push(next),
            }
        }
    }

    /// Whether the dispatch has finished all APs or reached its timeout tick.
    fn is_resolved(&self, now: u64) -> bool {
        let all_done = self.queued.is_empty() && self.active.is_empty();
        let timed_out = self.deadline_tick.is_some_and(|deadline| now >= deadline);
        all_done || timed_out
    }

    /// Reports the outcome to the caller. APs still queued or active are reported as
    /// failed (they did not finish before the timeout), and any that are still running
    /// are fenced off so they cannot block later work.
    fn resolve(self, mp: &MpSupport) {
        for &(i, _) in &self.active {
            let processor_index = ap_to_processor_index(i);
            log::warn!("Processor {processor_index} did not finish its dispatch in time; disabling it");
            mp.set_ap_enabled(i, false, Some(false));
        }
        let failed: Vec<usize> = self
            .unreachable
            .iter()
            .copied()
            .chain(self.queued.iter().copied())
            .chain(self.active.iter().map(|&(i, _)| i))
            .map(ap_to_processor_index)
            .collect();
        (self.completion)(&failed);
    }
}

/// Registry of in-flight non-blocking dispatches, resolved by the periodic poll.
pub(super) struct NotificationRegistry {
    pending: TplMutex<Vec<PendingDispatch>>,
}

impl NotificationRegistry {
    pub(super) fn new() -> Self {
        Self { pending: TplMutex::new(efi::TPL_NOTIFY, Vec::new(), "MpNotifications") }
    }

    /// Runs `dispatch` with the registry locked.
    ///
    /// The lock doubles as the MP dispatch lock: it raises to `TPL_NOTIFY`, which the
    /// periodic poll also needs, so no processor can be selected, signaled or taken
    /// while a dispatch is being set up. `dispatch` receives the pending list so it
    /// can both see which processors are already claimed and register itself.
    pub(super) fn with_dispatch_lock<R>(&self, dispatch: impl FnOnce(&mut Vec<PendingDispatch>) -> R) -> R {
        dispatch(&mut self.pending.lock())
    }

    /// Atomically closes dispatch and takes every outstanding notification, then
    /// resolves each request as failed before APs are reset for OS handoff.
    pub(super) fn shutdown(&self, mp: &MpSupport) {
        let pending = {
            let mut pending = self.pending.lock();
            mp.begin_shutdown();
            core::mem::take(&mut *pending)
        };
        for dispatch in pending {
            dispatch.resolve(mp);
        }
    }

    /// Advances every pending dispatch and resolves those that have completed or
    /// timed out. Intended to be called from the periodic timer callback.
    pub(super) fn poll(&self, mp: &MpSupport, timer: &dyn ArchTimerFunctionality) {
        let now = timer.cpu_count();
        let mut resolved: Vec<PendingDispatch> = Vec::new();

        {
            let mut pending = self.pending.lock();
            for dispatch in pending.iter_mut() {
                dispatch.advance(mp);
            }

            // Prune finished dispatches.
            resolved.extend(pending.extract_if(.., |dispatch| dispatch.is_resolved(now)));
        }

        // Resolve outside the lock to avoid re-entrance deadlocks.
        for dispatch in resolved {
            dispatch.resolve(mp);
        }
    }
}

/// Converts a microsecond timeout into an absolute performance-counter tick.
/// Returns `None` (wait indefinitely) for a zero timeout, or when the timer has no
/// calibrated frequency to measure elapsed time against.
pub(super) fn deadline_for(timer: &dyn ArchTimerFunctionality, timeout_us: usize) -> Option<u64> {
    let freq = timer.perf_frequency();
    if timeout_us == 0 || freq == 0 {
        return None;
    }
    let delta = (timeout_us as u128 * freq as u128 / 1_000_000) as u64;
    Some(timer.cpu_count().saturating_add(delta))
}
