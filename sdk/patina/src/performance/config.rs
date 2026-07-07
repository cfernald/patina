//! Performance measurement configuration.
//!
//! This module defines the configuration consumed by the performance measurement infrastructure. The configuration is
//! produced as a guided HOB prior to Patina DXE Core execution and read by the core to determine whether performance
//! measurement is enabled and which measurements to collect.
//!
//! ## License
//!
//! Copyright (c) Microsoft Corporation.
//!
//! SPDX-License-Identifier: Apache-2.0
//!
use crate::{BinaryGuid, component::hob::FromHob};

/// The configuration for performance measurement.
#[derive(Debug, Clone, Copy, zerocopy_derive::FromBytes)]
#[repr(C, packed)]
pub struct PerformanceConfig {
    /// Indicates whether performance measurement is enabled.
    pub enabled: u8,
    /// Bitmask of enabled measurements (see [`crate::performance::Measurement`]).
    pub enabled_measurements: u32,
}

impl PerformanceConfig {
    /// Constant value indicating that performance measurement is enabled.
    pub const ENABLED: u8 = 1;
    /// Constant value indicating that performance measurement is disabled.
    pub const DISABLED: u8 = 0;
    /// Constant value indicating that no performance measurements are enabled.
    pub const NO_MEASUREMENTS: u32 = 0;

    /// Creates a new `PerformanceConfig` that is disabled with no measurements.
    pub const fn new() -> Self {
        Self { enabled: Self::DISABLED, enabled_measurements: Self::NO_MEASUREMENTS }
    }
}

impl Default for PerformanceConfig {
    fn default() -> Self {
        Self::new()
    }
}

impl FromHob for PerformanceConfig {
    const HOB_GUID: BinaryGuid = BinaryGuid::from_string("fd87f2d8-112d-4640-9c00-d37d2a1fb75d");

    fn parse(bytes: &[u8]) -> Self {
        match <Self as zerocopy::FromBytes>::read_from_prefix(bytes) {
            Ok((config, _)) => config,
            Err(_) => panic!(
                "Guided Hob [{:#?}] parse failed. Buffer too small for type {}",
                Self::HOB_GUID,
                core::any::type_name::<Self>()
            ),
        }
    }
}
