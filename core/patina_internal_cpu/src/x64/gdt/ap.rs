//! Per-AP GDT construction and maintenance
//!
//! ## License
//!
//! Copyright (c) Microsoft Corporation.
//!
//! SPDX-License-Identifier: Apache-2.0
//!
use core::{
    cell::UnsafeCell,
    sync::atomic::{Ordering, fence},
};

use super::{DescriptorTablePointer, LONG_MODE_GDT_ENTRY_COUNT, minimal_long_mode_entries, tss_descriptor};

const TSS_DESCRIPTOR_ENTRY_COUNT: usize = 2;
const AP_GDT_ENTRY_COUNT: usize = LONG_MODE_GDT_ENTRY_COUNT + TSS_DESCRIPTOR_ENTRY_COUNT;

/// Per-AP GDT containing the common long-mode entries and one TSS descriptor.
#[repr(C, align(8))]
pub(crate) struct ApGdt {
    entries: UnsafeCell<[u64; AP_GDT_ENTRY_COUNT]>,
}

// SAFETY: Initialization happens before publication. The BSP may later restore
// only the TSS descriptor immediately before resetting its owning AP.
unsafe impl Sync for ApGdt {}

impl ApGdt {
    pub(crate) const fn new() -> Self {
        Self { entries: UnsafeCell::new([0; AP_GDT_ENTRY_COUNT]) }
    }

    pub(crate) fn initialize(&mut self, tss_address: u64) {
        let [entry0, entry1, entry2, entry3, entry4, entry5, entry6, entry7] = minimal_long_mode_entries();
        let (tss_low, tss_high) = tss_descriptor(tss_address);
        *self.entries.get_mut() = [entry0, entry1, entry2, entry3, entry4, entry5, entry6, entry7, tss_low, tss_high];
    }

    pub(crate) fn descriptor(&self) -> DescriptorTablePointer {
        DescriptorTablePointer {
            limit: (core::mem::size_of::<Self>() - 1) as u16,
            base: self.entries.get().cast::<u64>() as u64,
        }
    }

    /// Restores the TSS descriptor type changed to busy by `LTR`.
    ///
    /// # Safety
    ///
    /// The owning processor must not reload its task register until after the
    /// caller has completed this method and sent it through architectural reset.
    pub(crate) unsafe fn reset_tss_descriptor(&self, tss_address: u64) {
        let (tss_low, tss_high) = tss_descriptor(tss_address);
        let entries = self.entries.get().cast::<u64>();
        // SAFETY: `entries` points to this AP GDT's two TSS descriptor slots. The
        // caller guarantees its AP cannot concurrently reload the descriptor.
        unsafe {
            entries.add(LONG_MODE_GDT_ENTRY_COUNT).write(tss_low);
            entries.add(LONG_MODE_GDT_ENTRY_COUNT + 1).write(tss_high);
        }
        fence(Ordering::Release);
    }

    #[cfg(test)]
    fn entries(&self) -> &[u64; AP_GDT_ENTRY_COUNT] {
        // SAFETY: Unit tests do not mutate the GDT while inspecting it.
        unsafe { &*self.entries.get() }
    }
}

#[cfg(test)]
#[cfg_attr(coverage, coverage(off))]
mod tests {
    use super::super::TSS_SELECTOR;
    use super::*;

    fn descriptor_base(low: u64, high: u64) -> u64 {
        ((low >> 16) & 0xFFFF) | (((low >> 32) & 0xFF) << 16) | (((low >> 56) & 0xFF) << 24) | (high << 32)
    }

    #[test]
    fn test_ap_gdt_has_one_tss_descriptor() {
        let tss_address = 0x1234_5678_9ABC_DEF0;
        let mut gdt = ApGdt::new();
        gdt.initialize(tss_address);
        let descriptor = gdt.descriptor();
        let limit = descriptor.limit;
        let entries = gdt.entries();

        assert_eq!(limit, 79);
        assert_eq!(&entries[..LONG_MODE_GDT_ENTRY_COUNT], &minimal_long_mode_entries());
        assert_eq!(descriptor_base(entries[8], entries[9]), tss_address);
        assert_eq!((entries[8] >> 40) & 0xF, 0x9);
        assert_eq!(TSS_SELECTOR, 64);
    }

    #[test]
    fn test_ap_gdt_reset_restores_available_tss_without_changing_other_entries() {
        let address = 0x1234_5678_9ABC_DEF0;
        let mut gdt = ApGdt::new();
        gdt.initialize(address);
        let expected = *gdt.entries();
        gdt.entries.get_mut()[8] |= 2 << 40;
        // SAFETY: No processor can load this test-owned GDT.
        unsafe { gdt.reset_tss_descriptor(address) };
        assert_eq!(*gdt.entries(), expected);
    }
}
