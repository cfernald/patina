//! Implements HOB structure for configuring the Patina Debugger.
//!
//! ## License
//!
//! Copyright (C) Microsoft Corporation.
//!
//! SPDX-License-Identifier: Apache-2.0
//!

use bitfield_struct::bitfield;
use patina::base::guid::Guid;

/// Control flags for the debugger HOB.
#[bitfield(u32)]
pub struct DebuggerControlFlags {
    /// Indicates whether the debugger is enabled.
    pub enabled: bool,
    #[bits(31)]
    _unused: u32,
}

// {ceed55bc-eff6-4e08-99af-ace2336a1ad1}
// static const struct GUID __NAME__ = {0xceed55bc, 0xeff6, 0x4e08, {0x99, 0xaf, 0xac, 0xe2, 0x33, 0x6a, 0x1a, 0xd1}};

/// GUID for the Patina Debugger HOB.
pub const PATINA_DEBUGGER_HOB_GUID: Guid = Guid::from_string("CEED55BC-EFF6-4E08-99AF-ACE2336A1AD1");

/// Debugger control HOB structure.
///
/// This structure is used to pass debugger configuration to the core.
#[derive(Clone)]
#[repr(C)]
pub struct DebuggerConfigurationHob {
    /// Control flags for the DXE phase debugger.
    pub dxe_control: DebuggerControlFlags,
    /// Timeout value for the DXE phase debugger.
    pub dxe_timeout: u32,
    /// Control flags for the MM phase debugger.
    pub mm_control: DebuggerControlFlags,
    /// Timeout value for the MM phase debugger.
    pub mm_timeout: u32,
}
