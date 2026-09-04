//! CPU state adopted by APs before entering the Rust dispatch loop.
//!
//! ## License
//!
//! Copyright (c) Microsoft Corporation.
//!
//! SPDX-License-Identifier: Apache-2.0
//!

use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use core::{arch::global_asm, cell::UnsafeCell, ffi::c_void};

use patina::arch::x64::{read_msr, write_msr};
use patina::standard::efi::protocols::mp_services::ApProcedure;
use patina_mtrr::{Mtrr, create_mtrr_lib, structs::MtrrSettings};

use super::ApWorkItem;
use crate::gdt::{CODE_SELECTOR, DescriptorTablePointer};

const IA32_EFER: u32 = 0xC000_0080;

global_asm!(include_str!("ap_exception.asm"));

unsafe extern "C" {
    fn ap_exception_halt();
}

static AP_IDT: spin::LazyLock<HaltIdt> = spin::LazyLock::new(|| HaltIdt([IdtEntry::halt(); 256]));

#[derive(Clone, Copy)]
#[repr(C)]
struct IdtEntry {
    offset_low: u16,
    selector: u16,
    ist: u8,
    type_attr: u8,
    offset_mid: u16,
    offset_high: u32,
    reserved: u32,
}

impl IdtEntry {
    fn halt() -> Self {
        let address = ap_exception_halt as *const () as u64;
        Self {
            offset_low: address as u16,
            selector: CODE_SELECTOR,
            ist: 0,
            type_attr: 0x8E,
            offset_mid: (address >> 16) as u16,
            offset_high: (address >> 32) as u32,
            reserved: 0,
        }
    }
}

#[repr(C, align(16))]
struct HaltIdt([IdtEntry; 256]);

/// Processor-local architectural state that must remain valid for the lifetime
/// of an AP.
pub(super) struct ApCpuState {
    valid: AtomicBool,
    cr0: AtomicU64,
    cr3: AtomicU64,
    cr4: AtomicU64,
    efer: AtomicU64,
    mtrrs: UnsafeCell<Option<MtrrSettings>>,
}

// SAFETY: Startup registers are published through `valid`. The BSP writes the
// MTRR slot only while its AP is idle, then publishes work that reads it. After
// publication only the owning AP reads the slot.
unsafe impl Sync for ApCpuState {}

impl ApCpuState {
    pub(super) const fn new() -> Self {
        Self {
            valid: AtomicBool::new(false),
            cr0: AtomicU64::new(0),
            cr3: AtomicU64::new(0),
            cr4: AtomicU64::new(0),
            efer: AtomicU64::new(0),
            mtrrs: UnsafeCell::new(None),
        }
    }

    /// Stores the BSP startup state for this AP and publishes it for [`Self::apply`].
    pub(super) fn prepare(&self, state: CpuStartupState) {
        self.cr0.store(state.cr0, Ordering::Relaxed);
        self.cr3.store(state.cr3, Ordering::Relaxed);
        self.cr4.store(state.cr4, Ordering::Relaxed);
        self.efer.store(state.efer, Ordering::Relaxed);
        self.valid.store(true, Ordering::Release);
    }

    /// Captures the BSP's MTRRs in this AP's persistent slot and returns work
    /// that applies them on the AP.
    ///
    /// The caller must ensure the AP is idle and hold the dispatch lock until
    /// the returned work is published.
    pub(super) fn prepare_mtrr_sync(&self) -> Option<ApWorkItem> {
        let mtrr = create_mtrr_lib(0);
        if !mtrr.is_supported() {
            return None;
        }
        let settings = mtrr
            .get_all_mtrrs()
            .inspect_err(|e| log::error!("Failed to read BSP MTRRs for AP synchronization: {e:?}"))
            .ok()?;

        // SAFETY: The caller guarantees this AP is idle and holds the dispatch
        // lock, so no AP can be reading the slot while it is replaced.
        let slot = unsafe { &mut *self.mtrrs.get() };
        *slot = Some(settings);
        let argument = slot.as_mut().map(|settings| core::ptr::from_mut(settings).cast::<c_void>())?;
        // SAFETY: `apply_mtrrs` reads this processor's persistent snapshot slot.
        // The caller keeps the AP idle while the slot is written and publishes
        // this work before releasing the dispatch lock.
        Some(unsafe { ApWorkItem::new_efi(apply_mtrrs as ApProcedure, argument) })
    }

    /// Applies the prepared startup state to the calling AP.
    ///
    /// Returns `false` if this AP's state was never prepared.
    pub(super) fn apply(&self) -> bool {
        if !self.valid.load(Ordering::Acquire) {
            return false;
        }

        let idtr = DescriptorTablePointer {
            limit: (core::mem::size_of::<HaltIdt>() - 1) as u16,
            base: AP_IDT.0.as_ptr() as u64,
        };
        // SAFETY: The IDT is valid, and the pointer to it is correctly formed.
        unsafe {
            core::arch::asm!("lidt [{}]", in(reg) &idtr, options(nostack, preserves_flags));
        }

        super::apic::mask_local_interrupts();

        let cr0 = self.cr0.load(Ordering::Relaxed);
        let cr3 = self.cr3.load(Ordering::Relaxed);
        let cr4 = self.cr4.load(Ordering::Relaxed);
        let efer = self.efer.load(Ordering::Relaxed);
        // SAFETY: These are the BSP's own live values. EFER must be applied before CR3
        // because the BSP page table can contain NX entries that require EFER.NXE.
        unsafe {
            write_msr(IA32_EFER, efer);
            core::arch::asm!(
                "mov cr0, {0}",
                "mov cr3, {1}",
                "mov cr4, {2}",
                in(reg) cr0,
                in(reg) cr3,
                in(reg) cr4,
                options(nostack, preserves_flags),
            );
        }

        // SAFETY: FNINIT resets the x87 unit to a known state and has no operands.
        unsafe {
            core::arch::asm!("fninit", options(nomem, nostack, preserves_flags));
        }

        true
    }
}

/// BSP control-register values copied into each AP's startup state.
#[derive(Clone, Copy)]
pub(super) struct CpuStartupState {
    cr0: u64,
    cr3: u64,
    cr4: u64,
    efer: u64,
}

/// Captures the BSP's startup state. Must be called before any AP is woken.
pub(super) fn capture_bsp_state() -> CpuStartupState {
    let cr0: u64;
    let cr3: u64;
    let cr4: u64;
    // SAFETY: Reading control registers and IA32_EFER has no side effects.
    unsafe {
        core::arch::asm!(
            "mov {0}, cr0",
            "mov {1}, cr3",
            "mov {2}, cr4",
            out(reg) cr0,
            out(reg) cr3,
            out(reg) cr4,
            options(nomem, nostack, preserves_flags),
        );
    }
    let efer = unsafe { read_msr(IA32_EFER) };

    spin::LazyLock::force(&AP_IDT);
    CpuStartupState { cr0, cr3, cr4, efer }
}

/// AP procedure that programs the calling processor's MTRRs from its persistent
/// snapshot slot.
extern "efiapi" fn apply_mtrrs(argument: *mut c_void) {
    // SAFETY: `prepare_mtrr_sync` passes a pointer to the owning AP's initialized
    // snapshot, which remains valid for the lifetime of the AP context.
    let settings = unsafe { &*argument.cast::<MtrrSettings>() };
    let mut mtrr = create_mtrr_lib(0);
    if mtrr.is_supported() {
        mtrr.set_all_mtrrs(settings);
    }
}
