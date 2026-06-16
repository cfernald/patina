//! x64-specific architectural helpers for Patina.
//!
//! ## License
//!
//! Copyright (c) Microsoft Corporation.
//!
//! SPDX-License-Identifier: Apache-2.0
//!

use core::arch::asm;

pub(crate) struct X64;

impl super::ArchSupport for X64 {}

/// Writes a byte to an x64 I/O port.
///
/// # Safety
///
/// The caller must ensure `port` is valid for byte writes on this platform and
/// that the side effects are safe in the current execution context.
pub unsafe fn io_out8(port: u16, value: u8) {
    // SAFETY: Guaranteed by caller.
    unsafe {
        core::arch::asm!(
            "out dx, al",
            in("dx") port,
            in("al") value,
            options(nostack, nomem, preserves_flags)
        );
    }
}

/// Reads the time-stamp counter.
pub fn rdtsc() -> u64 {
    let lo: u32;
    let hi: u32;
    // SAFETY: `rdtsc` reads a CPU counter and does not violate memory safety.
    unsafe { core::arch::asm!("rdtsc", out("eax") lo, out("edx") hi, options(nostack, nomem)) };
    ((hi as u64) << 32) | lo as u64
}

impl super::Interrupts for X64 {
    fn enable_interrupts() {
        // SAFETY: Enabling interrupts via `sti` does not violate memory safety; the caller is
        // responsible for ensuring the system is ready to service interrupts.
        unsafe {
            asm!("sti", options(nostack, nomem, preserves_flags));
        }
    }

    fn disable_interrupts() {
        // SAFETY: Disabling interrupts via `cli` does not violate memory safety.
        unsafe {
            asm!("cli", options(nostack, nomem, preserves_flags));
        }
    }

    fn interrupts_enabled() -> bool {
        let eflags: u64;
        const IF: u64 = 0x200;
        // SAFETY: Reading RFLAGS via a push and pop has no side effects.
        unsafe {
            asm!("pushfq; pop {}", out(reg) eflags);
        }
        eflags & IF != 0
    }
}
