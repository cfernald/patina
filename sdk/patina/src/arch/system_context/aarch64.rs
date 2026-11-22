//! Saved `AArch64` exception context diagnostics.
//!
//! ## License
//!
//! Copyright (c) Microsoft Corporation.
//! SPDX-License-Identifier: Apache-2.0

use super::{SystemContextDiagnostics, fault_near_stack};
use crate::{bit, log_registers, standard::efi::protocols::debug_support::SystemContextAArch64};

impl SystemContextDiagnostics for SystemContextAArch64 {
    fn dump_exception(&self, exception_type: usize) {
        if exception_type == 0 {
            log::error!("");
            log::error!("EXCEPTION: Synchronous Exception");
            log::error!("");

            let ec = (self.esr >> 26) & 0x3F;
            let iss = self.esr & 0xFFFFFF;
            let far_valid = (iss & bit!(10)) == 0;

            if ec == 0x20 || ec == 0x21 {
                log::error!("Page Fault (Instruction Abort)");
            } else if ec == 0x24 || ec == 0x25 {
                if far_valid && fault_near_stack(self.sp, self.far) {
                    log::error!("Page Fault (Stack Overflow)");
                } else {
                    log::error!("Page Fault (Data Abort)");
                }
            }
            log::error!("");

            if !far_valid {
                log::error!("FAR not valid, page table won't be dumped");
            }
        } else {
            log::error!("EXCEPTION: {exception_type}");
        }
        self.dump_system_context_registers();
        log::error!("");
    }

    fn dump_system_context_registers(&self) {
        log::error!("Exception Registers:");
        log_registers!("ESR", self.esr, "ELR", self.elr, "SPSR", self.spsr, "FAR", self.far,);
        log::error!("");
        log::error!("General-Purpose Registers:");
        log_registers!(
            "x0", self.x0, "x1", self.x1, "x2", self.x2, "x3", self.x3, "x4", self.x4, "x5", self.x5, "x6", self.x6,
            "x7", self.x7, "x8", self.x8, "x9", self.x9, "x10", self.x10, "x11", self.x11, "x12", self.x12, "x13",
            self.x13, "x14", self.x14, "x15", self.x15, "x16", self.x16, "x17", self.x17, "x18", self.x18, "x19",
            self.x19, "x20", self.x20, "x21", self.x21, "x22", self.x22, "x23", self.x23, "x24", self.x24, "x25",
            self.x25, "x26", self.x26, "x27", self.x27, "x28", self.x28, "fp", self.fp, "lr", self.lr, "sp", self.sp
        );
        log::debug!("Full Context: {self:#X?}");
    }

    fn page_fault_address(&self, exception_type: usize) -> Option<u64> {
        let ec = (self.esr >> 26) & 0x3F;
        let iss = self.esr & 0xFFFFFF;
        let page_fault = ec == 0x20 || ec == 0x21 || ec == 0x24 || ec == 0x25;
        let far_valid = (iss & bit!(10)) == 0;
        (exception_type == 0 && page_fault && far_valid).then_some(self.far)
    }
}

#[cfg(test)]
#[cfg_attr(coverage, coverage(off))]
mod tests {
    use super::*;
    use crate::arch::system_context::tests::capture_logs;

    fn context() -> SystemContextAArch64 {
        // SAFETY: The saved context contains only integers and arrays, all valid when zeroed.
        unsafe { core::mem::zeroed() }
    }

    #[test]
    fn test_aarch64_diagnostics_page_fault_address() {
        let mut context = context();
        for address in [0, 0x1234_5678_9ABC_DEF0] {
            context.far = address;
            for ec in 0..64 {
                for invalid in [false, true] {
                    context.esr = (ec << 26) | if invalid { bit!(10) } else { 0 };
                    let page_fault = matches!(ec, 0x20 | 0x21 | 0x24 | 0x25);
                    assert_eq!(context.page_fault_address(0), (page_fault && !invalid).then_some(address));
                    for vector in 1..4 {
                        assert_eq!(context.page_fault_address(vector), None);
                    }
                }
            }
        }
    }

    #[test]
    fn test_aarch64_diagnostics_instruction_abort() {
        let mut context = context();
        for ec in [0x20, 0x21] {
            context.esr = ec << 26;
            let records = capture_logs(|| context.dump_exception(0));
            assert_eq!(records[1], "EXCEPTION: Synchronous Exception");
            assert_eq!(records[3], "Page Fault (Instruction Abort)");
            assert!(!records.iter().any(|record| record.contains("Stack Overflow")));
        }
    }

    #[test]
    fn test_aarch64_diagnostics_data_abort_stack_overflow_heuristic() {
        let mut context = context();
        for ec in [0x24, 0x25] {
            for (sp, far, invalid, overflow) in [
                (0x8123, 0x8FFF, false, true),
                (0x8123, 0x7FFF, false, true),
                (0x8123, 0x6FFF, false, false),
                (0x8123, 0x9000, false, false),
                (0x8123, 0x8FFF, true, false),
                (0, u64::MAX, false, true),
            ] {
                context.esr = (ec << 26) | if invalid { bit!(10) } else { 0 };
                context.sp = sp;
                context.far = far;
                let records = capture_logs(|| context.dump_exception(0));
                assert_eq!(
                    records[3],
                    if overflow { "Page Fault (Stack Overflow)" } else { "Page Fault (Data Abort)" },
                );
                assert_eq!(records.iter().any(|record| record == "FAR not valid, page table won't be dumped"), invalid,);
            }
        }
    }

    #[test]
    fn test_aarch64_diagnostics_non_abort_synchronous_exception() {
        let records = capture_logs(|| context().dump_exception(0));
        assert_eq!(records[1], "EXCEPTION: Synchronous Exception");
        assert!(!records.iter().any(|record| record.starts_with("Page Fault")));
        assert!(records.iter().any(|record| record == "Exception Registers:"));
        assert_eq!(records.last().unwrap(), "");
    }

    #[test]
    fn test_aarch64_diagnostics_other_exception() {
        let records = capture_logs(|| context().dump_exception(2));
        assert_eq!(records[0], "EXCEPTION: 2");
        assert_eq!(records[1], "Exception Registers:");
        assert_eq!(records.last().unwrap(), "");
    }

    #[test]
    fn test_aarch64_diagnostics_register_values_and_order() {
        let mut context = context();
        (
            context.x0,
            context.x1,
            context.x2,
            context.x3,
            context.x4,
            context.x5,
            context.x6,
            context.x7,
            context.x8,
            context.x9,
            context.x10,
            context.x11,
            context.x12,
            context.x13,
            context.x14,
            context.x15,
            context.x16,
            context.x17,
            context.x18,
            context.x19,
            context.x20,
            context.x21,
            context.x22,
            context.x23,
            context.x24,
            context.x25,
            context.x26,
            context.x27,
            context.x28,
            context.fp,
            context.lr,
            context.sp,
        ) = (
            1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29,
            30, 31, 32,
        );
        (context.esr, context.elr, context.spsr, context.far) = (33, 34, 35, 36);
        let records = capture_logs(|| context.dump_system_context_registers());
        assert_eq!(records.len(), 12);
        assert_eq!(records[0], "Exception Registers:");
        assert_eq!(records[2], "");
        assert_eq!(records[3], "General-Purpose Registers:");
        for (index, name) in ["ESR", "ELR", "SPSR", "FAR"].iter().enumerate() {
            assert!(records[1].contains(&format!("{name:>4}:  {:#018X}", index + 33)));
        }
        for index in 0..32 {
            let name = match index {
                29 => "fp".to_string(),
                30 => "lr".to_string(),
                31 => "sp".to_string(),
                _ => format!("x{index}"),
            };
            assert!(records[4 + index / 4].contains(&format!("{name:>4}:  {:#018X}", index + 1)));
        }
    }
}
