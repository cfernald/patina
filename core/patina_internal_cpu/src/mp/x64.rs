//! x86_64 Multiprocessor (MP) startup support.
//!
//! Brings Application Processors (APs) online by migrating them out of the PEI
//! wait loop into the DXE dispatch loop.
//!
//! ## Context Array
//!
//! The [`ApContext`] array is allocated and owned by [`MpSupport`]. Index 0 is
//! always the BSP; indices 1..N are APs. The BSP entry is populated during
//! startup with the BSP's APIC ID (no stack is assigned).
//!
//! ## License
//!
//! Copyright (c) Microsoft Corporation.
//!
//! SPDX-License-Identifier: Apache-2.0
//!

use core::{
    num::NonZeroU64,
    sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering},
};

use patina::{
    UEFI_PAGE_SIZE, bit,
    component::service::{
        memory::{AccessType, AllocationOptions, MemoryManager},
        perf_timer::ArchTimerFunctionality,
    },
    error::EfiError,
    uefi_size_to_pages,
};

use super::control::{ApConsumer, ApStateMachine};
use super::{ApWorkItem, MpDispatcher, MpHandOffInfo, ProcessorState};

mod apic;
mod cpu_state;
mod handoff;

/// Sentinel value indicating an unused entry in [`ApContext::apic_id`].
const APIC_ID_INVALID: u32 = 0xFFFF_FFFF;

/// Stack size allocated per AP.
const AP_STACK_SIZE: usize = 0x8000;

/// Window for APs to migrate into the DXE dispatch loop after being signaled.
const AP_STARTUP_TIMEOUT_US: u64 = 100_000;

/// Maximum time to wait for xAPIC to send the terminal INIT command.
const INIT_DELIVERY_TIMEOUT_US: u64 = 10_000;

/// Conservative interval for every AP to observe INIT before returning to EBS.
const INIT_SETTLE_US: u64 = 10_000;

/// Required wait loop mode for APs to be considered ready.
const REQUIRED_WAIT_LOOP_MODE: u32 = 8;

/// Per-processor context structure.
#[repr(C)]
struct ApContext {
    /// Stack top for this AP. Set by the BSP before AP startup.
    stack_top: AtomicU64,
    /// APIC ID of this AP. The AP will set this as they reserve indices.
    apic_id: AtomicU32,
    /// Dispatch state machine plus the work slot it guards for this AP.
    sm: ApStateMachine,
    /// Architectural state and persistent synchronization storage for this AP.
    cpu_state: cpu_state::ApCpuState,
}

impl ApContext {
    const fn new() -> Self {
        Self {
            stack_top: AtomicU64::new(0),
            apic_id: AtomicU32::new(APIC_ID_INVALID),
            sm: ApStateMachine::new(),
            cpu_state: cpu_state::ApCpuState::new(),
        }
    }
}

impl Default for ApContext {
    fn default() -> Self {
        Self::new()
    }
}

/// Multiprocessor support for x86_64.
pub struct MpSupport {
    processor_count: usize,
    contexts: &'static [ApContext],
    timer: &'static dyn ArchTimerFunctionality,
    /// Calibrated timer frequency, guaranteed nonzero at construction.
    perf_frequency: NonZeroU64,
    /// Terminal gate set before ExitBootServices starts quiescing APs.
    shutting_down: AtomicBool,
}

impl MpSupport {
    fn validate_handoff(handoff: &MpHandOffInfo<'_>) -> bool {
        let bsp_apic_id = Self::get_current_apic_id();
        let x2apic = Self::is_x2apic_enabled();
        let mut bsp_matches = 0;

        for (index, processor) in handoff.processors.iter().enumerate() {
            if processor.processor_id == bsp_apic_id {
                bsp_matches += 1;
            }
            if !x2apic && processor.processor_id > u8::MAX as u32 {
                log::error!("MP handoff processor {index} has an APIC ID that is invalid in xAPIC mode");
                return false;
            }
            if handoff.processors.iter().take(index).any(|prior| prior.processor_id == processor.processor_id) {
                log::error!("MP handoff contains duplicate APIC ID {:#x}", processor.processor_id);
                return false;
            }
            if processor.processor_id != bsp_apic_id
                && (processor.startup_signal_address == 0
                    || !processor.startup_signal_address.is_multiple_of(core::mem::align_of::<u32>() as u64)
                    || processor.startup_procedure_address == 0
                    || !processor.startup_procedure_address.is_multiple_of(core::mem::align_of::<u64>() as u64))
            {
                log::error!("MP handoff processor {index} has invalid startup slot addresses");
                return false;
            }
        }

        if bsp_matches != 1 {
            log::error!("MP handoff contains {bsp_matches} entries for BSP APIC ID {bsp_apic_id:#x}; expected one");
            return false;
        }
        true
    }

    /// Creates a new [`MpSupport`] instance.
    ///
    /// `contexts` is a caller-allocated slice of [`ApContext`] entries.
    /// Index 0 is always the BSP; indices 1..N are APs. A single entry describes a
    /// uniprocessor system, which is supported (every AP operation then reports
    /// that no APs exist).
    ///
    /// `timer` supplies the tick count and frequency used to convert microsecond
    /// delays and dispatch timeouts into timestamp-counter ticks.
    ///
    /// # Safety
    ///
    /// The caller must guarantee that `contexts` remains valid for the lifetime
    /// of the MP subsystem.
    ///
    /// # Returns
    ///
    /// `Some(MpSupport)` if the parameters meet the requirements, `None` otherwise.
    unsafe fn create(
        contexts: &'static [ApContext],
        timer: &'static dyn ArchTimerFunctionality,
        perf_frequency: NonZeroU64,
    ) -> Option<Self> {
        if contexts.is_empty() {
            log::error!("contexts must have at least one entry for the BSP");
            return None;
        }

        let processor_count = contexts.len();
        Some(Self { processor_count, contexts, timer, perf_frequency, shutting_down: AtomicBool::new(false) })
    }

    /// Resturns an iterator of AP contexts, skipping the BSP.
    fn ap_iter(&self) -> impl Iterator<Item = &ApContext> {
        self.contexts.iter().skip(1)
    }

    /// Returns the number of APs (total processors minus the BSP).
    fn ap_count(&self) -> usize {
        self.processor_count - 1
    }

    /// Populates the BSP context (index 0) and publishes the AP context array so
    /// that APs can find their own [`ApContext`] entry once they wake.
    fn initialize(&self) {
        // Fill BSP context at index 0.
        let bsp_apic_id = Self::get_current_apic_id();
        let (bsp, aps) = self.contexts.split_first().expect("contexts has at least one entry");
        bsp.apic_id.store(bsp_apic_id, Ordering::Relaxed);
        // The BSP never runs the dispatch loop; claiming its context here keeps anything
        // else from driving it.
        bsp.sm.claim_as_bsp();
        log::info!("BSP APIC ID: {bsp_apic_id:#x} (context index 0)");

        // Prepare each AP's execution environment before publishing the context array.
        let startup_state = cpu_state::capture_bsp_state();
        for ctx in aps {
            ctx.cpu_state.prepare(startup_state);
        }

        // Publish the AP context array so APs can locate themselves by APIC ID.
        handoff::publish_ap_contexts(aps);
        handoff::publish_bsp_gdtr();
    }

    fn is_x2apic_enabled() -> bool {
        apic::is_x2apic_enabled()
    }

    /// Spins until `done` returns true or `timeout_us` microseconds elapse.
    #[inline(never)]
    fn spin_for(&self, timeout_us: u64, mut done: impl FnMut() -> bool) -> bool {
        let ticks = timeout_us.saturating_mul(self.perf_frequency.get()) / 1_000_000;
        let start = self.timer.cpu_count();
        while self.timer.cpu_count().wrapping_sub(start) < ticks {
            if done() {
                return true;
            }
            core::hint::spin_loop();
        }
        done()
    }

    fn spin_delay(&self, delay_us: u64) {
        let ticks = delay_us.saturating_mul(self.perf_frequency.get()) / 1_000_000;
        let start = self.timer.cpu_count();
        while self.timer.cpu_count().wrapping_sub(start) < ticks {
            core::hint::spin_loop();
        }
    }

    /// Halts the calling AP permanently. Only reached before ExitBootServices, when
    /// an AP cannot join the dispatch loop; [`MpDispatcher::park`] later takes it out
    /// of this loop with an INIT.
    fn park_forever() -> ! {
        patina::arch::disable_interrupts();
        loop {
            // SAFETY: Halting is a defined idle state for a processor that is not
            // participating in dispatch.
            unsafe {
                core::arch::asm!("hlt", options(nomem, nostack));
            }
        }
    }

    fn cpuid(leaf: u32, subleaf: u32) -> core::arch::x86_64::CpuidResult {
        core::arch::x86_64::__cpuid_count(leaf, subleaf)
    }

    fn is_monitor_supported() -> bool {
        // Monitor support is indicated by CPUID.01H:ECX.MONITOR[bit 3].
        (Self::cpuid(1, 0).ecx & bit!(3)) != 0
    }

    /// Waits for the AP owning `ctx` to complete the dispatch identified by
    /// `work_id`. Returns true if it completed within `timeout_us` microseconds
    /// (0 = wait forever).
    fn wait_for_ap(&self, ctx: &ApContext, work_id: u64, timeout_us: usize) -> bool {
        if timeout_us == 0 {
            while !ctx.sm.is_finished(work_id) {
                core::hint::spin_loop();
            }
            return true;
        }
        if self.spin_for(timeout_us as u64, || ctx.sm.is_finished(work_id)) {
            return true;
        }
        // On timeout the AP is still working (unavailable); report success only if it
        // finished right at the deadline.
        ctx.sm.is_finished(work_id)
    }

    /// AP-side dispatch loop. All APs will enter this after init and await work from the BSP.
    fn ap_run_dispatch_loop(ctx: &'static ApContext) -> ! {
        if !ctx.cpu_state.apply() {
            Self::park_forever();
        }

        // A context that already has an owner leaves this processor nothing safe to do.
        let Some(ap) = ctx.sm.start() else {
            Self::park_forever();
        };

        let use_mwait = Self::is_monitor_supported();
        loop {
            if ap.execute_pending_work() {
                continue;
            }
            if use_mwait {
                monitor_wait(&ap);
            } else {
                core::hint::spin_loop();
            }
        }
    }

    /// Reads the current processor's APIC ID via CPUID.
    fn get_current_apic_id() -> u32 {
        if Self::is_x2apic_enabled() {
            // x2APIC: CPUID leaf 0xB, sub-leaf 0, EDX = full 32-bit ID.
            Self::cpuid(0xB, 0).edx
        } else {
            // xAPIC: CPUID leaf 1, EBX[31:24] = initial APIC ID.
            Self::cpuid(1, 0).ebx >> 24
        }
    }
}

impl MpDispatcher for MpSupport {
    /// Creates and starts multiprocessor support, migrating each AP out of its
    /// handoff loop into a rust dispatch loop.
    fn initialize(
        mm: &dyn MemoryManager,
        timer: &'static dyn ArchTimerFunctionality,
        handoff: Option<MpHandOffInfo<'_>>,
    ) -> Result<Self, EfiError> {
        let perf_frequency = NonZeroU64::new(timer.perf_frequency()).ok_or_else(|| {
            log::error!("MP Services requires a calibrated timer.");
            EfiError::Unsupported
        })?;

        let handoff = handoff.filter(|handoff| {
            if handoff.wait_loop_execution_mode != REQUIRED_WAIT_LOOP_MODE {
                log::error!(
                    "MP handoff wait-loop execution mode is {} bytes; x86_64 requires processors handed off in \
                     64-bit mode ({REQUIRED_WAIT_LOOP_MODE} bytes). Continuing with the BSP only.",
                    handoff.wait_loop_execution_mode
                );
                return false;
            }
            if handoff.processors.len() < 2 {
                log::warn!(
                    "MP handoff described {} processor(s); continuing with the BSP only.",
                    handoff.processors.len()
                );
                return false;
            }
            Self::validate_handoff(handoff)
        });

        if handoff.is_none() {
            log::warn!("No usable PEI-to-DXE processor handoff; MP Services will report a single processor.");
        }

        // Without a usable handoff there is still a BSP to describe, so the protocol
        // is published for a uniprocessor system rather than withheld entirely.
        let processors = handoff.as_ref().map(|handoff| handoff.processors).unwrap_or_default();
        let startup_signal_value = handoff.as_ref().map_or(0, |handoff| handoff.startup_signal_value);
        let processor_count = processors.len().max(1);

        // Allocate the context array: index 0 = BSP, indices 1..N = APs.

        let ctx_pages = uefi_size_to_pages!(processor_count * core::mem::size_of::<ApContext>());
        let ctx_alloc = mm.allocate_pages(ctx_pages, AllocationOptions::new()).map_err(|e| {
            log::error!("Failed to allocate MP context array: {e:?}");
            EfiError::OutOfResources
        })?;
        let contexts =
            ctx_alloc.leak_as_slice::<ApContext>().get(0..processor_count).ok_or(EfiError::OutOfResources)?;

        // Create the MpSupport struct to manage the APs.

        // SAFETY: `contexts` was just leaked from a page allocation, so it lives for
        // the remainder of the boot.
        let mp = unsafe { MpSupport::create(contexts, timer, perf_frequency) }.ok_or_else(|| {
            log::error!("Failed to create MpSupport");
            EfiError::DeviceError
        })?;

        // Give each AP (indices 1..N) a stack with a guard page at the bottom.

        let stack_pages = uefi_size_to_pages!(AP_STACK_SIZE);
        let total_pages = stack_pages + 1;
        for ctx in mp.ap_iter() {
            let stack_alloc = mm.allocate_pages(total_pages, AllocationOptions::new()).map_err(|e| {
                log::error!("Failed to allocate AP stack: {e:?}");
                EfiError::OutOfResources
            })?;
            let alloc_base = stack_alloc.into_raw_ptr::<u8>().ok_or(EfiError::OutOfResources)? as usize;

            // Setup the guard page.
            // SAFETY: The page is freshly allocated and we are setting it to NoAccess
            //         to create a guard page that will fault on stack overflow.
            unsafe {
                mm.set_page_attributes(alloc_base, 1, AccessType::NoAccess, None).map_err(|e| {
                    log::error!("Failed to set guard page attributes: {e:?}");
                    EfiError::DeviceError
                })?;
            }

            let stack_top = alloc_base + (total_pages * UEFI_PAGE_SIZE);
            ctx.stack_top.store(stack_top as u64, Ordering::Relaxed);
        }

        // Populate the BSP context (index 0) and publish the AP context array.

        mp.initialize();
        let bsp_apic_id = Self::get_current_apic_id();

        // Assign each non-BSP handoff entry to an AP context slot, recording its
        // APIC ID so the AP can find itself once it wakes.

        let (_, ap_contexts) = mp.contexts.split_first().expect("contexts has at least one entry");
        let ap_handoffs = || processors.iter().filter(|p| p.processor_id != bsp_apic_id);
        let assigned = ap_contexts.iter().zip(ap_handoffs()).count();
        for (ctx, p) in ap_contexts.iter().zip(ap_handoffs()) {
            ctx.apic_id.store(p.processor_id, Ordering::Relaxed);
        }

        if assigned < ap_contexts.len() {
            log::warn!("Handoff described {assigned} AP(s) but {} context slot(s) exist", ap_contexts.len());
        }

        // Wake each AP by writing the entry point into its handoff procedure
        // slot and raising its startup signal.

        let ap_count = mp.ap_count();
        if ap_count == 0 {
            log::info!("MP Services initialized: uniprocessor system (BSP only)");
            return Ok(mp);
        }

        log::info!("Waking APs via handoff...");
        let start_timestamp = timer.cpu_count();
        for p in ap_handoffs() {
            // SAFETY: the addresses come from the PEI handoff for APs still parked
            // in their wait loop; `wake_ap` publishes the entry before the signal.
            unsafe {
                handoff::wake_ap(p.startup_procedure_address, p.startup_signal_address, startup_signal_value);
            }
        }

        // Wait for all APs to migrate into the dispatch loop.
        mp.spin_for(AP_STARTUP_TIMEOUT_US, || mp.started_processor_count().saturating_sub(1) >= ap_count);
        let started_count = mp.started_processor_count().saturating_sub(1);
        let ap_start_time = (timer.cpu_count().saturating_sub(start_timestamp) * 1_000_000) / perf_frequency.get();

        log::info!("MP Services initialized: {started_count}/{ap_count} APs started in {ap_start_time} us");
        Ok(mp)
    }

    fn processor_count(&self) -> usize {
        self.processor_count
    }

    fn started_processor_count(&self) -> usize {
        self.contexts.iter().filter(|ctx| ctx.sm.state() != ProcessorState::NotStarted).count()
    }

    fn enabled_processor_count(&self) -> usize {
        self.contexts
            .iter()
            .filter(|ctx| !matches!(ctx.sm.state(), ProcessorState::NotStarted | ProcessorState::Disabled))
            .count()
    }

    fn set_ap_enabled(&self, index: usize, enabled: bool, healthy: Option<bool>) -> bool {
        if index == Self::BSP_INDEX {
            return false;
        }
        let Some(ctx) = self.contexts.get(index) else {
            return false;
        };
        if let Some(healthy) = healthy {
            ctx.sm.set_healthy(healthy);
        }
        ctx.sm.set_enabled(enabled);
        true
    }

    fn ap_healthy(&self, index: usize) -> bool {
        self.contexts.get(index).is_some_and(|ctx| ctx.sm.is_healthy())
    }

    fn who_am_i(&self) -> Option<usize> {
        let apic_id = Self::get_current_apic_id();
        self.contexts.iter().position(|ctx| ctx.apic_id.load(Ordering::Relaxed) == apic_id)
    }

    fn processor_id(&self, index: usize) -> Option<u32> {
        self.contexts.get(index).map(|ctx| ctx.apic_id.load(Ordering::Relaxed))
    }

    fn ap_finished(&self, index: usize, work_id: u64) -> bool {
        self.contexts.get(index).is_some_and(|ctx| ctx.sm.is_finished(work_id))
    }

    fn ap_availability(&self, index: usize) -> ProcessorState {
        self.contexts.get(index).map_or(ProcessorState::NotStarted, |ctx| ctx.sm.state())
    }

    fn signal_ap(&self, index: usize, work: ApWorkItem) -> Option<u64> {
        if self.shutting_down.load(Ordering::Acquire) {
            return None;
        }
        self.contexts.get(index).and_then(|ctx| ctx.sm.dispatch(work).ok())
    }

    fn wait_ap(&self, index: usize, work_id: u64, timeout_us: usize) -> bool {
        self.contexts.get(index).is_some_and(|ctx| self.wait_for_ap(ctx, work_id, timeout_us))
    }

    fn begin_shutdown(&self) {
        self.shutting_down.store(true, Ordering::Release);
    }

    fn park(&self) {
        // The broadcast covers every AP, including ones that never joined the dispatch
        // loop and are still sitting in PEI code. A processor left in wait-for-SIPI runs
        // nothing and references no memory, so nothing the OS reclaims can matter, and
        // it is exactly where the OS's INIT-SIPI-SIPI bring-up expects it.
        self.begin_shutdown();
        apic::send_init_all_excluding_self();
        if !self.spin_for(INIT_DELIVERY_TIMEOUT_US, apic::init_delivery_complete) {
            log::warn!("Timed out waiting for the local APIC to send terminal INIT");
        }
        self.spin_delay(INIT_SETTLE_US);
    }

    fn sync_ap(&self, index: usize) -> Option<u64> {
        if self.shutting_down.load(Ordering::Acquire) {
            return None;
        }
        let ctx = self.contexts.get(index)?;
        // The snapshot is only writable while the processor is idle. The caller holds
        // the dispatch lock across this call, so nothing else can take the processor
        // between here and the dispatch below.
        if ctx.sm.state() != ProcessorState::Ready {
            return None;
        }

        let work = ctx.cpu_state.prepare_mtrr_sync()?;
        ctx.sm.dispatch(work).ok()
    }
}

/// Sleeps in `MWAIT` until the BSP writes this processor's dispatch word.
fn monitor_wait(ap: &ApConsumer<'_>) {
    // SAFETY: MONITOR arms address monitoring on the dispatch word, a live writable
    // word in write-back memory.
    unsafe {
        core::arch::asm!(
            "monitor",
            in("rax") ap.monitor_address(),
            in("rcx") 0,
            in("rdx") 0,
            options(nostack, preserves_flags),
        );
    }
    // Re-check after arming; skip MWAIT if work already arrived so the wakeup is not lost.
    if !ap.work_pending() {
        // SAFETY: MWAIT idles until a store to the monitored line (or another break
        // event); no side effects beyond resuming execution.
        unsafe {
            core::arch::asm!(
                "mwait",
                in("rax") 0,
                in("rcx") 0,
                options(nostack, preserves_flags),
            );
        }
    }
}
