//! No-op `Architecture` implementation for unit tests.
//!
//! ## License
//!
//! Copyright (c) Microsoft Corporation.
//!
//! SPDX-License-Identifier: Apache-2.0
//!

/// No-op architecture used in unit tests.
pub(crate) struct NullArch;

impl super::ArchSupport for NullArch {}

impl super::Interrupts for NullArch {
    fn enable_interrupts() {}

    fn disable_interrupts() {}

    fn interrupts_enabled() -> bool {
        false
    }
}
