//! APIC code for the BSP to quiesce APs.
//!
//! ## License
//!
//! Copyright (c) Microsoft Corporation.
//!
//! SPDX-License-Identifier: Apache-2.0
//!

use core::ptr::NonNull;
use patina::{
    arch::x64::{read_msr, write_msr},
    bit,
    mmio::{UniqueMmioPointer, fields::ReadPureWrite},
};

/// Physical base address of the xAPIC register page, in `IA32_APIC_BASE[51:12]`.
const IA32_APIC_BASE_MSR: u32 = 0x1B;
/// Bit in `IA32_APIC_BASE` that indicates x2APIC mode.
const X2APIC_ENABLE_BIT: u64 = bit!(10);
/// Mask for the physical base address of the xAPIC register page.
const APIC_BASE_MASK: u64 = 0x000F_FFFF_FFFF_F000;

/// x2APIC interrupt command register.
const X2APIC_ICR_MSR: u32 = 0x830;
/// x2APIC local APIC version register.
const X2APIC_VERSION_MSR: u32 = 0x803;
/// Byte offset of the xAPIC interrupt command register within the register page.
const XAPIC_ICR_OFFSET: usize = 0x300;
/// Byte offset of the xAPIC local APIC version register.
const XAPIC_VERSION_OFFSET: usize = 0x30;

/// Local-vector-table registers that can deliver maskable interrupts or NMIs.
const XAPIC_LVT_OFFSETS: [usize; 6] = [0x320, 0x330, 0x340, 0x350, 0x360, 0x370];
const X2APIC_LVT_MSRS: [u32; 6] = [0x832, 0x833, 0x834, 0x835, 0x836, 0x837];
const LVT_MASKED: u32 = bit!(16);
const MAX_LVT_ENTRY_SHIFT: u32 = 16;
const MAX_LVT_ENTRY_MASK: u32 = 0xFF;

const ICR_DELIVERY_OFFSET: usize = 8;
const ICR_DELIVERY_STATUS: u32 = bit!(12);
const ICR_LEVEL_OFFSET: usize = 14;
const ICR_DESTINATION_OFFSET: usize = 18;
/// ICR Delivery mode INIT.
const ICR_DELIVERY_MODE_INIT: u32 = 0b101 << ICR_DELIVERY_OFFSET;
/// ICR Level assert.
const ICR_LEVEL_ASSERT: u32 = 1 << ICR_LEVEL_OFFSET;
/// Destination shorthand "all excluding self".
const ICR_ALL_EXCLUDING_SELF: u32 = 0b11 << ICR_DESTINATION_OFFSET;

/// INIT broadcast to every processor but the caller.
const ICR_INIT_ALL_EXCLUDING_SELF: u32 = ICR_DELIVERY_MODE_INIT | ICR_LEVEL_ASSERT | ICR_ALL_EXCLUDING_SELF;

/// Whether the local APIC is operating in x2APIC mode.
pub(super) fn is_x2apic_enabled() -> bool {
    // SAFETY: Reading IA32_APIC_BASE is valid on any x86_64 platform with a local APIC.
    let base = unsafe { read_msr(IA32_APIC_BASE_MSR) };
    (base & X2APIC_ENABLE_BIT) != 0
}

pub(super) fn mask_local_interrupts() {
    if is_x2apic_enabled() {
        // SAFETY: This is the architectural x2APIC version register.
        let version = unsafe { read_msr(X2APIC_VERSION_MSR) } as u32;
        let lvt_count =
            (((version >> MAX_LVT_ENTRY_SHIFT) & MAX_LVT_ENTRY_MASK) as usize + 1).min(X2APIC_LVT_MSRS.len());
        for &msr in X2APIC_LVT_MSRS.iter().take(lvt_count) {
            // SAFETY: These architectural x2APIC MSRs are the calling processor's
            // local-vector-table registers.
            let value = unsafe { read_msr(msr) } | LVT_MASKED as u64;
            // SAFETY: Preserving the register and setting its mask bit is valid for
            // every maskable local-vector-table entry.
            unsafe { write_msr(msr, value) };
        }
    } else {
        // SAFETY: Reading IA32_APIC_BASE is valid on x86_64 systems with a local APIC.
        let base = (unsafe { read_msr(IA32_APIC_BASE_MSR) } & APIC_BASE_MASK) as usize;
        let Some(base) = NonNull::new(base as *mut u8) else {
            return;
        };

        // SAFETY: `base` is the local APIC register page and this is its version register.
        let version =
            unsafe { UniqueMmioPointer::new(base.byte_add(XAPIC_VERSION_OFFSET).cast::<ReadPureWrite<u32>>()) };
        let lvt_count =
            (((version.read() >> MAX_LVT_ENTRY_SHIFT) & MAX_LVT_ENTRY_MASK) as usize + 1).min(XAPIC_LVT_OFFSETS.len());
        for &offset in XAPIC_LVT_OFFSETS.iter().take(lvt_count) {
            // SAFETY: `base` is the local APIC register page and `offset` identifies
            // one of its local-vector-table registers.
            let mut lvt = unsafe { UniqueMmioPointer::new(base.byte_add(offset).cast::<ReadPureWrite<u32>>()) };
            let value = lvt.read() | LVT_MASKED;
            lvt.write(value);
        }
    }
}

/// Sends an INIT IPI to every processor except the caller, leaving each of them in
/// the wait-for-SIPI state. They execute nothing and hold no page tables, descriptor
/// tables or stack, but can still respond to SMIs.
pub(super) fn send_init_all_excluding_self() {
    if is_x2apic_enabled() {
        // SAFETY: Writing the x2APIC ICR sends an IPI; the encoded command is a
        // well-formed INIT broadcast.
        unsafe { write_msr(X2APIC_ICR_MSR, ICR_INIT_ALL_EXCLUDING_SELF as u64) };
    } else {
        // SAFETY: Reading IA32_APIC_BASE is valid on any x86_64 platform with a local APIC.
        let base = (unsafe { read_msr(IA32_APIC_BASE_MSR) } & APIC_BASE_MASK) as usize;
        let Some(base) = NonNull::new(base as *mut u8) else {
            log::error!("No local APIC base. Cannot send INIT to the application processors!");
            return;
        };

        // SAFETY: `base` comes from IA32_APIC_BASE, so this is the correct local APIC register page.
        let mut icr = unsafe { UniqueMmioPointer::new(base.byte_add(XAPIC_ICR_OFFSET).cast::<ReadPureWrite<u32>>()) };
        icr.write(ICR_INIT_ALL_EXCLUDING_SELF);
    }
}

/// Whether the local APIC has finished sending the most recent ICR command.
/// x2APIC does not expose delivery status, so command completion is represented
/// by the bounded settle interval applied by the caller.
pub(super) fn init_delivery_complete() -> bool {
    if is_x2apic_enabled() {
        return true;
    }

    // SAFETY: Reading IA32_APIC_BASE is valid on any x86_64 platform with a local APIC.
    let base = (unsafe { read_msr(IA32_APIC_BASE_MSR) } & APIC_BASE_MASK) as usize;
    let Some(base) = NonNull::new(base as *mut u8) else {
        return false;
    };
    // SAFETY: `base` comes from IA32_APIC_BASE, so this is the local APIC ICR.
    let icr = unsafe { UniqueMmioPointer::new(base.byte_add(XAPIC_ICR_OFFSET).cast::<ReadPureWrite<u32>>()) };
    icr.read() & ICR_DELIVERY_STATUS == 0
}
