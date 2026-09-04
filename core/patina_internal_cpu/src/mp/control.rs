//! Common structure for the AP state machine producer and consumer.
//!
//! ## License
//!
//! Copyright (c) Microsoft Corporation.
//!
//! SPDX-License-Identifier: Apache-2.0
//!

use core::cell::UnsafeCell;
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use patina::bit;

use crate::mp::ProcessorState;

use super::ApWorkItem;

/// Lifecycle of a processor within the dispatch protocol.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u64)]
enum RunState {
    /// Has not joined the dispatch loop.
    NotStarted = 0,
    /// Started and waiting for work.
    Idle = 1,
    /// Work published, but not yet claimed by the AP.
    Signalled = 2,
    /// The AP is executing the work item.
    Running = 3,
}

impl RunState {
    fn from_bits(bits: u64) -> Self {
        match bits {
            1 => Self::Idle,
            2 => Self::Signalled,
            3 => Self::Running,
            _ => Self::NotStarted,
        }
    }
}

/// The run state and monotonically increasing dispatch identifier packed into one
/// word. This keeps completion and availability in a single atomic snapshot.
///
/// e.g.
///   Dispatch 0 (init):
///     NotStarted (0x0) -> Idle (0x1)
///   Dispatch 1:
///     Signalled (0x12) -> Running (0x13) -> Idle (0x11)
///   Dispatch 2:
///     Signalled (0x22) -> Running (0x23) -> Idle (0x21)
///
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct DispatchState(u64);

impl DispatchState {
    /// Width of the run-state field at the bottom of the packed dispatch word.
    const RUN_STATE_BITS: u32 = 4;
    const RUN_STATE_MASK: u64 = bit!(Self::RUN_STATE_BITS) - 1;

    const fn new(dispatch_id: u64, run: RunState) -> Self {
        Self((dispatch_id << Self::RUN_STATE_BITS) | run as u64)
    }

    fn run_state(self) -> RunState {
        RunState::from_bits(self.0 & Self::RUN_STATE_MASK)
    }

    fn dispatch_id(self) -> u64 {
        self.0 >> Self::RUN_STATE_BITS
    }

    fn with_run_state(self, run: RunState) -> Self {
        Self::new(self.dispatch_id(), run)
    }

    fn with_next_dispatch(self, run: RunState) -> Self {
        Self::new(self.dispatch_id() + 1, run)
    }
}

/// Per-AP dispatch state machine. A single producer (the BSP) hands work to a
/// single consumer (the owning AP). The packed [`DispatchState`] both sequences the
/// handoff and, through its release/acquire pairing, publishes the work slot without
/// a lock.
///
/// The consumer side is reachable only through [`ApStateMachine::start`], which uses a
/// compare-exchange that can succeed only once, so the single-consumer requirement the
/// `Sync` impl rests on is enforced rather than assumed.
///
/// A dispatch is identified by the id it advanced, so a specific dispatch
/// stays distinguishable even after the AP is reused, and a processor that never
/// completes its work can never hand out that id again.
pub(crate) struct ApStateMachine {
    state: AtomicU64,
    enabled: AtomicBool,
    healthy: AtomicBool,
    work: UnsafeCell<Option<ApWorkItem>>,
}

// SAFETY: Only the processor that wins `start` ever claims work, and only the BSP
// ever publishes it, so there is no concurrent access to the `UnsafeCell` work slot.
// The run state controls the handoff.
unsafe impl Sync for ApStateMachine {}

impl ApStateMachine {
    pub(crate) const fn new() -> Self {
        Self {
            state: AtomicU64::new(DispatchState::new(0, RunState::NotStarted).0),
            enabled: AtomicBool::new(true),
            healthy: AtomicBool::new(true),
            work: UnsafeCell::new(None),
        }
    }

    fn load(&self) -> DispatchState {
        DispatchState(self.state.load(Ordering::Acquire))
    }

    /// Moves the run state from `from` to `to`, preserving the dispatch id. Fails,
    /// without writing, if the processor is not in `from`.
    fn transition(&self, from: RunState, to: RunState, success: Ordering) -> bool {
        self.state
            .fetch_update(success, Ordering::Acquire, |current| {
                let word = DispatchState(current);
                (word.run_state() == from).then(|| word.with_run_state(to).0)
            })
            .is_ok()
    }

    pub(crate) fn state(&self) -> ProcessorState {
        match self.load().run_state() {
            RunState::NotStarted => ProcessorState::NotStarted,
            _ if !self.is_enabled() => ProcessorState::Disabled,
            RunState::Idle => ProcessorState::Ready,
            RunState::Signalled | RunState::Running => ProcessorState::Busy,
        }
    }

    pub(crate) fn is_enabled(&self) -> bool {
        self.enabled.load(Ordering::Acquire)
    }

    pub(crate) fn set_enabled(&self, enabled: bool) {
        self.enabled.store(enabled, Ordering::Release);
    }

    pub(crate) fn is_healthy(&self) -> bool {
        self.healthy.load(Ordering::Relaxed)
    }

    pub(crate) fn set_healthy(&self, healthy: bool) {
        self.healthy.store(healthy, Ordering::Relaxed);
    }

    /// Claims the AP context for the calling processor, returning a consumer handle that can
    /// be used to run work. None is returned in the event the context is already claimed by
    /// another processor. This guarantees exclusive access to the AP context for the claiming processor.
    pub(crate) fn start(&self) -> Option<ApConsumer<'_>> {
        self.transition(RunState::NotStarted, RunState::Idle, Ordering::Release).then_some(ApConsumer { sm: self })
    }

    /// Publishes `work` to this processor, returning the id identifying the dispatch.
    ///
    /// Callers must serialize dispatches against one another: the work slot is
    /// written before the state is published, which is only sound because nothing
    /// else can take an idle processor in the meantime.
    pub(crate) fn dispatch(&self, work: ApWorkItem) -> Result<u64, ProcessorState> {
        let state = self.state();
        if state != ProcessorState::Ready {
            return Err(state);
        }
        // SAFETY: the processor is idle, so the AP is not looking at the slot, and the
        // caller's dispatch lock keeps any other producer out until the store below
        // publishes the work.
        unsafe { *self.work.get() = Some(work) };
        self.state
            .fetch_update(Ordering::Release, Ordering::Acquire, |current| {
                let word = DispatchState(current);
                (word.run_state() == RunState::Idle).then(|| word.with_next_dispatch(RunState::Signalled).0)
            })
            .map(|previous| DispatchState(previous).dispatch_id() + 1)
            .map_err(|_| self.state())
    }

    // Checks whether the dispatch identified by `id` has finished.
    pub(crate) fn is_finished(&self, id: u64) -> bool {
        let word = self.load();
        word.dispatch_id() > id || (word.dispatch_id() == id && word.run_state() == RunState::Idle)
    }

    /// Simulates an AP that entered its work item and never came back.
    #[cfg(test)]
    fn wedge(&self) -> bool {
        self.transition(RunState::Signalled, RunState::Running, Ordering::Acquire)
    }
}

/// The consumer half of the state machine, owned by the AP the context belongs to.
///
/// Obtainable only by [`ApStateMachine::start`], and only once, so the producer side
/// cannot run work and two processors cannot share a context.
///
/// The owning AP drives it from its own loop, deciding for itself how to wait between
/// dispatches; [`Self::monitor_address`] and [`Self::work_pending`] are what an
/// architecture needs to park rather than spin.
pub(crate) struct ApConsumer<'a> {
    sm: &'a ApStateMachine,
}

impl ApConsumer<'_> {
    /// Runs whatever work the BSP has published, returning whether anything ran.
    pub(crate) fn execute_pending_work(&self) -> bool {
        // Try to transition to the running state. If this fails, there is no work to execute.
        if !self.sm.transition(RunState::Signalled, RunState::Running, Ordering::Acquire) {
            return false;
        }

        // SAFETY: The AP has been transitioning to running and has exclusive access to the work.
        let work = unsafe { (*self.sm.work.get()).take() };
        let ran = match work {
            Some(work) => {
                work.run();
                true
            }
            None => false,
        };
        self.sm.transition(RunState::Running, RunState::Idle, Ordering::Release);
        ran
    }

    /// Whether the BSP has published work this processor has not picked up yet.
    ///
    /// A caller that arms a monitor must re-check this *after* arming it, or it can
    /// sleep through the store that was meant to wake it.
    pub(crate) fn work_pending(&self) -> bool {
        self.sm.load().run_state() == RunState::Signalled
    }

    /// Address to arm a monitor on, for architectures that have one.
    pub(crate) fn monitor_address(&self) -> *const u64 {
        self.sm.state.as_ptr()
    }
}

#[cfg_attr(coverage, coverage(off))]
#[cfg(test)]
mod tests {
    use super::*;
    use core::ffi::c_void;
    use core::sync::atomic::AtomicU32;

    // AP procedure: increments the `AtomicU32` pointed to by `arg`.
    unsafe extern "efiapi" fn increment(arg: *mut c_void) {
        // SAFETY: every caller below passes the address of a live `AtomicU32`.
        let counter = unsafe { &*(arg as *const AtomicU32) };
        counter.fetch_add(1, Ordering::SeqCst);
    }

    fn work_for(counter: &AtomicU32) -> ApWorkItem {
        // SAFETY: `increment` matches the `AtomicU32` argument, which outlives every
        // in-test dispatch (the work runs synchronously before the counter drops).
        unsafe { ApWorkItem::new_efi(increment, counter as *const AtomicU32 as *mut c_void) }
    }

    /// Lets an AP procedure observe the machine from inside its own work item, which
    /// is the only point at which the `Running` state is visible.
    struct Probe {
        sm: *const ApStateMachine,
        id: AtomicU64,
        saw_busy: AtomicBool,
        saw_finished: AtomicBool,
    }

    unsafe extern "efiapi" fn observe(arg: *mut c_void) {
        // SAFETY: the test passes the address of a live `Probe`.
        let probe = unsafe { &*(arg as *const Probe) };
        // SAFETY: the probe refers to a state machine that outlives the dispatch.
        let sm = unsafe { &*probe.sm };
        probe.saw_busy.store(sm.state() == ProcessorState::Busy, Ordering::SeqCst);
        probe.saw_finished.store(sm.is_finished(probe.id.load(Ordering::SeqCst)), Ordering::SeqCst);
    }

    #[test]
    fn new_is_not_started() {
        let sm = ApStateMachine::new();
        assert_eq!(sm.state(), ProcessorState::NotStarted);
    }

    #[test]
    fn start_becomes_ready() {
        let sm = ApStateMachine::new();
        let ap = sm.start().expect("a fresh machine can be started");
        assert_eq!(sm.state(), ProcessorState::Ready);
        assert!(!ap.work_pending());
    }

    #[test]
    fn start_succeeds_for_only_one_processor() {
        let sm = ApStateMachine::new();
        let _ap = sm.start().expect("a fresh machine can be started");
        // A second processor reaching the same context gets nothing to drive it with.
        assert!(sm.start().is_none());
    }

    #[test]
    fn execute_pending_work_reports_whether_it_ran() {
        let counter = AtomicU32::new(0);
        let sm = ApStateMachine::new();
        let ap = sm.start().unwrap();

        // Nothing published yet, so there is nothing to run.
        assert!(!ap.execute_pending_work());
        assert_eq!(counter.load(Ordering::SeqCst), 0);

        let id = sm.dispatch(work_for(&counter)).unwrap();
        assert!(ap.execute_pending_work());
        assert_eq!(counter.load(Ordering::SeqCst), 1);
        assert!(sm.is_finished(id));
        assert_eq!(sm.state(), ProcessorState::Ready);
    }

    #[test]
    fn execute_pending_work_runs_each_dispatch_exactly_once() {
        let counter = AtomicU32::new(0);
        let sm = ApStateMachine::new();
        let ap = sm.start().unwrap();

        const CYCLES: u32 = 4;
        for _ in 0..CYCLES {
            sm.dispatch(work_for(&counter)).unwrap();
            assert!(ap.execute_pending_work());
            // A second call finds nothing left to do.
            assert!(!ap.execute_pending_work());
        }
        assert_eq!(counter.load(Ordering::SeqCst), CYCLES);
        assert_eq!(sm.state(), ProcessorState::Ready);
    }

    #[test]
    fn work_pending_tracks_the_publish_and_the_run() {
        let counter = AtomicU32::new(0);
        let sm = ApStateMachine::new();
        let ap = sm.start().unwrap();

        // A caller parking on the dispatch word must only sleep while this is false.
        assert!(!ap.work_pending());
        sm.dispatch(work_for(&counter)).unwrap();
        assert!(ap.work_pending());
        assert!(ap.execute_pending_work());
        assert!(!ap.work_pending());
    }

    #[test]
    fn dispatch_before_started_is_rejected() {
        let counter = AtomicU32::new(0);
        let sm = ApStateMachine::new();
        assert_eq!(sm.dispatch(work_for(&counter)), Err(ProcessorState::NotStarted));
        assert_eq!(counter.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn dispatch_when_ready_marks_busy() {
        let counter = AtomicU32::new(0);
        let sm = ApStateMachine::new();
        let ap = sm.start().unwrap();
        assert_eq!(sm.dispatch(work_for(&counter)), Ok(1));
        assert!(ap.work_pending());
        assert_eq!(sm.state(), ProcessorState::Busy);
    }

    #[test]
    fn dispatch_when_busy_is_rejected() {
        let counter = AtomicU32::new(0);
        let sm = ApStateMachine::new();
        let _ap = sm.start().unwrap();
        sm.dispatch(work_for(&counter)).unwrap();
        assert_eq!(sm.dispatch(work_for(&counter)), Err(ProcessorState::Busy));
    }

    #[test]
    fn disabled_rejects_dispatch_and_outranks_busy() {
        let counter = AtomicU32::new(0);
        let sm = ApStateMachine::new();
        let _ap = sm.start().unwrap();
        sm.set_enabled(false);
        assert_eq!(sm.state(), ProcessorState::Disabled);
        assert_eq!(sm.dispatch(work_for(&counter)), Err(ProcessorState::Disabled));

        sm.set_enabled(true);
        sm.dispatch(work_for(&counter)).unwrap();
        // A wedged AP is fenced off while still busy, and must not report as busy or
        // it would keep blocking dispatches to the other processors.
        sm.set_enabled(false);
        assert_eq!(sm.state(), ProcessorState::Disabled);
    }

    #[test]
    fn re_enabling_a_busy_ap_leaves_it_busy() {
        let counter = AtomicU32::new(0);
        let sm = ApStateMachine::new();
        let ap = sm.start().unwrap();
        sm.dispatch(work_for(&counter)).unwrap();
        sm.set_enabled(false);
        sm.set_enabled(true);
        assert_eq!(sm.state(), ProcessorState::Busy);

        assert!(ap.execute_pending_work());
        assert_eq!(sm.state(), ProcessorState::Ready);
    }

    #[test]
    fn health_defaults_to_healthy_and_is_settable() {
        let sm = ApStateMachine::new();
        assert!(sm.is_healthy());
        sm.set_healthy(false);
        assert!(!sm.is_healthy());
    }

    #[test]
    fn work_runs_while_the_processor_is_still_busy() {
        let sm = ApStateMachine::new();
        let ap = sm.start().unwrap();
        let probe = Probe {
            sm: &sm,
            id: AtomicU64::new(0),
            saw_busy: AtomicBool::new(false),
            saw_finished: AtomicBool::new(true),
        };

        // SAFETY: `observe` matches the `Probe` argument, which outlives the dispatch.
        let work = unsafe { ApWorkItem::new_efi(observe, &probe as *const Probe as *mut c_void) };
        let id = sm.dispatch(work).unwrap();
        probe.id.store(id, Ordering::SeqCst);

        assert!(ap.execute_pending_work());
        assert!(probe.saw_busy.load(Ordering::SeqCst), "work must run before the processor goes idle");
        assert!(!probe.saw_finished.load(Ordering::SeqCst), "the dispatch must not report finished while running");
        assert!(sm.is_finished(id));
        assert_eq!(sm.state(), ProcessorState::Ready);
    }

    #[test]
    fn is_finished_flips_only_once_the_work_has_run() {
        let counter = AtomicU32::new(0);
        let sm = ApStateMachine::new();
        let ap = sm.start().unwrap();
        let id = sm.dispatch(work_for(&counter)).unwrap();

        assert!(!sm.is_finished(id));
        assert!(ap.execute_pending_work());
        assert!(sm.is_finished(id));
    }

    #[test]
    fn work_ids_are_monotonic_and_latch() {
        let counter = AtomicU32::new(0);
        let sm = ApStateMachine::new();
        let ap = sm.start().unwrap();

        let id0 = sm.dispatch(work_for(&counter)).unwrap();
        assert!(ap.execute_pending_work());

        let id1 = sm.dispatch(work_for(&counter)).unwrap();
        assert!(id1 > id0, "each dispatch must yield a fresh, larger id");
        // The earlier dispatch stays finished; the new one is not yet.
        assert!(sm.is_finished(id0));
        assert!(!sm.is_finished(id1));

        assert!(ap.execute_pending_work());
        assert!(sm.is_finished(id1));
    }

    #[test]
    fn monitor_address_is_stable_and_non_null() {
        let sm = ApStateMachine::new();
        let ap = sm.start().unwrap();
        let addr = ap.monitor_address();
        assert!(!addr.is_null());
        assert_eq!(addr, ap.monitor_address());
    }

    #[test]
    fn dispatch_publishes_work_and_wakes_only_once() {
        let counter = AtomicU32::new(0);
        let sm = ApStateMachine::new();
        let ap = sm.start().unwrap();

        // An idle processor's word is untouched until there is real work for it, so an
        // AP monitoring it is not woken by bookkeeping.
        let idle = sm.load();
        assert_eq!(sm.state(), ProcessorState::Ready);
        assert!(!ap.work_pending());
        assert_eq!(sm.load(), idle);

        let id = sm.dispatch(work_for(&counter)).expect("an idle processor accepts work");
        assert!(ap.work_pending());
        assert_ne!(sm.load(), idle);

        assert!(ap.execute_pending_work());
        assert_eq!(counter.load(Ordering::SeqCst), 1);
        assert!(sm.is_finished(id));
    }

    #[test]
    fn dispatch_advances_the_id_so_ids_are_never_reused() {
        let counter = AtomicU32::new(0);
        let sm = ApStateMachine::new();
        let _ap = sm.start().unwrap();

        // A processor whose work never returns keeps its id and cannot hand out another.
        let stuck = sm.dispatch(work_for(&counter)).unwrap();
        assert!(sm.wedge());
        assert_eq!(sm.state(), ProcessorState::Busy);
        assert!(!sm.is_finished(stuck));

        sm.set_enabled(false);
        sm.set_enabled(true);
        assert_eq!(sm.dispatch(work_for(&counter)), Err(ProcessorState::Busy));
        assert!(!sm.is_finished(stuck));
    }

    #[test]
    fn disabled_processor_can_still_finish_its_in_flight_work() {
        let counter = AtomicU32::new(0);
        let sm = ApStateMachine::new();
        let ap = sm.start().unwrap();

        let id = sm.dispatch(work_for(&counter)).unwrap();
        // The BSP fences the processor off while it is mid-dispatch; the AP side must
        // not be blocked by the flag or it would wedge forever.
        sm.set_enabled(false);
        assert!(ap.execute_pending_work(), "a disabled processor still runs published work");
        assert!(sm.is_finished(id));
        assert_eq!(sm.state(), ProcessorState::Disabled);

        sm.set_enabled(true);
        assert_eq!(sm.state(), ProcessorState::Ready);
    }
}
