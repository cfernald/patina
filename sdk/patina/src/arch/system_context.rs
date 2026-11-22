//! This file contains diagnostic helpers for decoding exceptions.
//!
//! ## License
//!
//! Copyright (c) Microsoft Corporation.
//! SPDX-License-Identifier: Apache-2.0

#[doc(hidden)]
pub use log::error as log_error;

mod aarch64;
mod x64;

mod sealed {
    pub trait Sealed {}

    impl Sealed for crate::standard::efi::protocols::debug_support::SystemContextX64 {}
    impl Sealed for crate::standard::efi::protocols::debug_support::SystemContextAArch64 {}
}

/// This trait provides methods to decode architectural exception data. It does not touch
/// any system state or memory outside of the saved context.
///
/// Both x64 and `AArch64` diagnostics use the same implementations on firmware and host builds.
///
/// # Examples
///
/// ```
/// use patina::{
///     arch::SystemContextDiagnostics,
///     standard::efi::protocols::debug_support::SystemContextX64,
/// };
///
/// fn report_page_fault(context: &SystemContextX64) {
///     context.dump_exception(14);
/// }
/// ```
pub trait SystemContextDiagnostics: sealed::Sealed {
    /// Logs the exception classification, decoded error information, and saved registers.
    ///
    /// `exception_type` is the exception vector ID.
    fn dump_exception(&self, exception_type: usize);

    /// Logs the saved control and general-purpose registers.
    fn dump_system_context_registers(&self);

    /// Returns the saved fault address for a page fault, if the architecture marks it valid.
    ///
    /// Returns `None` for other exceptions. `exception_type` uses the same IDs as
    /// [`Self::dump_exception`]. The address is diagnostic data, not a pointer safe to dereference.
    fn page_fault_address(&self, exception_type: usize) -> Option<u64>;
}

/// Logs register name/value pairs at error level, in groups of four per line.
///
/// Names are right-aligned to at least four characters. Values are formatted as uppercase
/// hexadecimal with a prefix and at least sixteen digits. Formatting does not allocate.
///
/// # Examples
///
/// ```
/// use patina::log_registers;
///
/// log_registers!("RAX", 0x10_u64, "RBX", 0x20_u64);
/// ```
#[macro_export]
macro_rules! log_registers {
    ( $( $name:expr, $value:expr ),+ $(,)? ) => {{
        let registers = [$(($name, $value)),+];
        for chunk in registers.chunks(4) {
            match chunk {
                [c1, c2, c3, c4] => {
                    $crate::arch::system_context::log_error!(
                        "{:>4}:  {:#018X}   {:>4}:  {:#018X}   {:>4}:  {:#018X}   {:>4}:  {:#018X}",
                        c1.0, c1.1, c2.0, c2.1, c3.0, c3.1, c4.0, c4.1
                    );
                },
                [c1, c2, c3] => {
                    $crate::arch::system_context::log_error!(
                        "{:>4}:  {:#018X}   {:>4}:  {:#018X}   {:>4}:  {:#018X}",
                        c1.0, c1.1, c2.0, c2.1, c3.0, c3.1
                    );
                },
                [c1, c2] => {
                    $crate::arch::system_context::log_error!(
                        "{:>4}:  {:#018X}   {:>4}:  {:#018X}",
                        c1.0, c1.1, c2.0, c2.1
                    );
                },
                [c1] => {
                    $crate::arch::system_context::log_error!("{:>4}:  {:#018X}", c1.0, c1.1);
                },
                _ => {
                    $crate::arch::system_context::log_error!("");
                }
            }
        }
    }};
}

fn fault_near_stack(stack_pointer: u64, fault_address: u64) -> bool {
    use crate::{UEFI_PAGE_MASK, UEFI_PAGE_SIZE};

    let stack_page = stack_pointer & !(UEFI_PAGE_MASK as u64);
    let fault_page = fault_address & !(UEFI_PAGE_MASK as u64);
    fault_page == stack_page || fault_page == stack_page.wrapping_sub(UEFI_PAGE_SIZE as u64)
}

#[cfg(test)]
#[cfg_attr(coverage, coverage(off))]
mod tests {
    use std::{cell::RefCell, sync::Once};

    std::thread_local! {
        static RECORDS: RefCell<Option<Vec<CapturedRecord>>> = const { RefCell::new(None) };
    }

    struct CapturedRecord {
        message: String,
        target: String,
        file: Option<String>,
        line: Option<u32>,
    }

    struct TestLogger;

    impl log::Log for TestLogger {
        fn enabled(&self, _metadata: &log::Metadata<'_>) -> bool {
            true
        }

        fn log(&self, record: &log::Record<'_>) {
            if record.level() == log::Level::Error {
                RECORDS.with(|records| {
                    if let Some(records) = records.borrow_mut().as_mut() {
                        records.push(CapturedRecord {
                            message: record.args().to_string(),
                            target: record.target().to_string(),
                            file: record.file().map(str::to_string),
                            line: record.line(),
                        });
                    }
                });
            }
        }

        fn flush(&self) {}
    }

    pub(super) fn capture_logs(action: impl FnOnce()) -> Vec<String> {
        capture_records(action).into_iter().map(|record| record.message).collect()
    }

    fn capture_records(action: impl FnOnce()) -> Vec<CapturedRecord> {
        static INITIALIZE: Once = Once::new();
        INITIALIZE.call_once(|| {
            log::set_logger(&TestLogger).expect("Failed to install diagnostic test logger");
            log::set_max_level(log::LevelFilter::Debug);
        });
        RECORDS.with(|records| *records.borrow_mut() = Some(Vec::new()));
        action();
        RECORDS.with(|records| records.borrow_mut().take().expect("Missing diagnostic log capture"))
    }

    #[test]
    fn test_system_context_register_groups_preserve_formatting() {
        let records = capture_logs(|| {
            log_registers!("RAX", 1_u64, "RBX", 2, "RCX", 3, "RDX", 4, "RSI", 5, "RDI", 6, "RBP", 7);
        });
        assert_eq!(
            records,
            [
                " RAX:  0x0000000000000001    RBX:  0x0000000000000002    RCX:  0x0000000000000003    RDX:  0x0000000000000004",
                " RSI:  0x0000000000000005    RDI:  0x0000000000000006    RBP:  0x0000000000000007",
            ],
        );
    }

    #[test]
    fn test_system_context_register_partial_groups_preserve_formatting() {
        let records = capture_logs(|| {
            log_registers!("RAX", 1_u64);
            log_registers!("RAX", 1_u64, "RBX", 2);
            log_registers!("RAX", 1_u64, "RBX", 2, "RCX", 3);
        });
        assert_eq!(
            records,
            [
                " RAX:  0x0000000000000001",
                " RAX:  0x0000000000000001    RBX:  0x0000000000000002",
                " RAX:  0x0000000000000001    RBX:  0x0000000000000002    RCX:  0x0000000000000003",
            ],
        );
    }

    #[test]
    fn test_system_context_register_formatting_supports_other_widths() {
        let records = capture_logs(|| log_registers!("RFLAGS", u32::MAX, "R8", 0xABC,));
        assert_eq!(records, ["RFLAGS:  0x00000000FFFFFFFF     R8:  0x0000000000000ABC"]);
    }

    #[test]
    fn test_system_context_register_logging_preserves_caller_metadata() {
        use crate as sdk_alias;

        let mut call_line = 0;
        let records = capture_records(|| {
            call_line = line!() + 1;
            sdk_alias::log_registers!("RAX", 1_u64);
        });
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].target, module_path!());
        assert_eq!(records[0].file.as_deref(), Some(file!()));
        assert_eq!(records[0].line, Some(call_line));
    }
}
