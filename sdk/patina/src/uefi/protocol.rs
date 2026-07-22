//! UEFI Specification Protocol Definitions
//!
//! Each protocol in the UEFI Specification (or supporting EDK II protocols) is maintained as a
//! separate module. Every module exposes the protocol's GUID as `PROTOCOL_GUID` and its C-ABI
//! interface as a descriptively named struct ending in `Protocol` (e.g. `DecompressProtocol`),
//! together with an `unsafe impl` of [`crate::base::protocol::ProtocolInterface`].
//!
//! ## License
//!
//! Copyright (c) Microsoft Corporation.
//!
//! SPDX-License-Identifier: Apache-2.0
//!

pub mod decompress;
pub mod performance_measurement;
pub mod status_code;
