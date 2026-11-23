//! Shared x64 CPU capabilities and descriptor layouts.
//!
//! ## License
//!
//! Copyright (c) Microsoft Corporation.
//! SPDX-License-Identifier: Apache-2.0

#![cfg_attr(not(target_os = "uefi"), allow(dead_code))]

pub(crate) mod apic;
pub mod gdt;
pub(crate) mod idt;

/// Operand layout for the `lgdt` and `lidt` instructions.
#[repr(C, packed)]
pub(crate) struct DescriptorTablePointer {
    pub(crate) limit: u16,
    pub(crate) base: u64,
}

const _: () = assert!(core::mem::size_of::<DescriptorTablePointer>() == 10);
const _: () = assert!(core::mem::offset_of!(DescriptorTablePointer, base) == 2);
