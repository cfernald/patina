//! AArch64 data cache maintenance primitives.
//!
//! ## License
//!
//! Copyright (c) Microsoft Corporation.
//!
//! SPDX-License-Identifier: Apache-2.0
//!
#[cfg(not(test))]
use core::arch::asm;
use patina::pi::protocols::cpu_arch::CpuFlushType;
use r_efi::efi;

fn clean_data_entry_by_mva(_mva: efi::PhysicalAddress) {
    #[cfg(not(test))]
    // SAFETY: Cleaning the data cache has no impact on safety invariants.
    unsafe {
        asm!("dc cvac, {}", in(reg) _mva, options(nostack, preserves_flags));
    }
}

fn invalidate_data_cache_entry_by_mva(_mva: efi::PhysicalAddress) {
    #[cfg(not(test))]
    // SAFETY: Invalidating the data cache does not impact safety checks. It
    // does have the potential to corrupt memory if used incorrectly, but the caller is
    // expected to ensure that they are using this function correctly.
    unsafe {
        asm!("dc ivac, {}", in(reg) _mva, options(nostack, preserves_flags));
    }
}

fn clean_and_invalidate_data_entry_by_mva(_mva: efi::PhysicalAddress) {
    #[cfg(not(test))]
    // SAFETY: Cleaning and invalidating the data cache does not impact safety invariants.
    unsafe {
        asm!("dc civac, {}", in(reg) _mva, options(nostack, preserves_flags));
    }
}

fn data_cache_line_len() -> u64 {
    #[cfg(test)]
    let ctr_el0 = 0x0004_0000; // Provides line size of 64 in test mode

    #[cfg(not(test))]
    // SAFETY: Reading ctr_el0 has no impact on safety invariants
    let ctr_el0 = unsafe {
        let ctr_el0: u64;
        asm!("mrs {}, ctr_el0", out(reg) ctr_el0);
        ctr_el0
    };

    4 << ((ctr_el0 >> 16) & 0xf)
}

/// Performs a data cache maintenance operation over the virtual address range
/// `[start, start + length)` according to `op`, followed by a single `dsb sy`
/// barrier. A no-op if `length` is zero.
pub(crate) fn flush_data_cache_range(start: efi::PhysicalAddress, length: u64, op: CpuFlushType) {
    if length == 0 {
        return;
    }

    let cacheline_size = data_cache_line_len();
    let cacheline_mask = cacheline_size - 1;
    let mut aligned_addr = start & !cacheline_mask;
    let end_addr = match start.checked_add(length) {
        Some(end_addr) => end_addr,
        None => {
            debug_assert!(false, "Cache range overflow");
            return;
        }
    };

    while aligned_addr < end_addr {
        match op {
            CpuFlushType::EfiCpuFlushTypeWriteBack => clean_data_entry_by_mva(aligned_addr),
            CpuFlushType::EfiCpuFlushTypeInvalidate => invalidate_data_cache_entry_by_mva(aligned_addr),
            CpuFlushType::EfiCpuFlushTypeWriteBackInvalidate => clean_and_invalidate_data_entry_by_mva(aligned_addr),
        }

        match aligned_addr.checked_add(cacheline_size) {
            Some(next) => aligned_addr = next,
            None => break,
        }
    }

    #[cfg(not(test))]
    // we have a data barrier after all cache lines have had the operation performed on them as an optimization
    // SAFETY: a data barrier has no impact on safety invariants.
    unsafe {
        asm!("dsb sy", options(nostack));
    }
}

#[cfg(test)]
#[coverage(off)]
mod tests {
    use super::*;

    #[test]
    fn test_data_cache_line_len() {
        // Always uses 64 for tests.
        assert_eq!(data_cache_line_len(), 64);
    }

    #[test]
    fn test_all_clean_types() {
        flush_data_cache_range(0, 0x1000, CpuFlushType::EfiCpuFlushTypeWriteBack);
        flush_data_cache_range(0, 0x1000, CpuFlushType::EfiCpuFlushTypeInvalidate);
        flush_data_cache_range(0, 0x1000, CpuFlushType::EfiCpuFlushTypeWriteBackInvalidate);
    }

    #[test]
    fn test_clean_overflow() {
        // Should be handled gracefully.
        flush_data_cache_range(0xFFFFFFFFFFFFF000, 0xFFFFFFFFFFFFF000, CpuFlushType::EfiCpuFlushTypeWriteBack);
    }
}
