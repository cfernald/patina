//! CPU state adopted by APs before entering the Rust dispatch loop.
//!
//! ## License
//!
//! Copyright (c) Microsoft Corporation.
//!
//! SPDX-License-Identifier: Apache-2.0
//!

use core::cell::UnsafeCell;

use patina_mtrr::{Mtrr, create_mtrr_lib, structs::MtrrSettings};

use super::ApWorkItem;

/// Processor-local architectural state that must remain valid for the lifetime
/// of an AP.
pub(super) struct ApCpuState {
    mtrrs: UnsafeCell<Option<MtrrSettings>>,
}

// SAFETY: The BSP writes the MTRR slot only before the AP's initial handoff,
// while it is idle under the dispatch lock, or while it is in wait-for-SIPI.
// After publication only the owning AP reads the slot.
unsafe impl Sync for ApCpuState {}

impl ApCpuState {
    pub(super) const fn new() -> Self {
        Self { mtrrs: UnsafeCell::new(None) }
    }

    fn capture_mtrrs(&self) -> Result<bool, ()> {
        let mtrr = create_mtrr_lib(0);
        if !mtrr.is_supported() {
            return Ok(false);
        }
        let settings = mtrr
            .get_all_mtrrs()
            .inspect_err(|e| log::error!("Failed to read BSP MTRRs for AP synchronization: {e:?}"))
            .map_err(|_| ())?;

        // SAFETY: The caller guarantees this AP cannot be reading the slot while
        // it is replaced, either through the dispatch lock or architectural reset.
        let slot = unsafe { &mut *self.mtrrs.get() };
        *slot = Some(settings);
        Ok(true)
    }

    /// Captures the BSP's MTRRs in this AP's persistent slot and returns work
    /// that applies them on the AP.
    ///
    /// The caller must ensure the AP is idle and hold the dispatch lock until
    /// the returned work is published.
    pub(super) fn prepare_mtrr_sync(&'static self) -> Option<ApWorkItem> {
        self.capture_mtrrs().ok()?.then_some(ApWorkItem::new(apply_mtrrs, self))
    }

    /// Captures the BSP MTRRs before the owning AP enters the dispatch loop.
    pub(super) fn prepare_entry(&self) -> bool {
        self.capture_mtrrs().is_ok()
    }

    /// Applies the snapshot prepared by the BSP before this AP was started.
    pub(super) fn apply(&self) {
        apply_mtrrs(self);
    }
}

/// AP procedure that programs the calling processor's MTRRs from its persistent
/// snapshot slot.
fn apply_mtrrs(state: &ApCpuState) {
    let mut mtrr = create_mtrr_lib(0);
    if !mtrr.is_supported() {
        return;
    }

    // SAFETY: The BSP initializes this AP's snapshot while the AP cannot read it,
    // then publishes dispatch work, its initial entry point, or a STARTUP IPI.
    let Some(settings) = (unsafe { &*state.mtrrs.get() }).as_ref() else {
        log::error!("AP MTRR synchronization ran without a prepared snapshot");
        return;
    };
    mtrr.set_all_mtrrs(settings);
}
