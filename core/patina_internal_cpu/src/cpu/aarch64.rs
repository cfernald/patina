//! AArch64 CPU module
//!
//! ## License
//!
//! Copyright (c) Microsoft Corporation.
//!
//! SPDX-License-Identifier: Apache-2.0
//!
mod cache;
mod cpu;

pub(crate) use cache::flush_data_cache_range;
#[allow(unused)]
pub use cpu::EfiCpuAarch64;
