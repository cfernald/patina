//! Saved x64 exception context diagnostics.
//!
//! ## License
//!
//! Copyright (c) Microsoft Corporation.
//! SPDX-License-Identifier: Apache-2.0

use super::{SystemContextDiagnostics, fault_near_stack};
use crate::{bit, log_registers, standard::efi::protocols::debug_support::SystemContextX64};

impl SystemContextDiagnostics for SystemContextX64 {
    fn dump_exception(&self, exception_type: usize) {
        match exception_type {
            8 | 13 => {
                if exception_type == 8 {
                    log::error!("EXCEPTION: DOUBLE FAULT");
                } else {
                    log::error!("EXCEPTION: GP FAULT");
                }
                log::error!("Instruction Pointer: {:#X?}", self.rip);
                log::error!("Code Segment: {:#X?}", self.cs);
                log::error!("RFLAGS: {:#X?}", self.rflags);
                log::error!("Stack Segment: {:#X?}", self.ss);
                log::error!("Stack Pointer: {:#X?}", self.rsp);
                log::error!("Data Segment: {:#X?}", self.ds);
                log::error!("Paging Enable: {}", self.cr0 & 0x80000000 != 0);
                log::error!("Protection Enable: {}", self.cr0 & 0x00000001 != 0);
                log::error!("Page Directory Base: {:#X?}", self.cr3);
                log::error!("Control Flags (cr4): {:#X?}", self.cr4);
                if exception_type == 13 {
                    interpret_gp_fault_exception_data(self.exception_data);
                }
                log::error!("");
                self.dump_system_context_registers();
            }
            14 => {
                let data_access = (self.exception_data & bit!(4)) == 0;
                // Data faults on the stack page or the page below can indicate stack overflow.
                if fault_near_stack(self.rsp, self.cr2) && data_access {
                    log::error!("EXCEPTION: PAGE FAULT STACK OVERFLOW");
                } else {
                    log::error!("EXCEPTION: PAGE FAULT");
                }
                log::error!("Accessed Address: {:#X?}", self.cr2);
                log::error!("Paging Enabled: {}", self.cr0 & 0x80000000 != 0);
                log::error!("Instruction Pointer: {:#X?}", self.rip);
                log::error!("Code Segment: {:#X?}", self.cs);
                log::error!("RFLAGS: {:#X?}", self.rflags);
                log::error!("Stack Segment: {:#X?}", self.ss);
                log::error!("Data Segment: {:#X?}", self.ds);
                log::error!("Stack Pointer: {:#X?}", self.rsp);
                log::error!("Page Directory Base: {:#X?}", self.cr3);
                log::error!("Paging Features (cr4): {:#X?}", self.cr4);
                interpret_page_fault_exception_data(self.exception_data);
                log::error!("");
                self.dump_system_context_registers();
            }
            _ => {
                log::error!("EXCEPTION: {exception_type}");
                self.dump_system_context_registers();
            }
        }
    }

    fn dump_system_context_registers(&self) {
        log::error!("Control Registers:");
        log_registers!(
            "CR0",
            self.cr0,
            "CR2",
            self.cr2,
            "CR3",
            self.cr3,
            "CR4",
            self.cr4,
            "RIP",
            self.rip,
            "CS",
            self.cs,
            "SS",
            self.ss,
            "DS",
            self.ds,
            "RSP",
            self.rsp,
            "RFLAGS",
            self.rflags
        );
        log::error!("");
        log::error!("General-Purpose Registers:");
        log_registers!(
            "RAX", self.rax, "RBX", self.rbx, "RCX", self.rcx, "RDX", self.rdx, "RSI", self.rsi, "RDI", self.rdi,
            "RBP", self.rbp, "R8", self.r8, "R9", self.r9, "R10", self.r10, "R11", self.r11, "R12", self.r12, "R13",
            self.r13, "R14", self.r14, "R15", self.r15
        );
        log::debug!("Full Context: {self:#X?}");
    }

    fn page_fault_address(&self, exception_type: usize) -> Option<u64> {
        (exception_type == 14).then_some(self.cr2)
    }
}

// See Intel SDM Vol 3A section 7.15.
fn interpret_page_fault_exception_data(exception_data: u64) {
    log::error!("Error Code: {exception_data:#X?}");
    if (exception_data & bit!(0)) == 0 {
        log::error!("Page not present");
    } else {
        log::error!("Page-level protection violation");
    }

    if (exception_data & bit!(1)) == 0 {
        log::error!("R/W: Read");
    } else {
        log::error!("R/W: Write");
    }

    if (exception_data & bit!(2)) != 0 {
        log::error!("User-mode access violation");
    } else {
        log::error!("Supervisor-mode access violation");
    }

    if (exception_data & bit!(3)) != 0 {
        log::error!("Reserved bit violation");
    }

    if (exception_data & bit!(4)) == 0 {
        log::error!("Data access");
    } else {
        log::error!("Instruction fetch access");
    }
}

// See Intel SDM Vol 3A section 7.15.
fn interpret_gp_fault_exception_data(exception_data: u64) {
    if exception_data != 0 {
        log::error!("Segment descriptor or IDT vector number: {exception_data:#X?}");
    } else {
        log::error!("Not from loading a segment descriptor");
    }
}

#[cfg(test)]
#[cfg_attr(coverage, coverage(off))]
mod tests {
    use super::*;
    use crate::arch::system_context::tests::capture_logs;

    fn context() -> SystemContextX64 {
        // SAFETY: The saved context contains only integers and arrays, all valid when zeroed.
        unsafe { core::mem::zeroed() }
    }

    #[test]
    fn test_x64_diagnostics_page_fault_address() {
        let mut context = context();
        for address in [0, 0x1234_5678_9ABC_DEF0] {
            context.cr2 = address;
            for vector in 0..256 {
                assert_eq!(context.page_fault_address(vector), (vector == 14).then_some(address));
            }
        }
    }

    #[test]
    fn test_x64_diagnostics_double_fault() {
        let mut context = context();
        context.cr0 = 0x80000001;
        let records = capture_logs(|| context.dump_exception(8));
        assert_eq!(records[0], "EXCEPTION: DOUBLE FAULT");
        assert!(records.iter().any(|record| record == "Paging Enable: true"));
        assert!(records.iter().any(|record| record == "Protection Enable: true"));
        assert!(!records.iter().any(|record| record.contains("segment descriptor")));
    }

    #[test]
    fn test_x64_diagnostics_general_protection_fault() {
        let mut context = context();
        for (exception_data, expected) in
            [(0, "Not from loading a segment descriptor"), (0x28, "Segment descriptor or IDT vector number: 0x28")]
        {
            context.exception_data = exception_data;
            let records = capture_logs(|| context.dump_exception(13));
            assert_eq!(records[0], "EXCEPTION: GP FAULT");
            assert!(records.iter().any(|record| record == expected));
            assert!(records.iter().any(|record| record == "Paging Enable: false"));
            assert!(records.iter().any(|record| record == "Protection Enable: false"));
        }
    }

    #[test]
    fn test_x64_diagnostics_page_fault_error_bits() {
        let mut context = context();
        context.rsp = 0x8000;
        context.cr2 = 0x1000;
        let records = capture_logs(|| context.dump_exception(14));
        assert_eq!(records[0], "EXCEPTION: PAGE FAULT");
        for expected in
            ["Error Code: 0x0", "Page not present", "R/W: Read", "Supervisor-mode access violation", "Data access"]
        {
            assert!(records.iter().any(|record| record == expected), "{expected}");
        }
        assert!(!records.iter().any(|record| record == "Reserved bit violation"));

        context.exception_data = 0x1F;
        let records = capture_logs(|| context.dump_exception(14));
        for expected in [
            "Error Code: 0x1F",
            "Page-level protection violation",
            "R/W: Write",
            "User-mode access violation",
            "Reserved bit violation",
            "Instruction fetch access",
        ] {
            assert!(records.iter().any(|record| record == expected), "{expected}");
        }
    }

    #[test]
    fn test_x64_diagnostics_stack_overflow_heuristic() {
        let mut context = context();
        for (rsp, cr2, exception_data, overflow) in [
            (0x8123, 0x8FFF, 0, true),
            (0x8123, 0x7FFF, 0, true),
            (0x8123, 0x6FFF, 0, false),
            (0x8123, 0x9000, 0, false),
            (0x8123, 0x8FFF, bit!(4), false),
            (0, u64::MAX, 0, true),
        ] {
            context.rsp = rsp;
            context.cr2 = cr2;
            context.exception_data = exception_data;
            let records = capture_logs(|| context.dump_exception(14));
            assert_eq!(
                records[0],
                if overflow { "EXCEPTION: PAGE FAULT STACK OVERFLOW" } else { "EXCEPTION: PAGE FAULT" },
            );
        }
    }

    #[test]
    fn test_x64_diagnostics_other_exception() {
        let records = capture_logs(|| context().dump_exception(6));
        assert_eq!(records[0], "EXCEPTION: 6");
        assert_eq!(records[1], "Control Registers:");
    }

    #[test]
    fn test_x64_diagnostics_register_values_and_order() {
        let mut context = context();
        (
            context.cr0,
            context.cr2,
            context.cr3,
            context.cr4,
            context.rip,
            context.cs,
            context.ss,
            context.ds,
            context.rsp,
            context.rflags,
        ) = (1, 2, 3, 4, 5, 6, 7, 8, 9, 10);
        (
            context.rax,
            context.rbx,
            context.rcx,
            context.rdx,
            context.rsi,
            context.rdi,
            context.rbp,
            context.r8,
            context.r9,
            context.r10,
            context.r11,
            context.r12,
            context.r13,
            context.r14,
            context.r15,
        ) = (11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25);
        let records = capture_logs(|| context.dump_system_context_registers());
        assert_eq!(records.len(), 10);
        assert_eq!(records[0], "Control Registers:");
        assert_eq!(records[4], "");
        assert_eq!(records[5], "General-Purpose Registers:");
        for (index, name) in ["CR0", "CR2", "CR3", "CR4", "RIP", "CS", "SS", "DS", "RSP", "RFLAGS"].iter().enumerate() {
            assert!(records[1 + index / 4].contains(&format!("{name:>4}:  {:#018X}", index + 1)));
        }
        for (index, name) in
            ["RAX", "RBX", "RCX", "RDX", "RSI", "RDI", "RBP", "R8", "R9", "R10", "R11", "R12", "R13", "R14", "R15"]
                .iter()
                .enumerate()
        {
            assert!(records[6 + index / 4].contains(&format!("{name:>4}:  {:#018X}", index + 11)));
        }
    }
}
