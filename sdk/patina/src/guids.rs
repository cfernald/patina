//! Patina GUID Definitions
//!
//! GUIDs that are used for common and generic events between drivers but are not defined in a formal
//! specification.
//!
//! ## License
//!
//! Copyright (C) Microsoft Corporation.
//!
//! SPDX-License-Identifier: Apache-2.0
//!

use r_efi::efi;

/// Cache Attribute Change Event Group GUID
///
/// The GUID for an event group signaled when the cache attributes for a memory region are changed. The event group
/// is intended for architectures, such as x86, that require cache attribute changes to be propagated to all APs.
///
/// (`b8e477c7-26a9-4b9a-a7c9-5f8f1f3d9c7b`)
pub const CACHE_ATTRIBUTE_CHANGE_EVENT_GROUP: efi::Guid =
    efi::Guid::from_fields(0xb8e477c7, 0x26a9, 0x4b9a, 0xa7, 0xc9, &[0x5f, 0x8f, 0x1f, 0x3d, 0x9c, 0x7b]);

/// DXE Core Module GUID
///
/// The FFS file GUID for the DXE Core module. Interfaces that depend upon a module GUID such as the Memory Allocation
/// Module HOB and status codes that are produced by the DXE Core module will use this GUID.
///
/// Platforms that integrate the DXE Core module into their firmware volumes should use this GUID to identify the
/// DXE Core FFS file.
///
/// (`23C9322F-2AF2-476A-BC4C-26BC88266C71`)
/// ```
/// # use patina::{Guid, guids::DXE_CORE};
/// # assert_eq!("23C9322F-2AF2-476A-BC4C-26BC88266C71", format!("{:?}", Guid::from_ref(&DXE_CORE)));
/// ```
pub const DXE_CORE: efi::Guid =
    efi::Guid::from_fields(0x23C9322F, 0x2AF2, 0x476A, 0xBC, 0x4C, &[0x26, 0xBC, 0x88, 0x26, 0x6C, 0x71]);

/// Exit Boot Services Failed GUID
///
/// The GUID for an event group signaled when ExitBootServices() fails. For example, the ExitBootServices()
/// implementation may find that the memory map key provided does not match the current memory map key and return
/// an error code. This event group will be signaled in that case just before returning to the caller.
///
/// (`4f6c5507-232f-4787-b95e-72f862490cb1`)
/// ```
/// # use patina::{Guid, guids::EBS_FAILED};
/// # assert_eq!("4F6C5507-232F-4787-B95E-72F862490CB1", format!("{:?}", Guid::from_ref(&EBS_FAILED)));
/// ```
pub const EBS_FAILED: efi::Guid =
    efi::Guid::from_fields(0x4f6c5507, 0x232f, 0x4787, 0xb9, 0x5e, &[0x72, 0xf8, 0x62, 0x49, 0x0c, 0xb1]);

/// EDKII FPDT (Firmware Performance Data Table) extender firmware performance.
///
/// Use in HOB list to mark a hob as performance reports.
/// Report status code guide for FBPT address.
/// Configuration table guid for the FBPT address.
///
/// (`3B387BFD-7ABC-4CF2-A0CA-B6A16C1B1B25`)
/// ```
/// # use patina::{Guid, guids::EDKII_FPDT_EXTENDED_FIRMWARE_PERFORMANCE};
/// # assert_eq!("3B387BFD-7ABC-4CF2-A0CA-B6A16C1B1B25", format!("{:?}", Guid::from_ref(&EDKII_FPDT_EXTENDED_FIRMWARE_PERFORMANCE)));
/// ```
pub const EDKII_FPDT_EXTENDED_FIRMWARE_PERFORMANCE: efi::Guid =
    efi::Guid::from_fields(0x3b387bfd, 0x7abc, 0x4cf2, 0xa0, 0xca, &[0xb6, 0xa1, 0x6c, 0x1b, 0x1b, 0x25]);

/// End of dxe event group GUID.
///
/// (`02CE967A-DD7E-4FFC-9EE7-810CF0470880`)
/// ```
/// # use patina::{Guid, guids::EVENT_GROUP_END_OF_DXE};
/// # assert_eq!("02CE967A-DD7E-4FFC-9EE7-810CF0470880", format!("{:?}", Guid::from_ref(&EVENT_GROUP_END_OF_DXE)));
/// ```
pub const EVENT_GROUP_END_OF_DXE: efi::Guid =
    efi::Guid::from_fields(0x2ce967a, 0xdd7e, 0x4ffc, 0x9e, 0xe7, &[0x81, 0xc, 0xf0, 0x47, 0x8, 0x80]);

/// Hardware Interrupt protocol GUID.
/// This protocol provides a means of registering and unregistering interrupt handlers for AARCH64 systems.
///
/// (`2890B3EA-053D-1643-AD0C-D64808DA3FF1`)
/// ```
/// # use patina::{Guid, guids::HARDWARE_INTERRUPT_PROTOCOL};
/// # assert_eq!("2890B3EA-053D-1643-AD0C-D64808DA3FF1", format!("{:?}", Guid::from_ref(&HARDWARE_INTERRUPT_PROTOCOL)));
/// ```
pub const HARDWARE_INTERRUPT_PROTOCOL: efi::Guid =
    efi::Guid::from_fields(0x2890B3EA, 0x053D, 0x1643, 0xAD, 0x0C, &[0xD6, 0x48, 0x08, 0xDA, 0x3F, 0xF1]);

/// Hardware Interrupt v2 protocol GUID.
/// This protocol provides a means of registering and unregistering interrupt handlers for AARCH64 systems.
/// This protocol extends the Hardware Interrupt Protocol to support interrupt type query.
///
/// (`32898322-2DA1-474A-BAAA-F3F7CF569470`)
/// ```
/// # use patina::{Guid, guids::HARDWARE_INTERRUPT_PROTOCOL_V2};
/// # assert_eq!("32898322-2DA1-474A-BAAA-F3F7CF569470", format!("{:?}", Guid::from_ref(&HARDWARE_INTERRUPT_PROTOCOL_V2)));
/// ```
pub const HARDWARE_INTERRUPT_PROTOCOL_V2: efi::Guid =
    efi::Guid::from_fields(0x32898322, 0x2da1, 0x474a, 0xba, 0xaa, &[0xf3, 0xf7, 0xcf, 0x56, 0x94, 0x70]);

/// Memory Type Info GUID
///
/// The memory type information HOB and variable can be used to store information
/// for each memory type in Variable or HOB.
///
/// The Memory Type Information GUID can also be optionally used as the Owner
/// field of a Resource Descriptor HOB to provide the preferred memory range
/// for the memory types described in the Memory Type Information GUID HOB.
///
/// (`4C19049F-4137-4DD3-9C10-8B97A83FFDFA`)
/// ```
/// # use patina::{Guid, guids::MEMORY_TYPE_INFORMATION};
/// # assert_eq!("4C19049F-4137-4DD3-9C10-8B97A83FFDFA", format!("{:?}", Guid::from_ref(&MEMORY_TYPE_INFORMATION)));
/// ```
pub const MEMORY_TYPE_INFORMATION: efi::Guid =
    efi::Guid::from_fields(0x4C19049F, 0x4137, 0x4DD3, 0x9C, 0x10, &[0x8B, 0x97, 0xA8, 0x3F, 0xFD, 0xFA]);

/// Performance Protocol GUID.
///
/// This protocol provides a means of adding performace record to the Firmware Basic Boot Performance Table (FBPT).
///
/// (`76B6BDFA-2ACD-4462-9E3F-CB58C969D937`)
/// ```
/// # use patina::{Guid, guids::PERFORMANCE_PROTOCOL};
/// # assert_eq!("76B6BDFA-2ACD-4462-9E3F-CB58C969D937", format!("{:?}", Guid::from_ref(&PERFORMANCE_PROTOCOL)));
/// ```
pub const PERFORMANCE_PROTOCOL: efi::Guid =
    efi::Guid::from_fields(0x76b6bdfa, 0x2acd, 0x4462, 0x9E, 0x3F, &[0xcb, 0x58, 0xC9, 0x69, 0xd9, 0x37]);

/// EFI SMM Communication Protocol GUID as defined in the PI 1.2 specification.
///
/// This protocol provides a means of communicating between drivers outside of SMM and SMI
/// handlers inside of SMM.
///
/// (`C68ED8E2-9DC6-4CBD-9D94-DB65ACC5C332`)
/// ```
/// # use patina::{Guid, guids::SMM_COMMUNICATION_PROTOCOL};
/// # assert_eq!("C68ED8E2-9DC6-4CBD-9D94-DB65ACC5C332", format!("{:?}", Guid::from_ref(&SMM_COMMUNICATION_PROTOCOL)));
/// ```
pub const SMM_COMMUNICATION_PROTOCOL: efi::Guid =
    efi::Guid::from_fields(0xc68ed8e2, 0x9dc6, 0x4cbd, 0x9d, 0x94, &[0xdb, 0x65, 0xac, 0xc5, 0xc3, 0x32]);

/// Zero GUID
///
/// All-zero GUID, used as a marker or placeholder.
///
/// (`00000000-0000-0000-0000-000000000000`)
/// ```
/// # use patina::{Guid, guids::ZERO};
/// # assert_eq!("00000000-0000-0000-0000-000000000000", format!("{:?}", Guid::from_ref(&ZERO)));
/// ```
pub const ZERO: efi::Guid = efi::Guid::from_fields(0, 0, 0, 0, 0, &[0, 0, 0, 0, 0, 0]);

/// EFI_HOB_MEMORY_ALLOC_STACK_GUID
///
///  Describes the memory stack that is produced by the HOB producer phase and upon which all post
///  memory-installed executable content in the HOB producer phase is executing.
///
/// (`4ED4BF27-4092-42E9-807D-527B1D00C9BD`)
/// ```
/// # use patina::{Guid, guids::HOB_MEMORY_ALLOC_STACK};
/// # assert_eq!("4ED4BF27-4092-42E9-807D-527B1D00C9BD", format!("{:?}", Guid::from_ref(&HOB_MEMORY_ALLOC_STACK)));
/// ```
pub const HOB_MEMORY_ALLOC_STACK: efi::Guid =
    efi::Guid::from_fields(0x4ed4bf27, 0x4092, 0x42e9, 0x80, 0x7d, &[0x52, 0x7b, 0x1d, 0x00, 0xc9, 0xbd]);
