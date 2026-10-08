//! AArch64 RNDR-backed random-number source.
//!
//! ## License
//!
//! Copyright (c) Microsoft Corporation.
//!
//! SPDX-License-Identifier: Apache-2.0
//!

use core::arch::asm;

use patina::{
    component::service::random::{RandomAlgorithm, RandomError},
    read_sysreg,
};

use crate::sources::RngSource;

pub(crate) struct Rndr {
    // Forces creation to go through `new`, ensuring the CPU supports RNDR.
    _private: (),
}

impl Rndr {
    const RNDR_SHIFT: u32 = 60;
    const RNDR_MASK: u64 = 0xF;
    const RNDR_IMPLEMENTED: u64 = 1;

    /// Creates a new instance if the CPU supports RNDR and it passes the self-test.
    pub fn new() -> Option<Self> {
        Self::is_supported().then_some(Self { _private: () })
    }

    fn is_supported() -> bool {
        let features = read_sysreg!(ID_AA64ISAR0_EL1);
        (features >> Self::RNDR_SHIFT) & Self::RNDR_MASK == Self::RNDR_IMPLEMENTED
    }

    fn rndr(&self) -> Option<u64> {
        let value: u64;
        let success: u64;

        // SAFETY: Construction verifies FEAT_RNG before executing RNDR.
        unsafe {
            asm!(
                "mrs {value}, RNDR",
                "cset {success}, ne",
                value = out(reg) value,
                success = out(reg) success,
                options(nomem, nostack),
            );
        }

        (success != 0).then_some(value)
    }
}

impl RngSource for Rndr {
    fn algorithm(&self) -> RandomAlgorithm {
        RandomAlgorithm::UnknownNistRndr
    }

    fn generate(&self) -> Result<u64, RandomError> {
        self.rndr().ok_or(RandomError::NotReady)
    }
}
