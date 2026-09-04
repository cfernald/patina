//! `MpInformation2` HOB parsing for MP Services.
//!
//! For information on these HOBs see the EDK2 definitions.
//! - https://github.com/tianocore/edk2/blob/master/UefiCpuPkg/Include/Guid/MpInformation2.h
//! - https://github.com/tianocore/edk2/blob/master/UefiCpuPkg/Library/MpInitLib/MpHandOff.h
//!
//! ## License
//!
//! Copyright (c) Microsoft Corporation.
//!
//! SPDX-License-Identifier: Apache-2.0
//!

use alloc::vec::Vec;

use patina::component::hob::FromHob;
use patina::standard::efi::protocols::mp_services;
use patina_internal_cpu::mp::ProcessorHandOff;
use zerocopy::FromBytes;

/// One `EFI_PROCESSOR_INFORMATION` record as laid out at the start of each
/// `MP_INFORMATION2_ENTRY`. Read via `zerocopy`, then converted to the SDK's
/// [`mp_services::ProcessorInformation`] (which carries a union) afterward.
#[derive(Clone, Copy, zerocopy_derive::FromBytes)]
#[repr(C)]
struct RawProcessorInfo {
    processor_id: u64,
    status_flag: u32,
    /// package, core, thread.
    location: [u32; 3],
    /// package, module, tile, die, core, thread.
    location2: [u32; 6],
}

impl From<RawProcessorInfo> for mp_services::ProcessorInformation {
    fn from(r: RawProcessorInfo) -> Self {
        Self {
            processor_id: r.processor_id,
            status_flag: r.status_flag,
            location: mp_services::CpuPhysicalLocation {
                package: r.location[0],
                core: r.location[1],
                thread: r.location[2],
            },
            extended_information: mp_services::ExtendedProcessorInformation {
                location2: mp_services::CpuPhysicalLocation2 {
                    package: r.location2[0],
                    module: r.location2[1],
                    tile: r.location2[2],
                    die: r.location2[3],
                    core: r.location2[4],
                    thread: r.location2[5],
                },
            },
        }
    }
}

#[derive(Clone, Copy, zerocopy_derive::FromBytes)]
#[repr(C)]
struct MpInfo2Header {
    number_of_processors: u16,
    entry_size: u16,
    _version: u8,
    _reserved: [u8; 3],
    processor_index: u64,
}

/// EDK2's `MP_INFORMATION2_HOB_DATA` (`gMpInformation2HobGuid`)
///
/// See: https://github.com/tianocore/edk2/blob/master/UefiCpuPkg/Include/Guid/MpInformation2.h
pub(super) struct MpInformation2 {
    processor_index: usize,
    processors: Vec<mp_services::ProcessorInformation>,
}

impl MpInformation2 {
    pub(super) fn processor_index(&self) -> usize {
        self.processor_index
    }

    /// Per-processor information entries carried by this HOB instance.
    pub(super) fn processors(&self) -> &[mp_services::ProcessorInformation] {
        &self.processors
    }
}

impl FromHob for MpInformation2 {
    const HOB_GUID: patina::BinaryGuid = patina::BinaryGuid::from_string("417A7F64-F4E9-4B32-846A-5CC4D8621879");

    fn parse(bytes: &[u8]) -> Self {
        let Ok((header, _)) = MpInfo2Header::read_from_prefix(bytes) else {
            return Self { processor_index: 0, processors: Vec::new() };
        };
        // `EntrySize` is the stride between entries; fall back to the size of
        // `RawProcessorInfo` if the firmware reports a smaller/zero value.
        let entry_size = (header.entry_size as usize).max(core::mem::size_of::<RawProcessorInfo>());

        let mut processors = Vec::new();
        let mut off = core::mem::size_of::<MpInfo2Header>();
        for _ in 0..header.number_of_processors {
            let Some(entry) = bytes.get(off..) else { break };
            let Ok((raw, _)) = RawProcessorInfo::read_from_prefix(entry) else { break };
            processors.push(raw.into());
            off = match off.checked_add(entry_size) {
                Some(next) => next,
                None => break,
            };
        }
        Self { processor_index: header.processor_index as usize, processors }
    }
}

#[derive(Clone, Copy, zerocopy_derive::FromBytes)]
#[repr(C)]
struct RawProcessorHandOff {
    apic_id: u32,
    _health: u32,
    startup_signal_address: u64,
    startup_procedure_address: u64,
}

/// EDK2's `MP_HAND_OFF` header (`gMpHandOffGuid`)
///
/// See: https://github.com/tianocore/edk2/blob/master/UefiCpuPkg/Library/MpInitLib/MpHandOff.h
#[derive(Clone, Copy, zerocopy_derive::FromBytes)]
#[repr(C)]
struct MpHandOffHeader {
    processor_index: u32,
    cpu_count: u32,
}

/// Parsed EDK II `MP_HAND_OFF` (`gMpHandOffGuid`): the per-processor PEI-to-DXE
/// handoff records for the processor range this HOB instance describes.
pub(super) struct MpHandOff {
    processor_index: usize,
    processors: Vec<ProcessorHandOff>,
}

impl MpHandOff {
    pub(super) fn processor_index(&self) -> usize {
        self.processor_index
    }

    /// Per-processor handoff records carried by this HOB instance.
    pub(super) fn processors(&self) -> &[ProcessorHandOff] {
        &self.processors
    }
}

impl FromHob for MpHandOff {
    const HOB_GUID: patina::BinaryGuid = patina::BinaryGuid::from_string("11E2BD88-ED38-4ABD-A399-21F25FD07A60");

    fn parse(bytes: &[u8]) -> Self {
        let Ok((header, _)) = MpHandOffHeader::read_from_prefix(bytes) else {
            return Self { processor_index: 0, processors: Vec::new() };
        };

        let mut processors = Vec::new();
        let mut off = core::mem::size_of::<MpHandOffHeader>();
        for _ in 0..header.cpu_count {
            let Some(entry) = bytes.get(off..) else { break };
            let Ok((raw, _)) = RawProcessorHandOff::read_from_prefix(entry) else { break };
            processors.push(ProcessorHandOff {
                processor_id: raw.apic_id,
                startup_signal_address: raw.startup_signal_address,
                startup_procedure_address: raw.startup_procedure_address,
            });
            off = match off.checked_add(core::mem::size_of::<RawProcessorHandOff>()) {
                Some(next) => next,
                None => break,
            };
        }
        Self { processor_index: header.processor_index as usize, processors }
    }
}

/// EDK2's `MP_HAND_OFF_CONFIG` (`gMpHandOffConfigGuid`)
///
/// See: https://github.com/tianocore/edk2/blob/master/UefiCpuPkg/Library/MpInitLib/MpHandOff.h
#[derive(Clone, Copy, zerocopy_derive::FromBytes)]
#[repr(C)]
pub(super) struct MpHandOffConfig {
    pub wait_loop_execution_mode: u32,
    pub startup_signal_value: u32,
}

impl FromHob for MpHandOffConfig {
    const HOB_GUID: patina::BinaryGuid = patina::BinaryGuid::from_string("DABBD793-7B46-4144-8AD4-101C7C08EBFA");

    fn parse(bytes: &[u8]) -> Self {
        Self::read_from_prefix(bytes)
            .map(|(config, _)| config)
            .unwrap_or(Self { wait_loop_execution_mode: 0, startup_signal_value: 0 })
    }
}
