//! `AArch64` multiprocessor startup, dispatch, and shutdown using PSCI.
//!
//! ## License
//!
//! Copyright (c) Microsoft Corporation.
//!
//! SPDX-License-Identifier: Apache-2.0
//!

use core::{
    num::{NonZeroU64, NonZeroUsize},
    sync::atomic::{AtomicBool, Ordering},
};

use patina::{
    arch::psci::{AffinityState, Conduit, Error as PsciError, MPIDR_AFFINITY_MASK},
    component::service::{memory::MemoryManager, perf_timer::ArchTimerFunctionality},
    error::EfiError,
};

use super::{ApWorkItem, MpDispatcher, MpStartup, Processor, ProcessorState, control::ApStateMachine};

mod bootstrap;

const STARTUP_TIMEOUT_US: u64 = 1_000_000;
const SHUTDOWN_TIMEOUT_US: u64 = 1_000_000;

#[cfg(test)]
#[mockall::automock]
trait Psci {
    fn affinity_info(&self, processor: u64) -> Result<AffinityState, PsciError>;
    unsafe fn cpu_on(&self, processor: u64, entry: NonZeroU64, context: u64) -> Result<(), PsciError>;
}

enum PowerControl {
    Firmware(Conduit),
    #[cfg(test)]
    Mock(std::boxed::Box<MockPsci>),
}

impl PowerControl {
    fn affinity_info(&self, processor: u64) -> Result<AffinityState, PsciError> {
        match self {
            Self::Firmware(conduit) => conduit.affinity_info(processor),
            #[cfg(test)]
            Self::Mock(mock) => mock.affinity_info(processor),
        }
    }

    unsafe fn cpu_on(&self, processor: u64, entry: NonZeroU64, context: u64) -> Result<(), PsciError> {
        // SAFETY: The caller has validated and published the target's bootstrap
        // environment and owns the off processor. Mocks perform no hardware access.
        unsafe {
            match self {
                Self::Firmware(conduit) => conduit.cpu_on(processor, entry, context),
                #[cfg(test)]
                Self::Mock(mock) => mock.cpu_on(processor, entry, context),
            }
        }
    }
}

/// Persistent context and bootstrap register state for one `AArch64` AP.
#[repr(C, align(64))]
pub struct ApContext {
    boot: bootstrap::CpuState,
    processor_id: u64,
    started: AtomicBool,
    sm: ApStateMachine,
}

impl Default for ApContext {
    fn default() -> Self {
        Self {
            boot: bootstrap::CpuState::default(),
            processor_id: 0,
            started: AtomicBool::new(false),
            sm: ApStateMachine::new(),
        }
    }
}

impl ApContext {
    /// Required usable stack size for each AP, excluding its guard page.
    pub const STACK_SIZE: usize = 0x8000;

    /// Records the top of the stack provisioned for this AP.
    ///
    /// # Safety
    ///
    /// `stack_top` must identify the top of writable, exclusively owned stack
    /// storage that remains valid until this AP is powered off.
    pub unsafe fn set_stack_top(&mut self, stack_top: NonZeroUsize) -> Result<(), EfiError> {
        if !stack_top.get().is_multiple_of(16) {
            return Err(EfiError::InvalidParameter);
        }
        self.boot.stack_top = stack_top.get() as u64;
        Ok(())
    }
}

/// `AArch64` application processor management using the platform's PSCI conduit.
pub struct MpSupport {
    contexts: &'static [ApContext],
    bsp_processor_id: u64,
    power: Option<PowerControl>,
    timer: &'static dyn ArchTimerFunctionality,
    perf_frequency: NonZeroU64,
    shutting_down: AtomicBool,
}

impl MpSupport {
    fn validate_processors(processors: &[u64], bsp: u64) -> Result<(), EfiError> {
        if processors.iter().filter(|&&id| id == bsp).count() != 1 {
            log::error!("PSCI processor inventory must contain exactly one BSP MPIDR {bsp:#x}");
            return Err(EfiError::InvalidParameter);
        }
        for (index, &id) in processors.iter().enumerate() {
            if id & !MPIDR_AFFINITY_MASK != 0 || processors.iter().take(index).any(|&prior| prior == id) {
                log::error!("Invalid or duplicate PSCI processor affinity {id:#x}");
                return Err(EfiError::InvalidParameter);
            }
        }
        Ok(())
    }

    fn prepare_contexts<'a>(
        contexts: &'a mut [ApContext],
        processors: &[u64],
        bsp: u64,
        state: bootstrap::CpuState,
    ) -> Result<&'a mut [ApContext], EfiError> {
        Self::validate_processors(processors, bsp)?;
        let count = processors.len() - 1;
        if contexts.len() != count || contexts.iter().any(|context| context.boot.stack_top == 0) {
            log::error!("PSCI startup requires exactly {count} AP contexts with provisioned stacks");
            return Err(EfiError::InvalidParameter);
        }
        for (context, &id) in contexts.iter_mut().zip(processors.iter().filter(|&&id| id != bsp)) {
            let stack_top = context.boot.stack_top;
            context.boot = state;
            context.boot.stack_top = stack_top;
            context.processor_id = id;
        }
        Ok(contexts)
    }

    fn wait_until(&self, timeout_us: u64, mut done: impl FnMut() -> bool) -> bool {
        let ticks = (u128::from(timeout_us) * u128::from(self.perf_frequency.get())).div_ceil(1_000_000);
        let start = self.timer.cpu_count();
        loop {
            bootstrap::check_for_failures();
            if done() {
                return true;
            }
            if u128::from(self.timer.cpu_count().wrapping_sub(start)) >= ticks {
                return false;
            }
            core::hint::spin_loop();
        }
    }

    fn find_processor(&self, id: u64) -> Option<Processor> {
        if id == self.bsp_processor_id {
            Some(Processor::Bsp)
        } else {
            self.contexts.iter().position(|context| context.processor_id == id).map(Processor::Ap)
        }
    }

    fn start_aps(&self, entry_point: NonZeroU64) -> Result<(), EfiError> {
        let power = self.power.as_ref().expect("PSCI power control is configured before AP startup");
        for context in self.contexts {
            let power_state = power.affinity_info(context.processor_id).map_err(|error| {
                log::error!("Cannot query MPIDR {:#x} power state: {error}", context.processor_id);
                EfiError::DeviceError
            })?;
            if power_state != AffinityState::Off {
                log::error!("MPIDR {:#x} is not off; another MP provider may own it", context.processor_id);
                return Err(EfiError::DeviceError);
            }
        }
        for context in self.contexts {
            // SAFETY: All targets are off. The caller has published persistent,
            // identity-accessible contexts, stacks, and the bootstrap entry.
            unsafe {
                power.cpu_on(context.processor_id, entry_point, core::ptr::from_ref(context) as u64).unwrap_or_else(
                    |error| panic!("PSCI CPU_ON failed for MPIDR {:#x}: {error}", context.processor_id),
                );
            }
        }
        assert!(
            self.wait_until(STARTUP_TIMEOUT_US, || self.started_ap_count() == self.contexts.len()),
            "PSCI AP startup timed out: {}/{} APs acknowledged; cannot safely abandon pending CPU_ON requests",
            self.started_ap_count(),
            self.contexts.len(),
        );
        log::info!("MP Services initialized using PSCI: {} APs started", self.contexts.len());
        Ok(())
    }
}

impl MpDispatcher for MpSupport {
    type Context = ApContext;

    fn initialize(
        _memory_manager: &dyn MemoryManager,
        timer: &'static dyn ArchTimerFunctionality,
    ) -> Result<Self, EfiError> {
        let perf_frequency = NonZeroU64::new(timer.perf_frequency()).ok_or_else(|| {
            log::error!("AArch64 MP Services requires a calibrated timer");
            EfiError::Unsupported
        })?;
        Ok(Self {
            contexts: &[],
            bsp_processor_id: bootstrap::processor_id(),
            power: None,
            timer,
            perf_frequency,
            shutting_down: AtomicBool::new(false),
        })
    }

    // Register capture and cache publication require real hardware; QEMU boots
    // exercise this bridge, while start_aps tests cover the power-state protocol.
    #[cfg_attr(coverage, coverage(off))]
    fn setup_aps(
        &mut self,
        contexts: &'static mut [ApContext],
        handoff: Option<MpStartup<'_>>,
    ) -> Result<(), EfiError> {
        let Some(MpStartup::Psci { conduit, processors }) = handoff else {
            log::error!("AArch64 MP Services requires a PSCI processor inventory");
            return Err(EfiError::Unsupported);
        };
        let version = conduit.version().map_err(|error| {
            log::error!("Cannot query PSCI version: {error}");
            EfiError::Unsupported
        })?;
        if version < 2 {
            log::error!("AArch64 MP Services requires PSCI 0.2 or newer; found {version:#x}");
            return Err(EfiError::Unsupported);
        }
        let mut state = bootstrap::CpuState::capture()?;
        state.conduit = u64::from(conduit == Conduit::Hvc);
        self.contexts = Self::prepare_contexts(contexts, processors, self.bsp_processor_id, state)?;
        self.power = Some(PowerControl::Firmware(conduit));
        bootstrap::publish(self.contexts)?;
        self.start_aps(bootstrap::entry_point()?)
    }

    fn ap_count(&self) -> usize {
        self.contexts.len()
    }

    fn started_ap_count(&self) -> usize {
        self.contexts.iter().filter(|context| context.started.load(Ordering::Acquire)).count()
    }

    fn enabled_ap_count(&self) -> usize {
        self.contexts
            .iter()
            .filter(|context| matches!(context.sm.state(), ProcessorState::Ready | ProcessorState::Busy))
            .count()
    }

    fn set_ap_enabled(&self, index: usize, enabled: bool, healthy: Option<bool>) -> bool {
        bootstrap::check_for_failures();
        if self.shutting_down.load(Ordering::Acquire) {
            return false;
        }
        let Some(context) = self.contexts.get(index) else { return false };
        if enabled && !context.sm.is_idle() {
            return false;
        }
        context.sm.set_enabled(enabled);
        if let Some(healthy) = healthy {
            context.sm.set_healthy(healthy);
        }
        true
    }

    fn ap_healthy(&self, index: usize) -> bool {
        self.contexts.get(index).is_some_and(|context| context.sm.is_healthy())
    }

    fn who_am_i(&self) -> Option<Processor> {
        self.find_processor(bootstrap::processor_id())
    }

    fn bsp_processor_id(&self) -> u64 {
        self.bsp_processor_id
    }

    fn ap_processor_id(&self, index: usize) -> Option<u64> {
        self.contexts.get(index).map(|context| context.processor_id)
    }

    fn ap_finished(&self, index: usize, work_id: u64) -> bool {
        bootstrap::check_for_failures();
        self.contexts.get(index).is_some_and(|context| context.sm.is_finished(work_id))
    }

    fn ap_availability(&self, index: usize) -> ProcessorState {
        bootstrap::check_for_failures();
        self.contexts.get(index).map_or(ProcessorState::NotStarted, |context| context.sm.state())
    }

    fn signal_ap(&self, index: usize, work: ApWorkItem) -> Option<u64> {
        bootstrap::check_for_failures();
        if self.shutting_down.load(Ordering::Acquire) {
            return None;
        }
        let work_id = self.contexts.get(index)?.sm.dispatch(work).ok()?;
        bootstrap::wake();
        Some(work_id)
    }

    fn abort_ap(&self, index: usize, work_id: u64) -> bool {
        bootstrap::check_for_failures();
        let Some(context) = self.contexts.get(index) else { return false };
        if context.sm.is_finished(work_id) {
            return true;
        }
        // TODO: Implement forced AArch64 AP termination and recovery. PSCI CPU_OFF
        // only stops its caller; returning here would invalidate caller-owned data
        // while the AP could still access it.
        panic!("MPIDR {:#x} timed out; forced AArch64 AP recovery is not implemented", context.processor_id);
    }

    fn park(&self) {
        bootstrap::check_for_failures();
        self.shutting_down.store(true, Ordering::Release);
        for context in self.contexts {
            context.sm.signal_exit();
        }
        bootstrap::wake();
        let Some(power) = self.power.as_ref() else {
            assert!(self.contexts.is_empty(), "PSCI contexts exist without a conduit");
            return;
        };
        assert!(
            self.wait_until(SHUTDOWN_TIMEOUT_US, || {
                self.contexts.iter().all(|context| {
                    power.affinity_info(context.processor_id).unwrap_or_else(|error| {
                        panic!("Cannot confirm MPIDR {:#x} shutdown: {error}", context.processor_id)
                    }) == AffinityState::Off
                })
            }),
            "PSCI AP shutdown timed out; cannot release boot-services memory"
        );
        log::info!("Powered off {} application processors using PSCI", self.contexts.len());
    }

    fn sync_aps(&self) -> bool {
        bootstrap::check_for_failures();
        if self.shutting_down.load(Ordering::Acquire)
            || self.contexts.iter().any(|context| context.sm.state() == ProcessorState::Busy)
        {
            return false;
        }
        let work = ApWorkItem::new(|()| {}, &());
        for context in self.contexts.iter().filter(|context| context.sm.state() == ProcessorState::Ready) {
            if context.sm.dispatch(work).is_err() {
                bootstrap::wake();
                return false;
            }
        }
        bootstrap::wake();
        while self.contexts.iter().any(|context| context.sm.state() == ProcessorState::Busy) {
            bootstrap::check_for_failures();
            core::hint::spin_loop();
        }
        true
    }
}

#[cfg(all(target_os = "uefi", target_arch = "aarch64"))]
unsafe extern "efiapi" fn ap_main(context: *const ApContext) -> ! {
    // SAFETY: CPU_ON passes this AP's persistent context; assembly has installed
    // its translation regime, exception vectors, FP access, and exclusive stack.
    let context = unsafe { &*context };
    let Some(consumer) = context.sm.start() else {
        bootstrap::fail(2);
    };
    context.started.store(true, Ordering::Release);
    consumer.run_dispatch_loop(bootstrap::flush_tlb, bootstrap::idle);
    // SAFETY: The consumer has acknowledged terminal exit and no longer owns work.
    // CPU_OFF is reached before the BSP releases any boot-services memory.
    unsafe {
        let conduit = if context.boot.conduit == 0 { Conduit::Smc } else { Conduit::Hvc };
        match conduit.cpu_off() {
            Ok(never) => match never {},
            Err(_) => bootstrap::fail(3),
        }
    }
}

#[cfg(test)]
#[cfg_attr(coverage, coverage(off))]
mod tests {
    use super::*;
    use patina::component::service::perf_timer::MockArchTimerFunctionality;
    use std::{boxed::Box, vec};

    fn context() -> ApContext {
        let mut context = ApContext::default();
        // SAFETY: Tests never execute on this synthetic stack.
        unsafe { context.set_stack_top(NonZeroUsize::new(ApContext::STACK_SIZE * 2).unwrap()).unwrap() };
        context
    }

    fn support(contexts: &'static [ApContext]) -> MpSupport {
        MpSupport {
            contexts,
            bsp_processor_id: 0x12_0000_0000,
            power: None,
            timer: Box::leak(Box::new(MockArchTimerFunctionality::new())),
            perf_frequency: NonZeroU64::new(1_000_000).unwrap(),
            shutting_down: AtomicBool::new(false),
        }
    }

    #[test]
    fn test_mp_aarch64_validates_full_affinity_and_exactly_one_bsp() {
        assert_eq!(MpSupport::validate_processors(&[1, 0x12_0000_0000], 0x12_0000_0000), Ok(()));
        for processors in [vec![], vec![1], vec![0, 0], vec![0, 1 << 24], vec![0, 1 << 40]] {
            assert_eq!(MpSupport::validate_processors(&processors, 0), Err(EfiError::InvalidParameter));
        }
    }

    #[test]
    fn test_mp_aarch64_initialization_requires_a_calibrated_timer() {
        use patina::component::service::memory::MockMemoryManager;
        let memory = MockMemoryManager::new();
        let mut timer = MockArchTimerFunctionality::new();
        timer.expect_perf_frequency().once().return_const(0u64);
        assert!(matches!(MpSupport::initialize(&memory, Box::leak(Box::new(timer))), Err(EfiError::Unsupported)));
        let mut timer = MockArchTimerFunctionality::new();
        timer.expect_perf_frequency().once().return_const(1_000_000u64);
        let initialized = MpSupport::initialize(&memory, Box::leak(Box::new(timer))).unwrap();
        assert_eq!(initialized.who_am_i(), Some(Processor::Bsp));
        let mut context = context();
        assert_eq!(
            // SAFETY: This test rejects the misaligned pointer without accessing it.
            unsafe { context.set_stack_top(NonZeroUsize::new(0x10_001).unwrap()) },
            Err(EfiError::InvalidParameter)
        );
    }

    #[test]
    fn test_mp_aarch64_preflight_refuses_live_or_unqueryable_processors() {
        for state in [Ok(AffinityState::On), Ok(AffinityState::OnPending), Err(PsciError::Denied)] {
            let contexts = Box::leak(Box::new([context()]));
            let mut support = support(contexts);
            let mut power = MockPsci::new();
            power.expect_affinity_info().once().return_const(state);
            power.expect_cpu_on().never();
            support.power = Some(PowerControl::Mock(Box::new(power)));
            assert_eq!(support.start_aps(NonZeroU64::new(4).unwrap()), Err(EfiError::DeviceError));
        }
    }

    #[test]
    fn test_mp_aarch64_cpu_on_waits_for_dispatcher_acknowledgement() {
        let contexts: &'static [ApContext] = Box::leak(Box::new([context()]));
        let mut support = support(contexts);
        let mut power = MockPsci::new();
        power.expect_affinity_info().once().return_const(Ok(AffinityState::Off));
        power.expect_cpu_on().once().returning(move |_, entry, pointer| {
            assert_eq!(entry.get(), 4);
            assert_eq!(pointer, core::ptr::from_ref(&contexts[0]) as u64);
            contexts[0].sm.start().unwrap();
            contexts[0].started.store(true, Ordering::Release);
            Ok(())
        });
        support.power = Some(PowerControl::Mock(Box::new(power)));
        let mut timer = MockArchTimerFunctionality::new();
        timer.expect_cpu_count().once().return_const(0u64);
        support.timer = Box::leak(Box::new(timer));
        assert_eq!(support.start_aps(NonZeroU64::new(4).unwrap()), Ok(()));
        assert_eq!(support.started_ap_count(), 1);
    }

    #[test]
    #[should_panic(expected = "PSCI CPU_ON failed")]
    fn test_mp_aarch64_cpu_on_failure_does_not_abandon_started_processors() {
        let contexts = Box::leak(Box::new([context()]));
        let mut support = support(contexts);
        let mut power = MockPsci::new();
        power.expect_affinity_info().once().return_const(Ok(AffinityState::Off));
        power.expect_cpu_on().once().return_const(Err(PsciError::Denied));
        support.power = Some(PowerControl::Mock(Box::new(power)));
        let _ = support.start_aps(NonZeroU64::new(4).unwrap());
    }

    #[test]
    #[should_panic(expected = "PSCI AP startup timed out")]
    fn test_mp_aarch64_cpu_on_success_without_acknowledgement_fails_stop() {
        let contexts = Box::leak(Box::new([context()]));
        let mut support = support(contexts);
        let mut power = MockPsci::new();
        power.expect_affinity_info().once().return_const(Ok(AffinityState::Off));
        power.expect_cpu_on().once().return_const(Ok(()));
        support.power = Some(PowerControl::Mock(Box::new(power)));
        let mut timer = MockArchTimerFunctionality::new();
        let mut sequence = mockall::Sequence::new();
        timer.expect_cpu_count().once().in_sequence(&mut sequence).return_const(0u64);
        timer.expect_cpu_count().once().in_sequence(&mut sequence).return_const(STARTUP_TIMEOUT_US);
        support.timer = Box::leak(Box::new(timer));
        let _ = support.start_aps(NonZeroU64::new(4).unwrap());
    }

    #[test]
    fn test_mp_aarch64_shutdown_confirms_disabled_processors_are_off() {
        let contexts = Box::leak(Box::new([context()]));
        let mut support = support(contexts);
        let _consumer = contexts[0].sm.start().unwrap();
        contexts[0].sm.set_enabled(false);
        let mut power = MockPsci::new();
        power.expect_affinity_info().once().return_const(Ok(AffinityState::Off));
        support.power = Some(PowerControl::Mock(Box::new(power)));
        let mut timer = MockArchTimerFunctionality::new();
        timer.expect_cpu_count().once().return_const(0u64);
        support.timer = Box::leak(Box::new(timer));
        support.park();
        assert_eq!(support.ap_availability(0), ProcessorState::Disabled);
        assert!(support.signal_ap(0, ApWorkItem::new(|()| {}, &())).is_none());
    }

    #[test]
    fn test_mp_aarch64_synchronization_waits_for_actual_work_completion() {
        let contexts = Box::leak(Box::new([context()]));
        let support = support(contexts);
        let consumer = contexts[0].sm.start().unwrap();
        let worker = std::thread::spawn(move || consumer.run_dispatch_loop(|| {}, |_| std::thread::yield_now()));
        assert!(support.sync_aps());
        assert!(support.ap_finished(0, 1));
        assert!(support.abort_ap(0, 1));
        contexts[0].sm.signal_exit();
        worker.join().unwrap();
    }

    #[test]
    fn test_mp_aarch64_prepares_contexts_without_assuming_bsp_is_first() {
        let mut contexts = [context(), context()];
        let state = bootstrap::CpuState { tcr: 42, ..Default::default() };
        let prepared =
            MpSupport::prepare_contexts(&mut contexts, &[2, 0x12_0000_0000, 1], 0x12_0000_0000, state).unwrap();
        assert_eq!(prepared[0].processor_id, 2);
        assert_eq!(prepared[1].processor_id, 1);
        assert_eq!(prepared[0].boot.tcr, 42);
        assert_eq!(prepared[0].boot.stack_top, 0x10_000);
        assert!(MpSupport::prepare_contexts(&mut [], &[0, 1], 0, state).is_err());
        assert!(MpSupport::prepare_contexts(&mut [ApContext::default()], &[0, 1], 0, state).is_err());
    }

    #[test]
    fn test_mp_aarch64_tracks_identity_dispatch_and_enablement() {
        let mut context = context();
        context.processor_id = 0x34_0000_0001;
        let contexts = Box::leak(Box::new([context]));
        let support = support(contexts);
        let _consumer = contexts[0].sm.start().unwrap();
        contexts[0].started.store(true, Ordering::Release);
        assert_eq!(support.find_processor(0x12_0000_0000), Some(Processor::Bsp));
        assert_eq!(support.find_processor(0x34_0000_0001), Some(Processor::Ap(0)));
        assert_eq!(support.find_processor(7), None);
        assert_eq!(support.ap_processor_id(0), Some(0x34_0000_0001));
        assert_eq!(support.ap_processor_id(1), None);
        assert_eq!(support.ap_count(), 1);
        assert_eq!(support.started_ap_count(), 1);
        assert_eq!(support.enabled_ap_count(), 1);
        assert!(support.set_ap_enabled(0, false, Some(false)));
        assert_eq!(support.enabled_ap_count(), 0);
        assert!(!support.ap_healthy(0));
        assert!(support.signal_ap(0, ApWorkItem::new(|()| {}, &())).is_none());
        assert!(support.set_ap_enabled(0, true, Some(true)));
        let work_id = support.signal_ap(0, ApWorkItem::new(|()| {}, &())).unwrap();
        assert!(!support.ap_finished(0, work_id));
        assert_eq!(support.ap_availability(0), ProcessorState::Busy);
        assert!(!support.set_ap_enabled(0, true, None));
        assert!(!support.set_ap_enabled(1, false, None));
        assert!(!support.sync_aps());
    }

    #[test]
    #[should_panic(expected = "forced AArch64 AP recovery is not implemented")]
    fn test_mp_aarch64_unfinished_abort_fails_stop() {
        let contexts = Box::leak(Box::new([context()]));
        support(contexts).abort_ap(0, 1);
    }

    #[test]
    fn test_mp_aarch64_terminal_shutdown_rejects_work() {
        let support = support(&[]);
        support.park();
        assert!(support.signal_ap(0, ApWorkItem::new(|()| {}, &())).is_none());
        assert!(!support.set_ap_enabled(0, true, None));
        assert!(!support.sync_aps());
        assert!(!support.abort_ap(0, 1));
    }
}
