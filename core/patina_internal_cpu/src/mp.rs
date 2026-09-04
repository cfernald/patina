//! Multiprocessor (MP) support for Patina.
//!
//! Provides the architecture abstraction for multiprocessor (MP) support.
//!
//! ## License
//!
//! Copyright (c) Microsoft Corporation.
//!
//! SPDX-License-Identifier: Apache-2.0
//!

use core::ffi::c_void;

use patina::standard::efi::protocols::mp_services::ApProcedure;

#[cfg(target_arch = "x86_64")] // Will use for aarch64 too.
mod control;

#[cfg(target_arch = "x86_64")]
mod x64;

cfg_if::cfg_if! {
    if #[cfg(target_arch = "x86_64")] {
        pub use x64::{ApContext, MpSupport};
    } else {
        mod stub;
        pub use stub::{ApContext, MpSupport};
    }
}

/// Dispatch eligibility of an AP, derived from its dispatch state.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ProcessorState {
    /// Not started yet. Cannot accept work.
    NotStarted,
    /// Idle or finished with previous work. Ready for a new dispatch.
    Ready,
    /// Still running a prior dispatch. Cannot accept work.
    Busy,
    /// Excluded from dispatch, either by request or after failing to complete a
    /// prior dispatch within its timeout.
    Disabled,
}

/// Identity of a processor known to the MP dispatcher.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Processor {
    /// The bootstrap processor.
    Bsp,
    /// An application processor identified by its zero-based AP index.
    Ap(usize),
}

/// PEI-to-DXE handoff record for a single logical processor.
///
/// Mirrors the EDK II `PROCESSOR_HAND_OFF` entry carried in the `MP_HAND_OFF`
/// HOB. The BSP wakes an AP by writing an entry-point address into the slot at
/// `startup_procedure_address` and then the agreed signal value into the slot at
/// `startup_signal_address`, which the AP is monitoring from its PEI wait loop.
#[derive(Clone, Copy)]
pub struct ProcessorHandOff {
    /// Architecture specific ID of the processor.
    pub processor_id: u32,
    /// Whether the processor passed its built-in self-test in PEI.
    pub healthy: bool,
    /// Address of the word the AP monitors for the wake-up signal value.
    pub startup_signal_address: u64,
    /// Address of the slot into which the BSP writes the AP entry-point address.
    pub startup_procedure_address: u64,
}

/// PEI-to-DXE multiprocessor handoff, gathered from the platform HOBs.
pub struct MpHandOffInfo<'a> {
    /// Pointer width (in bytes) of the phase that produced the handoff. APs can
    /// only be woken by the signal mechanism when this matches the consuming
    /// phase's pointer width (8 for x86_64).
    pub wait_loop_execution_mode: u32,
    /// Value the BSP writes to each AP's startup signal address to wake it.
    pub startup_signal_value: u32,
    /// Per-processor handoff records (including the BSP entry, identified by its
    /// APIC ID).
    pub processors: &'a [ProcessorHandOff],
}

/// Architecture abstraction interface for multiprocessor management.
pub trait MpDispatcher: Sized {
    /// Number of application processors known to the dispatcher.
    fn ap_count(&self) -> usize;

    /// Number of application processors that have reported as started.
    fn started_ap_count(&self) -> usize;

    /// Number of application processors that are started and not disabled.
    fn enabled_ap_count(&self) -> usize;

    /// Enables or disables the AP at `index` for dispatch, optionally recording a
    /// new health status. Returns `false` for an out-of-range AP index.
    ///
    /// Disabling an AP that is still running a dispatch fences it off immediately;
    /// it becomes eligible again only if it is re-enabled *and* its work completes.
    fn set_ap_enabled(&self, index: usize, enabled: bool, healthy: Option<bool>) -> bool;

    /// Whether the AP at `index` is currently considered healthy.
    fn ap_healthy(&self, index: usize) -> bool;

    /// Identity of the calling processor.
    fn who_am_i(&self) -> Option<Processor>;

    /// Architectural processor ID of the BSP.
    fn bsp_processor_id(&self) -> u32;

    /// Architectural processor ID recorded for the AP at `index`.
    fn ap_processor_id(&self, index: usize) -> Option<u32>;

    /// Whether the AP at `index` has completed the dispatch identified by `work_id`
    /// (as returned by [`MpDispatcher::signal_ap`]).
    fn ap_finished(&self, index: usize, work_id: u64) -> bool;

    /// Dispatch eligibility of the AP at `index`.
    fn ap_availability(&self, index: usize) -> ProcessorState;

    /// Publishes `work` to the AP at `index` so it runs on its next scheduling,
    /// returning the id identifying this dispatch (for use with
    /// [`MpDispatcher::wait_ap`] / [`MpDispatcher::ap_finished`]). Returns `None`
    /// for an out-of-range index or an AP that cannot currently accept work.
    fn signal_ap(&self, index: usize, work: ApWorkItem) -> Option<u64>;

    /// Blocks until the AP at `index` completes the dispatch identified by `work_id`
    /// or `timeout_us` microseconds elapse (0 waits indefinitely). Returns whether it
    /// completed.
    fn wait_ap(&self, index: usize, work_id: u64, timeout_us: usize) -> bool;

    /// Prevents any new AP dispatch before terminal OS handoff begins.
    fn begin_shutdown(&self);

    /// Quiesces every AP for OS handoff at ExitBootServices, leaving them in the
    /// state the OS's own bring-up expects to find them in.
    ///
    /// This is terminal: the APs cannot be dispatched to again.
    fn park(&self);

    /// Publishes architecture-specific state from the BSP to the AP at `index` and
    /// dispatches it, returning the id identifying the dispatch. On x86_64 this
    /// replicates the BSP's MTRR settings.
    ///
    /// The caller owns scheduling: it selects the processor, holds the dispatch lock
    /// across this call, and waits on the returned id.
    fn sync_ap(&self, index: usize) -> Option<u64>;
}

/// A work-item to be executed by an Application Processor (AP).
///
/// This structure acts as a wrapper to allow the work-item to be
/// passed around without requiring all intermediaries to have to
/// uphold the safety predicates.
#[derive(Clone, Copy)]
pub struct ApWorkItem {
    procedure: ApProcedure,
    argument: *mut c_void,
}

impl ApWorkItem {
    /// Create a new AP work item.
    ///
    /// ## Safety
    ///
    /// The caller must ensure that:
    ///
    /// 1. The procedure is correct and can handle appropriate multi-threading scenarios.
    /// 2. The argument provided is valid and matches the expectations of `procedure`.
    /// 3. The argument is multi-thread safe, if applicable.
    /// 4. The argument's lifetime guarantees match the duration of the dispatch.
    ///
    pub unsafe fn new_efi(procedure: ApProcedure, argument: *mut c_void) -> Self {
        Self { procedure, argument }
    }

    pub fn run(self) {
        // SAFETY: The safety guarantees are satisfied by the safety predicates for creating
        //         this `ApWorkItem` instance.
        unsafe { (self.procedure)(self.argument) };
    }
}

#[cfg_attr(coverage, coverage(off))]
#[cfg(test)]
mod tests {
    use super::*;

    unsafe extern "efiapi" fn set_boolean(arg: *mut c_void) {
        // SAFETY: The test passes the address of a live `bool` as the argument.
        let value = unsafe { &mut *(arg as *mut bool) };
        *value = true;
    }

    #[test]
    fn create_and_call_work_item() {
        let mut called = false;
        // SAFETY: Procedure and argument match, and argument's lifetime matches work_item.
        let work_item = unsafe { ApWorkItem::new_efi(set_boolean, &mut called as *mut bool as *mut c_void) };
        work_item.run();
        assert!(called);
    }
}
