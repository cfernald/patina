//! UEFI CPU Crate
//!
//! This crate provides implementation for the Cpu.
//!
//! ## License
//!
//! Copyright (c) Microsoft Corporation.
//!
//! SPDX-License-Identifier: Apache-2.0
//!
#![cfg_attr(all(not(feature = "std"), not(test), not(feature = "mockall")), no_std)]
#![cfg_attr(coverage, feature(coverage_attribute))]

pub use patina::log_registers;

#[cfg(target_arch = "x86_64")]
mod x64;

// Temporary descriptor re-exports; remove after callers migrate to the x64 modules.
#[cfg(target_arch = "x86_64")]
pub use x64::gdt;
#[cfg(all(target_arch = "x86_64", target_os = "uefi"))]
pub(crate) use x64::idt;
pub mod interrupts;
#[cfg(feature = "alloc")]
pub mod mp;
pub mod paging;
pub mod save_state;
