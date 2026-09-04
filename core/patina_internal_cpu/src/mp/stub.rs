//! Stub multiprocessor support for architectures without a full implementation.
//!
//! This provides a [`MpDispatcher`] so that crates depending on MP support compile
//! and link on architectures where the AP startup mechanism has not yet been
//! implemented (e.g. aarch64). Its constructor returns [`EfiError::Unsupported`],
//! so an instance is never created and the remaining methods are unreachable.
//!
//! ## License
//!
//! Copyright (c) Microsoft Corporation.
//!
//! SPDX-License-Identifier: Apache-2.0
//!

use patina::{
    component::service::{memory::MemoryManager, perf_timer::ArchTimerFunctionality},
    error::EfiError,
};

use super::{ApWorkItem, MpDispatcher, MpHandOffInfo, ProcessorState};

/// An MP manager placeholder for architectures without AP startup support. It's construction
/// will always fail so its functions are not reachable. It is a empty enum so it can't be instantiated.
pub enum MpSupport {}

impl MpDispatcher for MpSupport {
    fn initialize(
        _mm: &dyn MemoryManager,
        _timer: &'static dyn ArchTimerFunctionality,
        _handoff: Option<MpHandOffInfo<'_>>,
    ) -> Result<Self, EfiError> {
        Err(EfiError::Unsupported)
    }

    fn processor_count(&self) -> usize {
        unreachable!();
    }

    fn started_processor_count(&self) -> usize {
        unreachable!()
    }

    fn enabled_processor_count(&self) -> usize {
        unreachable!()
    }

    fn set_ap_enabled(&self, _index: usize, _enabled: bool, _healthy: Option<bool>) -> bool {
        unreachable!()
    }

    fn ap_healthy(&self, _index: usize) -> bool {
        unreachable!()
    }

    fn who_am_i(&self) -> Option<usize> {
        unreachable!();
    }

    fn processor_id(&self, _index: usize) -> Option<u32> {
        unreachable!();
    }

    fn ap_finished(&self, _index: usize, _work_id: u64) -> bool {
        unreachable!();
    }

    fn ap_availability(&self, _index: usize) -> ProcessorState {
        unreachable!();
    }

    fn signal_ap(&self, _index: usize, _work: ApWorkItem) -> Option<u64> {
        unreachable!();
    }

    fn wait_ap(&self, _index: usize, _work_id: u64, _timeout_us: usize) -> bool {
        unreachable!();
    }

    fn begin_shutdown(&self) {
        unreachable!();
    }

    fn park(&self) {
        unreachable!();
    }

    fn sync_ap(&self, _index: usize) -> Option<u64> {
        unreachable!()
    }
}
