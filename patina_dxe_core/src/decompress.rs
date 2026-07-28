//! A module for core UEFI decompression functionality.
//!
//! ## License
//!
//! Copyright (c) Microsoft Corporation.
//!
//! SPDX-License-Identifier: Apache-2.0
//!
extern crate alloc;

use core::ffi::c_void;

use alloc::boxed::Box;
use patina::{
    base::error::EfiError,
    component::{Storage, component},
    log_debug_assert,
    standard::efi::{self, protocols::decompress},
    uefi::{
        boot_services::BootServices,
        decompress::{DecompressionAlgorithm, decompress_into_with_algo},
    },
};

/// Component to install the UEFI Decompress Protocol.
#[derive(Default)]
pub(crate) struct DecompressProtocolInstaller;

#[component]
impl DecompressProtocolInstaller {
    fn entry_point(self, storage: &mut Storage) -> patina::base::error::Result<()> {
        let protocol = Box::new(decompress::Protocol { get_info, decompress });

        match storage.boot_services().install_protocol_interface(None, protocol) {
            Ok(_) => Ok(()),
            Err(err) => EfiError::status_to_result(err),
        }
    }
}

unsafe extern "efiapi" fn get_info(
    _: *mut decompress::Protocol,
    src: *mut c_void,
    src_size: u32,
    dst_size: *mut u32,
    scratch_size: *mut u32,
) -> efi::Status {
    if src.is_null() | dst_size.is_null() | scratch_size.is_null() {
        return efi::Status::INVALID_PARAMETER;
    }

    if src_size < 8 {
        return efi::Status::INVALID_PARAMETER;
    }

    // SAFETY: The data the pointer points to is at least 8 bytes long, as checked above.
    let compressed_size = unsafe { src.cast::<u32>().read_unaligned() };

    if (src_size < compressed_size + 8) || compressed_size.checked_add(8).is_none() {
        return efi::Status::INVALID_PARAMETER;
    }

    // SAFETY: The pointers are not null, as checked above.
    //         The data the pointer points to is at least 8 bytes long, as checked above.
    unsafe { dst_size.write_volatile(src.cast::<u32>().add(1).read_unaligned()) };

    // We do not need any scratch space for the rust implementation.
    // SAFETY: The pointer is not null, as checked above.
    unsafe { scratch_size.cast::<u32>().write_volatile(0) };

    efi::Status::SUCCESS
}

/// FFI interface to decompress data and return it.
unsafe extern "efiapi" fn decompress(
    _: *mut decompress::Protocol,
    source_buffer: *mut c_void,
    source_size: u32,
    destination_buffer: *mut c_void,
    destination_size: u32,
    _scratch_buffer: *mut c_void,
    _scratch_size: u32,
) -> efi::Status {
    if source_buffer.is_null() || destination_buffer.is_null() {
        log_debug_assert!("DecompressProtocol::decompress called with null pointer");
        return efi::Status::INVALID_PARAMETER;
    }

    // SAFETY: source_buffer and destination_buffer pointers are validated as non-null.
    // Sizes are provided by caller and trusted to match the buffer allocations.
    let src = unsafe { core::slice::from_raw_parts(source_buffer as *const u8, source_size as usize) };
    // SAFETY: destination_buffer is validated as non-null and mutable access is exclusive.
    let dst = unsafe { core::slice::from_raw_parts_mut(destination_buffer as *mut u8, destination_size as usize) };

    match decompress_into_with_algo(src, dst, DecompressionAlgorithm::UefiDecompress) {
        Ok(()) => efi::Status::SUCCESS,
        Err(_) => efi::Status::INVALID_PARAMETER,
    }
}
