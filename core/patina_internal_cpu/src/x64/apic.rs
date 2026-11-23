//! Local APIC access shared by interrupt initialization and MP support.
//!
//! ## License
//!
//! Copyright (c) Microsoft Corporation.
//! SPDX-License-Identifier: Apache-2.0

use core::{arch::x86_64::CpuidResult, num::NonZeroUsize, ptr::NonNull};
use patina::{
    arch::x64::{read_msr, write_msr},
    bit,
    error::EfiError,
    mmio::{
        UniqueMmioPointer,
        fields::{ReadPureWrite, WriteOnly},
    },
};

const APIC_BASE_MSR: u32 = 0x1B;
const APIC_BASE_MASK: u64 = 0x000F_FFFF_FFFF_F000;
const APIC_ENABLE: u64 = bit!(11);
const X2APIC_ENABLE: u64 = bit!(10);
const X2APIC_MSR_BASE: u32 = 0x800;
const VERSION_OFFSET: usize = 0x30;
const SVR_OFFSET: usize = 0xF0;
const ICR_LOW_OFFSET: usize = 0x300;
const ICR_HIGH_OFFSET: usize = 0x310;
const LINT0_OFFSET: usize = 0x350;
const LINT1_OFFSET: usize = 0x360;
const LVT_OFFSETS: [usize; 6] = [0x320, 0x330, 0x340, LINT0_OFFSET, LINT1_OFFSET, 0x370];
const MASKED: u32 = bit!(16);
const ICR_DELIVERY_STATUS: u32 = bit!(12);
const ICR_LEVEL_ASSERT: u32 = bit!(14);
const ICR_DELIVERY_MODE_INIT: u32 = 0b101 << 8;
const ICR_DELIVERY_MODE_STARTUP: u32 = 0b110 << 8;

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
enum ApicMode {
    XApic(NonZeroUsize),
    X2Apic,
}

impl ApicMode {
    fn from_base(apic_base: u64) -> Result<Self, EfiError> {
        if apic_base & APIC_ENABLE == 0 {
            return Err(EfiError::Unsupported);
        }
        if apic_base & X2APIC_ENABLE != 0 {
            return Ok(Self::X2Apic);
        }
        NonZeroUsize::new((apic_base & APIC_BASE_MASK) as usize).map(Self::XApic).ok_or(EfiError::Unsupported)
    }
}

#[cfg_attr(test, mockall::automock)]
trait RegisterAccess {
    fn cpuid(&self, leaf: u32, subleaf: u32) -> CpuidResult;
    fn read_msr(&self, msr: u32) -> u64;
    fn write_msr(&self, msr: u32, value: u64);
    fn read_mmio(&self, address: usize) -> u32;
    fn write_mmio(&self, address: usize, value: u32);
}

struct HardwareAccess;

#[cfg_attr(coverage, coverage(off))]
impl RegisterAccess for HardwareAccess {
    fn cpuid(&self, leaf: u32, subleaf: u32) -> CpuidResult {
        #[allow(unused_unsafe)]
        // SAFETY: Reading CPUID does not violate memory safety.
        unsafe {
            core::arch::x86_64::__cpuid_count(leaf, subleaf)
        }
    }

    fn read_msr(&self, msr: u32) -> u64 {
        // SAFETY: The driver checks MSR support before reading APIC_BASE and only accesses
        // architectural x2APIC MSRs after validating that x2APIC is enabled.
        unsafe { read_msr(msr) }
    }

    fn write_msr(&self, msr: u32, value: u64) {
        // SAFETY: The driver only writes architectural registers of the enabled x2APIC.
        unsafe { write_msr(msr, value) };
    }

    fn read_mmio(&self, address: usize) -> u32 {
        let address = NonNull::new(address as *mut ReadPureWrite<u32>).expect("Validated APIC register address");
        // SAFETY: The driver supplies aligned addresses in the enabled local APIC's mapped
        // register page. These registers have no read side effects.
        unsafe { UniqueMmioPointer::new(address) }.read()
    }

    fn write_mmio(&self, address: usize, value: u32) {
        let address = NonNull::new(address as *mut WriteOnly<u32>).expect("Validated APIC register address");
        // SAFETY: The driver supplies aligned addresses in the enabled local APIC's mapped
        // register page and writes only documented register fields.
        unsafe { UniqueMmioPointer::new(address) }.write(value);
    }
}

#[cfg_attr(test, mockall::automock)]
pub(crate) trait ApicBackend {
    fn max_apic_id(&self) -> u32;
    fn current_apic_id(&self) -> u32;
    fn send_icr(&self, apic_id: u32, command: u32);
    fn mask_local_interrupts(&self);
    fn initialize_virtual_wire_mode(&self);
}

struct LocalApic<R> {
    mode: ApicMode,
    registers: R,
}

impl<R: RegisterAccess> LocalApic<R> {
    fn detect(registers: R) -> Result<Self, EfiError> {
        let features = registers.cpuid(1, 0).edx;
        let required = bit!(5) | bit!(9);
        if features & required != required {
            return Err(EfiError::Unsupported);
        }
        let mode = ApicMode::from_base(registers.read_msr(APIC_BASE_MSR))?;
        Ok(Self { mode, registers })
    }

    fn read_register(&self, offset: usize) -> u32 {
        match self.mode {
            ApicMode::XApic(base) => self.registers.read_mmio(base.get() + offset),
            ApicMode::X2Apic => self.registers.read_msr(X2APIC_MSR_BASE + (offset >> 4) as u32) as u32,
        }
    }

    fn write_register(&self, offset: usize, value: u32) {
        match self.mode {
            ApicMode::XApic(base) => self.registers.write_mmio(base.get() + offset, value),
            ApicMode::X2Apic => self.registers.write_msr(X2APIC_MSR_BASE + (offset >> 4) as u32, u64::from(value)),
        }
    }

    fn update_register(&self, offset: usize, clear: u32, set: u32) {
        let value = self.read_register(offset);
        if self.mode == ApicMode::X2Apic {
            core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);
        }
        self.write_register(offset, (value & !clear) | set);
    }
}

impl<R: RegisterAccess> ApicBackend for LocalApic<R> {
    fn max_apic_id(&self) -> u32 {
        match self.mode {
            ApicMode::XApic(_) => 0xFE,
            ApicMode::X2Apic => u32::MAX - 1,
        }
    }

    fn current_apic_id(&self) -> u32 {
        match self.mode {
            ApicMode::XApic(_) => self.registers.cpuid(1, 0).ebx >> 24,
            ApicMode::X2Apic => self.registers.cpuid(0xB, 0).edx,
        }
    }

    fn send_icr(&self, apic_id: u32, command: u32) {
        match self.mode {
            ApicMode::XApic(_) => {
                while self.read_register(ICR_LOW_OFFSET) & ICR_DELIVERY_STATUS != 0 {
                    core::hint::spin_loop();
                }
                self.write_register(ICR_HIGH_OFFSET, apic_id << 24);
                self.write_register(ICR_LOW_OFFSET, command);
                while self.read_register(ICR_LOW_OFFSET) & ICR_DELIVERY_STATUS != 0 {
                    core::hint::spin_loop();
                }
            }
            ApicMode::X2Apic => {
                self.registers.write_msr(
                    X2APIC_MSR_BASE + (ICR_LOW_OFFSET >> 4) as u32,
                    (u64::from(apic_id) << 32) | u64::from(command),
                );
            }
        }
    }

    fn mask_local_interrupts(&self) {
        let lvt_count = (((self.read_register(VERSION_OFFSET) >> 16) & 0xFF) as usize + 1).min(LVT_OFFSETS.len());
        for &offset in LVT_OFFSETS.iter().take(lvt_count) {
            self.update_register(offset, 0, MASKED);
        }
    }

    fn initialize_virtual_wire_mode(&self) {
        self.update_register(SVR_OFFSET, 0xFF, bit!(8) | 0x0F);
        let lint_mask = bit!(8) | bit!(9) | bit!(10) | bit!(13) | bit!(15) | MASKED;
        self.update_register(LINT0_OFFSET, lint_mask, bit!(8) | bit!(9) | bit!(10));
        self.update_register(LINT1_OFFSET, lint_mask, bit!(10));
    }
}

pub(crate) struct Apic(ApicImplementation);

enum ApicImplementation {
    Native(LocalApic<HardwareAccess>),
    #[cfg(test)]
    Mock(MockApicBackend),
}

impl Apic {
    #[cfg_attr(coverage, coverage(off))]
    pub(crate) fn current() -> Result<Self, EfiError> {
        LocalApic::detect(HardwareAccess)
            .map(|apic| Self(ApicImplementation::Native(apic)))
            .inspect_err(|error| log::error!("Local APIC is unavailable: {error:?}"))
    }

    #[cfg(test)]
    pub(crate) fn mock(mock: MockApicBackend) -> Self {
        Self(ApicImplementation::Mock(mock))
    }

    fn backend(&self) -> &dyn ApicBackend {
        match &self.0 {
            ApicImplementation::Native(apic) => apic,
            #[cfg(test)]
            ApicImplementation::Mock(apic) => apic,
        }
    }

    pub(crate) fn max_apic_id(&self) -> u32 {
        self.backend().max_apic_id()
    }

    pub(crate) fn current_apic_id(&self) -> u32 {
        self.backend().current_apic_id()
    }

    pub(crate) fn send_init(&self, apic_id: u32) {
        self.backend().send_icr(apic_id, ICR_DELIVERY_MODE_INIT | ICR_LEVEL_ASSERT);
    }

    pub(crate) fn send_startup(&self, apic_id: u32, startup_vector: u8) {
        self.backend().send_icr(apic_id, ICR_DELIVERY_MODE_STARTUP | ICR_LEVEL_ASSERT | u32::from(startup_vector));
    }

    pub(crate) fn mask_local_interrupts(&self) {
        self.backend().mask_local_interrupts();
    }

    pub(crate) fn initialize_virtual_wire_mode(&self) {
        self.backend().initialize_virtual_wire_mode();
    }
}

#[cfg(test)]
#[cfg_attr(coverage, coverage(off))]
mod tests {
    use super::*;
    use mockall::{Sequence, predicate::eq};

    const BASE: usize = 0xFEE0_0000;

    fn cpuid_result(ebx: u32, edx: u32) -> CpuidResult {
        CpuidResult { eax: 0, ebx, ecx: 0, edx }
    }

    fn controller(mode: ApicMode, registers: MockRegisterAccess) -> LocalApic<MockRegisterAccess> {
        LocalApic { mode, registers }
    }

    fn xapic_mode() -> ApicMode {
        ApicMode::XApic(NonZeroUsize::new(BASE).unwrap())
    }

    fn expect_update(
        registers: &mut MockRegisterAccess,
        sequence: &mut Sequence,
        mode: ApicMode,
        offset: usize,
        old: u32,
        expected: u32,
    ) {
        match mode {
            ApicMode::XApic(base) => {
                registers
                    .expect_read_mmio()
                    .with(eq(base.get() + offset))
                    .once()
                    .in_sequence(sequence)
                    .return_const(old);
                registers
                    .expect_write_mmio()
                    .with(eq(base.get() + offset), eq(expected))
                    .once()
                    .in_sequence(sequence)
                    .return_const(());
            }
            ApicMode::X2Apic => {
                let msr = 0x800 + (offset >> 4) as u32;
                registers.expect_read_msr().with(eq(msr)).once().in_sequence(sequence).return_const(u64::from(old));
                registers
                    .expect_write_msr()
                    .with(eq(msr), eq(u64::from(expected)))
                    .once()
                    .in_sequence(sequence)
                    .return_const(());
            }
        }
    }

    #[test]
    fn test_apic_detect_rejects_missing_capabilities_without_reading_msrs() {
        for features in [0, bit!(5), bit!(9)] {
            let mut registers = MockRegisterAccess::new();
            registers.expect_cpuid().with(eq(1), eq(0)).once().returning(move |_, _| cpuid_result(0, features));
            registers.expect_read_msr().never();
            assert_eq!(LocalApic::detect(registers).err(), Some(EfiError::Unsupported));
        }
    }

    #[test]
    fn test_apic_detect_validates_enabled_mode_and_base() {
        for (base, expected) in [
            (0, None),
            (BASE as u64, None),
            (X2APIC_ENABLE, None),
            (APIC_ENABLE, None),
            (BASE as u64 | APIC_ENABLE | bit!(8) | bit!(60), Some(xapic_mode())),
            (APIC_ENABLE | X2APIC_ENABLE, Some(ApicMode::X2Apic)),
        ] {
            let mut registers = MockRegisterAccess::new();
            let mut sequence = Sequence::new();
            registers
                .expect_cpuid()
                .with(eq(1), eq(0))
                .once()
                .in_sequence(&mut sequence)
                .returning(|_, _| cpuid_result(0, bit!(5) | bit!(9)));
            registers.expect_read_msr().with(eq(0x1B)).once().in_sequence(&mut sequence).return_const(base);
            let result = LocalApic::detect(registers);
            match expected {
                Some(mode) => assert_eq!(result.unwrap().mode, mode),
                None => assert_eq!(result.err(), Some(EfiError::Unsupported)),
            }
        }
    }

    #[test]
    fn test_apic_identity_and_id_limits_preserve_mode_specific_cpuid() {
        for (mode, leaf, ebx, edx, id, limit) in [
            (xapic_mode(), 1, 0x5A00_0000, 0, 0x5A, 0xFE),
            (ApicMode::X2Apic, 0xB, 0, 0x1234_5678, 0x1234_5678, u32::MAX - 1),
        ] {
            let mut registers = MockRegisterAccess::new();
            registers.expect_cpuid().with(eq(leaf), eq(0)).once().returning(move |_, _| cpuid_result(ebx, edx));
            let apic = controller(mode, registers);
            assert_eq!(apic.max_apic_id(), limit);
            assert_eq!(apic.current_apic_id(), id);
        }
    }

    #[test]
    fn test_apic_virtual_wire_updates_preserve_unrelated_register_bits() {
        for mode in [xapic_mode(), ApicMode::X2Apic] {
            let mut registers = MockRegisterAccess::new();
            let mut sequence = Sequence::new();
            expect_update(&mut registers, &mut sequence, mode, 0xF0, 0xABCD_12F0, 0xABCD_130F);
            expect_update(&mut registers, &mut sequence, mode, 0x350, 0x0003_FF55, 0x0002_5F55);
            expect_update(&mut registers, &mut sequence, mode, 0x360, 0x0001_A700, 0x400);
            controller(mode, registers).initialize_virtual_wire_mode();
        }
    }

    #[test]
    fn test_apic_masks_only_reported_lvt_entries_and_clamps_to_supported_registers() {
        for mode in [xapic_mode(), ApicMode::X2Apic] {
            for count in [1_usize, 4, 6, 256] {
                let mut registers = MockRegisterAccess::new();
                let mut sequence = Sequence::new();
                let version = ((count - 1) as u32) << 16 | 0x14;
                match mode {
                    ApicMode::XApic(base) => {
                        registers
                            .expect_read_mmio()
                            .with(eq(base.get() + 0x30))
                            .once()
                            .in_sequence(&mut sequence)
                            .return_const(version);
                    }
                    ApicMode::X2Apic => {
                        registers
                            .expect_read_msr()
                            .with(eq(0x803))
                            .once()
                            .in_sequence(&mut sequence)
                            .return_const(u64::from(version));
                    }
                }
                for offset in [0x320, 0x330, 0x340, 0x350, 0x360, 0x370].into_iter().take(count.min(6)) {
                    let old = offset as u32 | 0x1000;
                    expect_update(&mut registers, &mut sequence, mode, offset, old, old | 0x10000);
                }
                controller(mode, registers).mask_local_interrupts();
            }
        }
    }

    #[test]
    fn test_apic_xapic_icr_polls_delivery_and_writes_destination_before_command() {
        let mut registers = MockRegisterAccess::new();
        let mut sequence = Sequence::new();
        for status in [0x1000_u32, 0] {
            registers.expect_read_mmio().with(eq(BASE + 0x300)).once().in_sequence(&mut sequence).return_const(status);
        }
        registers
            .expect_write_mmio()
            .with(eq(BASE + 0x310), eq(0x5A00_0000))
            .once()
            .in_sequence(&mut sequence)
            .return_const(());
        registers
            .expect_write_mmio()
            .with(eq(BASE + 0x300), eq(0x4608))
            .once()
            .in_sequence(&mut sequence)
            .return_const(());
        for status in [0x1000_u32, 0] {
            registers.expect_read_mmio().with(eq(BASE + 0x300)).once().in_sequence(&mut sequence).return_const(status);
        }
        controller(xapic_mode(), registers).send_icr(0x5A, 0x4608);
    }

    #[test]
    fn test_apic_x2apic_icr_writes_full_destination_and_command_atomically() {
        let mut registers = MockRegisterAccess::new();
        registers.expect_write_msr().with(eq(0x830), eq(0x1234_5678_0000_4608)).once().return_const(());
        controller(ApicMode::X2Apic, registers).send_icr(0x1234_5678, 0x4608);
    }

    #[test]
    fn test_apic_dispatches_shared_operations_and_encodes_init_and_startup() {
        let mut backend = MockApicBackend::new();
        let mut sequence = Sequence::new();
        backend.expect_max_apic_id().once().in_sequence(&mut sequence).return_const(0xFE_u32);
        backend.expect_current_apic_id().once().in_sequence(&mut sequence).return_const(0x5A_u32);
        backend.expect_initialize_virtual_wire_mode().once().in_sequence(&mut sequence).return_const(());
        backend.expect_mask_local_interrupts().once().in_sequence(&mut sequence).return_const(());
        backend.expect_send_icr().with(eq(0x5A), eq(0x4500)).once().in_sequence(&mut sequence).return_const(());
        backend.expect_send_icr().with(eq(0x5A), eq(0x4608)).once().in_sequence(&mut sequence).return_const(());
        let apic = Apic::mock(backend);
        assert_eq!(apic.max_apic_id(), 0xFE);
        assert_eq!(apic.current_apic_id(), 0x5A);
        apic.initialize_virtual_wire_mode();
        apic.mask_local_interrupts();
        apic.send_init(0x5A);
        apic.send_startup(0x5A, 8);
    }
}
