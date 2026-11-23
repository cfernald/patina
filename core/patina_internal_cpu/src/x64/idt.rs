//! X64 IDT management and descriptor construction.
//!
//! ## License
//!
//! Copyright (c) Microsoft Corporation.
//! SPDX-License-Identifier: Apache-2.0

use super::DescriptorTablePointer;

#[cfg(target_os = "uefi")]
use core::{arch::global_asm, cell::UnsafeCell};

pub(crate) const IDT_ENTRY_COUNT: usize = 256;

#[cfg(target_os = "uefi")]
global_asm!(include_str!("interrupt_handler.asm"), idt_entry_count = const IDT_ENTRY_COUNT);

#[cfg(target_os = "uefi")]
unsafe extern "efiapi" {
    fn AsmGetVectorAddress(index: usize) -> u64;
}

/// A single 16-byte gate descriptor in the IDT.
#[derive(Clone, Copy, zerocopy_derive::IntoBytes, zerocopy_derive::Immutable)]
#[repr(C)]
pub(crate) struct IdtEntry {
    offset_low: u16,
    selector: u16,
    ist: u8,
    type_attr: u8,
    offset_mid: u16,
    offset_high: u32,
    reserved: u32,
}

impl IdtEntry {
    pub(crate) const fn empty() -> Self {
        Self { offset_low: 0, selector: 0, ist: 0, type_attr: 0, offset_mid: 0, offset_high: 0, reserved: 0 }
    }

    /// Creates a present interrupt gate (DPL 0) pointing to `address`.
    pub(crate) const fn interrupt_gate(address: u64, selector: u16, ist_index: u8) -> Self {
        Self {
            offset_low: address as u16,
            selector,
            ist: ist_index & 0x7,
            type_attr: 0x8E,
            offset_mid: (address >> 16) as u16,
            offset_high: (address >> 32) as u32,
            reserved: 0,
        }
    }
}

/// The 256-entry x86-64 Interrupt Descriptor Table.
#[repr(C, align(16))]
pub(crate) struct Idt {
    entries: [IdtEntry; IDT_ENTRY_COUNT],
}

impl Idt {
    pub(crate) const fn filled(entry: IdtEntry) -> Self {
        Self { entries: [entry; IDT_ENTRY_COUNT] }
    }

    pub(crate) fn entry_mut(&mut self, vector: usize) -> Option<&mut IdtEntry> {
        self.entries.get_mut(vector)
    }

    pub(crate) fn descriptor(&self) -> DescriptorTablePointer {
        DescriptorTablePointer {
            limit: (core::mem::size_of::<Self>() - 1) as u16,
            base: core::ptr::from_ref(self) as u64,
        }
    }
}

const _: () = assert!(core::mem::size_of::<IdtEntry>() == 16);
const _: () = assert!(core::mem::size_of::<Idt>() == 4096);

#[cfg(target_os = "uefi")]
struct StaticIdt(UnsafeCell<Idt>);

#[cfg(target_os = "uefi")]
// SAFETY: IDT initialization and loading is serialized during early CPU init and concurrent
// access is not possible.
unsafe impl Sync for StaticIdt {}

#[cfg(target_os = "uefi")]
static IDT: StaticIdt = StaticIdt(UnsafeCell::new(Idt::filled(IdtEntry::empty())));

/// Gets the address of the assembly entry point for the given vector index.
#[cfg(target_os = "uefi")]
fn get_vector_address(index: usize) -> u64 {
    assert!(index < IDT_ENTRY_COUNT, "Invalid vector index! 0x{index:#X?}");
    // SAFETY: Index has been validated to be in [0, 255].
    unsafe { AsmGetVectorAddress(index) }
}

/// Fills every IDT gate with its assembly entry point using the code segment selector `cs`.
#[cfg(target_os = "uefi")]
pub(crate) fn prepare_idt(cs: u16) -> DescriptorTablePointer {
    // SAFETY: Initialization is exclusive, before concurrent interrupt table access is possible.
    let idt = unsafe { &mut *IDT.0.get() };
    for vector in 0..IDT_ENTRY_COUNT {
        // Double faults and page faults use IST 1 to ensure they have a valid stack.
        let ist_index = u8::from(vector == 8 || vector == 14);
        *idt.entry_mut(vector).expect("IDT vector must be in range") =
            IdtEntry::interrupt_gate(get_vector_address(vector), cs, ist_index);
    }
    idt.descriptor()
}

#[cfg(test)]
#[cfg_attr(coverage, coverage(off))]
mod tests {
    use super::*;
    use zerocopy::IntoBytes;

    #[test]
    fn test_idt_gate_encodes_full_address_selector_and_ist() {
        let entry = IdtEntry::interrupt_gate(0x1234_5678_9ABC_DEF0, 0x38, 1);
        assert_eq!(entry.as_bytes(), &[0xF0, 0xDE, 0x38, 0, 1, 0x8E, 0xBC, 0x9A, 0x78, 0x56, 0x34, 0x12, 0, 0, 0, 0],);
        assert_eq!(IdtEntry::interrupt_gate(0, 0, 0xFF).ist, 7);
    }

    #[test]
    fn test_idt_empty_gate_has_no_present_or_reserved_bits() {
        assert_eq!(IdtEntry::empty().as_bytes(), &[0; 16]);
    }

    #[test]
    fn test_idt_table_descriptor_and_entry_bounds() {
        let mut idt = Idt::filled(IdtEntry::empty());
        *idt.entry_mut(255).unwrap() = IdtEntry::interrupt_gate(0x1234, 0x38, 0);
        assert!(idt.entry_mut(256).is_none());
        assert_eq!(idt.entries[254].type_attr, 0);
        assert_eq!(idt.entries[255].type_attr, 0x8E);
        let descriptor = idt.descriptor();
        let limit = descriptor.limit;
        let base = descriptor.base;
        assert_eq!(limit, 4095);
        assert_eq!(base, core::ptr::from_ref(&idt) as u64);
        assert_eq!(base & 0xF, 0);
    }
}
