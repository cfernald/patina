//! AArch64 CPU initialization implementation
//!
//! ## License
//!
//! Copyright (c) Microsoft Corporation.
//!
//! SPDX-License-Identifier: Apache-2.0
//!
use crate::cpu::Cpu;
#[cfg(not(test))]
use core::arch::asm;
use patina::{
    error::EfiError,
    pi::protocols::cpu_arch::{CpuFlushType, CpuInitType},
};
use r_efi::efi;

/// Struct to implement AArch64 Cpu Init.
///
/// This struct cannot be used directly. It replaces the `EfiCpu` struct when compiling for the AArch64 architecture.
#[derive(Default)]
pub struct EfiCpuAarch64;

#[allow(dead_code)]
impl EfiCpuAarch64 {
    /// This function initializes the CPU for the AArch64 architecture.
    pub fn initialize(&mut self) -> Result<(), EfiError> {
        Ok(())
    }

    // AArch64 related cache functions
    fn cache_range_operation(&self, _start: efi::PhysicalAddress, _length: u64, _op: CpuFlushType) {
        let cacheline_alignment = self.data_cache_line_len() - 1;
        let mut aligned_addr = _start - (_start & cacheline_alignment);
        let end_addr = _start + _length;

        loop {
            match _op {
                CpuFlushType::EfiCpuFlushTypeWriteBack => self.clean_data_entry_by_mva(aligned_addr),
                CpuFlushType::EfiCpuFlushTypeInvalidate => self.invalidate_data_cache_entry_by_mva(aligned_addr),
                CpuFlushType::EfiCpuFlushTypeWriteBackInvalidate => {
                    self.clean_and_invalidate_data_entry_by_mva(aligned_addr)
                }
            }

            aligned_addr += cacheline_alignment;
            if aligned_addr >= end_addr {
                break;
            }
        }

        #[cfg(not(test))]
        // we have a data barrier after all cache lines have had the operation performed on them as an optimization
        // SAFETY: a data barrier has no impact on safety invariants.
        unsafe {
            asm!("dsb sy", options(nostack));
        }
    }

    fn clean_data_entry_by_mva(&self, _mva: efi::PhysicalAddress) {
        #[cfg(not(test))]
        // SAFETY: Cleaning the data cache has no impact on safety invariants.
        unsafe {
            asm!("dc cvac, {}", in(reg) _mva, options(nostack, preserves_flags));
        }
    }

    fn invalidate_data_cache_entry_by_mva(&self, _mva: efi::PhysicalAddress) {
        #[cfg(not(test))]
        // SAFETY: Invalidating the data cache does not impact safety checks. It
        // does have the potential to corrupt memory if used incorrectly, but the caller is
        // expected to ensure that they are using this function correctly.
        unsafe {
            asm!("dc ivac, {}", in(reg) _mva, options(nostack, preserves_flags));
        }
    }

    fn clean_and_invalidate_data_entry_by_mva(&self, _mva: efi::PhysicalAddress) {
        #[cfg(not(test))]
        // SAFETY: Cleaning and invalidating the data cache does not impact safety invariants.
        unsafe {
            asm!("dc civac, {}", in(reg) _mva, options(nostack, preserves_flags));
        }
    }

    fn data_cache_line_len(&self) -> u64 {
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

    /// Causes the CPU to enter a low power state until the next interrupt.
    // This routine only does bare-metal hardware access, so no coverage.
    #[coverage(off)]
    pub fn sleep() {
        #[cfg(not(test))]
        // SAFETY: The caller is expected to ensure that they want to wait for an interrupt
        unsafe {
            asm!("wfi", options(nostack));
        }
    }
}

impl Cpu for EfiCpuAarch64 {
    fn flush_data_cache(
        &self,
        start: efi::PhysicalAddress,
        length: u64,
        flush_type: CpuFlushType,
    ) -> Result<(), EfiError> {
        self.cache_range_operation(start, length, flush_type);
        Ok(())
    }

    fn init(&self, _init_type: CpuInitType) -> Result<(), EfiError> {
        unimplemented!("init not implemented for AArch64")
    }

    fn get_timer_value(&self, _timer_index: u32) -> Result<(u64, u64), EfiError> {
        Err(EfiError::Unsupported)
    }

    fn cache_writeback_granule(&self) -> u32 {
        #[cfg(not(test))]
        // SAFETY: CTR_EL0 is a read-only system register accessible at all exception levels
        let ctr_el0 = unsafe {
            let ctr_el0: u64;
            asm!("mrs {}, ctr_el0", out(reg) ctr_el0);
            ctr_el0
        };

        #[cfg(test)]
        let ctr_el0 = 0x0400_0000; // Provides granule of 64 in test mode

        // CWG (Cache Writeback Granule): CTR_EL0 bits [27:24]
        let cwg = ((ctr_el0 >> 24) & 0xF) as u32;

        // CWG is Log2 of the max size in words
        if cwg > 0 {
            4 << cwg
        } else {
            patina::base::SIZE_2KB as u32 // Default to 2K if register contains 0 per Armv8-A spec
        }
    }
}

#[cfg(test)]
#[coverage(off)]
mod tests {
    use super::*;

    #[test]
    fn test_initialize() {
        let mut cpu_init = EfiCpuAarch64;
        assert!(cpu_init.initialize().is_ok());
    }

    #[test]
    fn test_flush_data_cache() {
        let cpu_init = EfiCpuAarch64;

        let start: efi::PhysicalAddress = 0;
        let length: u64 = 0;
        let flush_type: CpuFlushType = CpuFlushType::EfiCpuFlushTypeWriteBackInvalidate;
        assert_eq!(cpu_init.flush_data_cache(start, length, flush_type), Ok(()));

        let start: efi::PhysicalAddress = 0;
        let length: u64 = 0;
        let flush_type: CpuFlushType = CpuFlushType::EfiCpuFlushTypeInvalidate;
        assert_eq!(cpu_init.flush_data_cache(start, length, flush_type), Ok(()));

        let start: efi::PhysicalAddress = 0;
        let length: u64 = 0;
        let flush_type: CpuFlushType = CpuFlushType::EfiCpuFlushTypeWriteBack;
        assert_eq!(cpu_init.flush_data_cache(start, length, flush_type), Ok(()));
    }

    #[test]
    fn test_get_timer_value() {
        let cpu_init = EfiCpuAarch64;

        assert_eq!(cpu_init.get_timer_value(1), Err(EfiError::Unsupported));
        assert_eq!(cpu_init.get_timer_value(0), Err(EfiError::Unsupported));
    }

    #[test]
    fn test_cache_writeback_granule() {
        let cpu_init = EfiCpuAarch64;
        let granule = cpu_init.cache_writeback_granule();
        assert!(granule == 64);
    }
}
