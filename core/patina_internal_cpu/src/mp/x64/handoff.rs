//! PEI-to-DXE AP handoff bridge.
//!
//! ## License
//!
//! Copyright (c) Microsoft Corporation.
//!
//! SPDX-License-Identifier: Apache-2.0
//!

use core::arch::global_asm;
use core::cell::UnsafeCell;
use core::sync::atomic::{AtomicU32, AtomicU64, Ordering};

use super::ApContext;
use crate::gdt::{CODE_SELECTOR, DATA_SELECTOR, DescriptorTablePointer};

/// Pointer to the core-allocated [`ApContext`] array (APs only, excluding the BSP).
#[unsafe(no_mangle)]
static AP_CONTEXTS: AtomicU64 = AtomicU64::new(0);

/// Number of entries in the [`ApContext`] array pointed to by [`AP_CONTEXTS`].
#[unsafe(no_mangle)]
static AP_CONTEXT_COUNT: AtomicU32 = AtomicU32::new(0);

/// The BSP's GDTR, in the fixed 10-byte layout the AP assembly loads with `lgdt`.
#[unsafe(no_mangle)]
static AP_BSP_GDTR: GdtrHolder = GdtrHolder(UnsafeCell::new(DescriptorTablePointer { limit: 0, base: 0 }));

struct GdtrHolder(UnsafeCell<DescriptorTablePointer>);
// SAFETY: Written once by BSP before APs are woken. APs only read after signaling.
unsafe impl Sync for GdtrHolder {}

const AP_CONTEXT_SIZE: usize = core::mem::size_of::<ApContext>();
const AP_CONTEXT_STACK_OFFSET: usize = core::mem::offset_of!(ApContext, stack_top);
const AP_CONTEXT_APIC_ID_OFFSET: usize = core::mem::offset_of!(ApContext, apic_id);

global_asm!(
    include_str!("ap_entry.asm"),
    ap_context_size = const AP_CONTEXT_SIZE,
    ap_ctx_stack_off = const AP_CONTEXT_STACK_OFFSET,
    ap_ctx_apic_off = const AP_CONTEXT_APIC_ID_OFFSET,
    bsp_code64_sel = const CODE_SELECTOR,
    bsp_data64_sel = const DATA_SELECTOR,
    ap_entry = sym ap_entry,
);

unsafe extern "C" {
    fn ap_entry_64();
}

pub(super) fn publish_bsp_gdtr() {
    // SAFETY: Written once by the BSP before any AP is woken. APs only read it
    // after observing their startup signal.
    unsafe { AP_BSP_GDTR.0.get().write(crate::gdt::descriptor()) };
}

fn ap_entry_addr() -> usize {
    ap_entry_64 as *const () as usize
}

pub(super) fn publish_ap_contexts(aps: &'static [ApContext]) {
    AP_CONTEXTS.store(aps.as_ptr() as u64, Ordering::Release);
    AP_CONTEXT_COUNT.store(aps.len() as u32, Ordering::Release);
}

/// Wakes the AP who is monitoring `signal_address` by writing the
/// AP entry point into `procedure_address` and then raising the signal.
///
/// # Safety
///
/// `procedure_address` and `signal_address` must be the handoff slots
/// for an AP parked in a wait loop.
pub(super) unsafe fn wake_ap(procedure_address: u64, signal_address: u64, signal_value: u32) {
    let entry = ap_entry_addr() as u64;
    // SAFETY: the caller guarantees these are the handoff slots are correctly set
    // up for the AP.
    unsafe {
        core::ptr::write_volatile(procedure_address as *mut u64, entry);
        core::sync::atomic::fence(Ordering::Release);
        core::ptr::write_volatile(signal_address as *mut u32, signal_value);
    }
}

/// AP entry point. Called by each AP after it has configured its stack and
/// switched to the BSP's GDT.
extern "efiapi" fn ap_entry(context: *const ApContext) {
    // SAFETY: `ap_entry_64` selected this pointer from the published context array,
    // which remains allocated for the lifetime of the MP subsystem.
    let Some(context) = (unsafe { context.as_ref() }) else {
        super::MpSupport::park_forever();
    };
    super::MpSupport::ap_run_dispatch_loop(context);
}
