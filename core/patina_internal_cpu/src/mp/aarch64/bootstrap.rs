//! PSCI secondary-entry environment and terminal AP failure reporting.
//!
//! ## License
//!
//! Copyright (c) Microsoft Corporation.
//!
//! SPDX-License-Identifier: Apache-2.0
//!

#![cfg_attr(coverage, coverage(off))]

use core::{
    num::NonZeroU64,
    sync::atomic::{AtomicU64, Ordering},
};

use patina::error::EfiError;

use super::ApContext;

/// Register state consumed before entering Rust on an AP.
#[derive(Clone, Copy, Default)]
#[repr(C)]
pub(super) struct CpuState {
    pub stack_top: u64,
    pub exception_level: u64,
    pub ttbr0: u64,
    pub ttbr1: u64,
    pub tcr: u64,
    pub mair: u64,
    pub sctlr: u64,
    pub hcr: u64,
    pub cpacr: u64,
    pub cptr: u64,
    pub conduit: u64,
}

#[repr(C)]
struct Failure {
    state: AtomicU64,
    reason: AtomicU64,
    mpidr: AtomicU64,
    esr: AtomicU64,
    far: AtomicU64,
}

static FAILURE: Failure = Failure {
    state: AtomicU64::new(0),
    reason: AtomicU64::new(0),
    mpidr: AtomicU64::new(0),
    esr: AtomicU64::new(0),
    far: AtomicU64::new(0),
};

#[cfg(all(target_os = "uefi", target_arch = "aarch64"))]
mod hardware {
    use super::{CpuState, FAILURE};
    use crate::mp::aarch64::ap_main;

    core::arch::global_asm!(
        include_str!("entry.asm"),
        stack = const core::mem::offset_of!(CpuState, stack_top),
        el = const core::mem::offset_of!(CpuState, exception_level),
        ttbr0 = const core::mem::offset_of!(CpuState, ttbr0),
        ttbr1 = const core::mem::offset_of!(CpuState, ttbr1),
        tcr = const core::mem::offset_of!(CpuState, tcr),
        mair = const core::mem::offset_of!(CpuState, mair),
        sctlr = const core::mem::offset_of!(CpuState, sctlr),
        hcr = const core::mem::offset_of!(CpuState, hcr),
        cpacr = const core::mem::offset_of!(CpuState, cpacr),
        cptr = const core::mem::offset_of!(CpuState, cptr),
        failure = sym FAILURE,
        main = sym ap_main,
    );

    unsafe extern "efiapi" {
        pub(super) fn patina_arm_ap_start();
        pub(super) fn patina_arm_ap_fail(reason: u64) -> !;
        pub(super) static patina_arm_ap_end: u8;
    }
}

impl CpuState {
    pub(super) fn capture() -> Result<Self, EfiError> {
        #[cfg(all(target_os = "uefi", target_arch = "aarch64"))]
        {
            use patina::read_sysreg;
            let exception_level = read_sysreg!(CurrentEL);
            let mut state = Self { exception_level, ..Self::default() };
            match exception_level {
                4 => {
                    state.ttbr0 = read_sysreg!(TTBR0_EL1);
                    state.ttbr1 = read_sysreg!(TTBR1_EL1);
                    state.tcr = read_sysreg!(TCR_EL1);
                    state.mair = read_sysreg!(MAIR_EL1);
                    state.sctlr = read_sysreg!(SCTLR_EL1);
                }
                8 => {
                    state.hcr = read_sysreg!(HCR_EL2);
                    if state.hcr & (1 << 34) != 0 {
                        log::error!("AArch64 MP bootstrap does not support an EL2 host using VHE");
                        return Err(EfiError::Unsupported);
                    }
                    state.ttbr0 = read_sysreg!(TTBR0_EL2);
                    state.tcr = read_sysreg!(TCR_EL2);
                    state.mair = read_sysreg!(MAIR_EL2);
                    state.sctlr = read_sysreg!(SCTLR_EL2);
                    state.cptr = read_sysreg!(CPTR_EL2) & !(1 << 10);
                }
                _ => {
                    log::error!("Unsupported BSP exception level {exception_level:#x}");
                    return Err(EfiError::Unsupported);
                }
            }
            state.cpacr = read_sysreg!(CPACR_EL1) | (3 << 20);
            if state.sctlr & 1 == 0 {
                log::error!("AArch64 MP Services requires active BSP page tables");
                return Err(EfiError::Unsupported);
            }
            Ok(state)
        }
        #[cfg(not(all(target_os = "uefi", target_arch = "aarch64")))]
        {
            Err(EfiError::Unsupported)
        }
    }
}

pub(super) fn processor_id() -> u64 {
    #[cfg(all(target_os = "uefi", target_arch = "aarch64"))]
    {
        patina::read_sysreg!(MPIDR_EL1) & patina::arch::psci::MPIDR_AFFINITY_MASK
    }
    #[cfg(not(all(target_os = "uefi", target_arch = "aarch64")))]
    {
        0
    }
}

pub(super) fn entry_point() -> Result<NonZeroU64, EfiError> {
    #[cfg(all(target_os = "uefi", target_arch = "aarch64"))]
    {
        NonZeroU64::new(hardware::patina_arm_ap_start as *const () as u64).ok_or(EfiError::InvalidParameter)
    }
    #[cfg(not(all(target_os = "uefi", target_arch = "aarch64")))]
    {
        Err(EfiError::Unsupported)
    }
}

#[cfg(all(target_os = "uefi", target_arch = "aarch64"))]
fn identity_mapped(address: usize) -> bool {
    let result: u64;
    // SAFETY: These instructions query, but do not modify, the BSP's active
    // translation regime. They execute only at a supported privileged EL.
    unsafe {
        if patina::read_sysreg!(CurrentEL) == 8 {
            core::arch::asm!("at s1e2r, {address}", "isb", "mrs {result}, par_el1",
                address = in(reg) address, result = out(reg) result, options(nostack));
        } else {
            core::arch::asm!("at s1e1r, {address}", "isb", "mrs {result}, par_el1",
                address = in(reg) address, result = out(reg) result, options(nostack));
        }
    }
    result & 1 == 0 && ((result & 0x000F_FFFF_FFFF_F000) | (address as u64 & 0xFFF)) == address as u64
}

#[cfg(all(target_os = "uefi", target_arch = "aarch64"))]
fn publish_range(address: usize, length: usize) -> Result<(), EfiError> {
    let end = address.checked_add(length).ok_or(EfiError::InvalidParameter)?;
    let mut page = address & !0xFFF;
    while page < end {
        if !identity_mapped(page) {
            log::error!("PSCI bootstrap memory is not identity mapped at {page:#x}");
            return Err(EfiError::Unsupported);
        }
        page = page.checked_add(0x1000).ok_or(EfiError::InvalidParameter)?;
    }
    patina::arch::flush_data_cache(
        address as u64,
        length as u64,
        patina::pi::protocol::cpu_arch::CpuFlushType::EfiCpuFlushTypeWriteBack,
    )
}

pub(super) fn publish(contexts: &[ApContext]) -> Result<(), EfiError> {
    #[cfg(all(target_os = "uefi", target_arch = "aarch64"))]
    {
        let start = hardware::patina_arm_ap_start as *const () as usize;
        let end = core::ptr::addr_of!(hardware::patina_arm_ap_end) as usize;
        publish_range(start, end.checked_sub(start).ok_or(EfiError::InvalidParameter)?)?;
        publish_range(core::ptr::from_ref(&FAILURE) as usize, core::mem::size_of::<Failure>())?;
        publish_range(contexts.as_ptr() as usize, core::mem::size_of_val(contexts))?;
        Ok(())
    }
    #[cfg(not(all(target_os = "uefi", target_arch = "aarch64")))]
    {
        let _ = contexts;
        Err(EfiError::Unsupported)
    }
}

#[cfg(all(target_os = "uefi", target_arch = "aarch64"))]
pub(super) fn flush_tlb() {
    // SAFETY: Each AP shares the BSP page tables. It invalidates its own cached
    // translations before accessing newly published work or acknowledging sync.
    unsafe {
        if patina::read_sysreg!(CurrentEL) == 8 {
            core::arch::asm!("dsb ish", "tlbi alle2", "dsb ish", "isb", options(nostack));
        } else {
            core::arch::asm!("dsb ish", "tlbi vmalle1", "dsb ish", "isb", options(nostack));
        }
    }
}

pub(super) fn wake() {
    #[cfg(all(target_os = "uefi", target_arch = "aarch64"))]
    // SAFETY: Completing work/exit publication before SEV ensures an AP that
    // wakes from WFE can observe the updated lifecycle state.
    unsafe {
        core::arch::asm!("dsb ishst", "sev", options(nostack, preserves_flags));
    }
}

#[cfg(all(target_os = "uefi", target_arch = "aarch64"))]
pub(super) fn idle(state: &core::sync::atomic::AtomicU8) {
    // SAFETY: The first WFE consumes a locally primed event. Rechecking the
    // acquire-loaded state before the second WFE prevents a lost wake-up; work
    // published after the check sends an event that releases the second WFE.
    unsafe {
        core::arch::asm!("sevl", "wfe", options(nostack, preserves_flags));
        if crate::mp::control::should_wait(state) {
            core::arch::asm!("wfe", options(nostack, preserves_flags));
        }
    }
}

pub(super) fn check_for_failures() {
    assert_ne!(
        FAILURE.state.load(Ordering::Acquire),
        2,
        "AArch64 AP failure: reason {}, MPIDR {:#x}, ESR {:#x}, FAR {:#x}",
        FAILURE.reason.load(Ordering::Relaxed),
        FAILURE.mpidr.load(Ordering::Relaxed),
        FAILURE.esr.load(Ordering::Relaxed),
        FAILURE.far.load(Ordering::Relaxed),
    );
}

#[cfg(all(target_os = "uefi", target_arch = "aarch64"))]
pub(super) fn fail(reason: u64) -> ! {
    // SAFETY: The bootstrap installs a stack-independent terminal failure
    // handler. It publishes the first error and never resumes AP execution.
    unsafe { hardware::patina_arm_ap_fail(reason) }
}
