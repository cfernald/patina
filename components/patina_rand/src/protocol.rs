//! UEFI RNG protocol adapter.
//!
//! ## License
//!
//! Copyright (c) Microsoft Corporation.
//!
//! SPDX-License-Identifier: Apache-2.0
//!

use core::{mem, ptr, slice};

use patina::{
    BinaryGuid,
    component::service::{
        Service,
        random::{Random, RandomAlgorithm, RandomError, RandomExt},
    },
    protocol::ProtocolInterface,
    standard::efi::{self, protocols::rng},
};

const ARM_RNDR_ALGORITHM_GUID: efi::Guid =
    efi::Guid::from_fields(0x43d2fde3, 0x9d4e, 0x4d79, 0x02, 0x96, &[0xa8, 0x9b, 0xca, 0x78, 0x08, 0x41]);

const EDKII_RNG_ALGORITHM_UNSAFE_GUID: efi::Guid =
    efi::Guid::from_fields(0x869f728c, 0x409d, 0x4ab4, 0xac, 0x03, &[0x71, 0xd3, 0x09, 0xc1, 0xb3, 0xf4]);

#[repr(C)]
pub(crate) struct RngProtocol {
    protocol: rng::Protocol,
    random: Service<dyn Random>,
}

impl RngProtocol {
    pub(crate) fn new(random: Service<dyn Random>) -> Self {
        Self { protocol: rng::Protocol { get_info: get_info, get_rng: get_rng }, random }
    }
}

// SAFETY: `RngProtocol` is `repr(C)` and its first field is the canonical r-efi RNG protocol interface.
unsafe impl ProtocolInterface for RngProtocol {
    const PROTOCOL_GUID: BinaryGuid = BinaryGuid(rng::PROTOCOL_GUID);
}

fn algorithm_guid(algorithm: RandomAlgorithm) -> efi::Guid {
    match algorithm {
        RandomAlgorithm::Sp80090Ctr256 => rng::ALGORITHM_SP800_90_CTR_256_GUID,
        RandomAlgorithm::UnknownAArch64Rndr => ARM_RNDR_ALGORITHM_GUID,
        RandomAlgorithm::InsecurePseudoRandom => EDKII_RNG_ALGORITHM_UNSAFE_GUID,
    }
}

fn algorithm_from_guid(guid: efi::Guid) -> Option<RandomAlgorithm> {
    if guid == rng::ALGORITHM_SP800_90_CTR_256_GUID {
        Some(RandomAlgorithm::Sp80090Ctr256)
    } else if guid == ARM_RNDR_ALGORITHM_GUID {
        Some(RandomAlgorithm::UnknownAArch64Rndr)
    } else if guid == EDKII_RNG_ALGORITHM_UNSAFE_GUID {
        Some(RandomAlgorithm::InsecurePseudoRandom)
    } else {
        None
    }
}

unsafe fn get_service<'a>(this: *mut rng::Protocol) -> Option<Service<dyn Random>> {
    if this.is_null() {
        None
    } else {
        // SAFETY: `RngProtocol` is `repr(C)` with `protocol` at offset zero, and the installer passes that field.
        Some(unsafe { &*this.cast::<RngProtocol>() }.random)
    }
}

unsafe extern "efiapi" fn get_info(
    this: *mut rng::Protocol,
    algorithm_list_size: *mut usize,
    algorithm_list: *mut rng::Algorithm,
) -> efi::Status {
    if algorithm_list_size.is_null() {
        return efi::Status::INVALID_PARAMETER;
    }

    // SAFETY: UEFI supplies `this` as the installed first-field protocol pointer.
    let Some(rng) = (unsafe { get_service(this) }) else {
        return efi::Status::INVALID_PARAMETER;
    };

    let algorithms = rng.supported_algorithms();
    let Some(required_size) = algorithms.len().checked_mul(mem::size_of::<rng::Algorithm>()) else {
        return efi::Status::DEVICE_ERROR;
    };

    if required_size == 0 {
        return efi::Status::UNSUPPORTED;
    }

    // SAFETY: UEFI requires `algorithm_list_size` to point to a writable `usize`.
    let provided_size = unsafe { *algorithm_list_size };
    if provided_size < required_size {
        // SAFETY: The pointer validity requirement is established above.
        unsafe { *algorithm_list_size = required_size };
        return efi::Status::BUFFER_TOO_SMALL;
    }

    if algorithm_list.is_null() {
        return efi::Status::INVALID_PARAMETER;
    }

    for (index, algorithm) in algorithms.iter().copied().enumerate() {
        // SAFETY: The caller reports a buffer of at least `required_size`, which covers every indexed GUID.
        unsafe { algorithm_list.add(index).write(algorithm_guid(algorithm)) };
    }

    // SAFETY: The pointer validity requirement is established above.
    unsafe { *algorithm_list_size = required_size };
    efi::Status::SUCCESS
}

unsafe extern "efiapi" fn get_rng(
    this: *mut rng::Protocol,
    algorithm: *mut rng::Algorithm,
    output_size: usize,
    output: *mut u8,
) -> efi::Status {
    if output_size == 0 || output.is_null() {
        return efi::Status::INVALID_PARAMETER;
    }

    // SAFETY: UEFI supplies `this` as the installed first-field protocol pointer.
    let Some(rng) = (unsafe { get_service(this) }) else {
        return efi::Status::INVALID_PARAMETER;
    };

    // SAFETY: UEFI requires `output` to reference `output_size` writable bytes.
    let output = unsafe { slice::from_raw_parts_mut(output, output_size) };
    let result = if algorithm.is_null() {
        rng.fill_bytes(output)
    } else {
        // SAFETY: A non-null algorithm pointer supplied by UEFI points to a readable GUID.
        let guid = unsafe { ptr::read_unaligned(algorithm) };
        let Some(algorithm) = algorithm_from_guid(guid) else {
            return efi::Status::UNSUPPORTED;
        };
        rng.fill_bytes_with_algorithm(algorithm, output)
    };

    match result {
        Ok(()) => efi::Status::SUCCESS,
        Err(error) => error.into(),
    }
}

// #[cfg(test)]
// #[cfg_attr(coverage, coverage(off))]
// mod tests {
//     use super::*;
//     use patina::component::service::random::MockRandom;

//     fn mock_service() -> Service<dyn Random> {
//         Service::mock(Box::new(MockRandom::new()) as Box<dyn Random>)
//     }

//     #[test]
//     fn test_random_protocol_reports_required_algorithm_buffer_size() {
//         let mock = MockRandom::new();
//         let service = Service::mock(mock);
//         let mut wrapper = RngProtocol::new(MockRandom);
//         let mut size = 0;

//         // SAFETY: All pointers refer to live values for the duration of the call.
//         let status = unsafe {
//             (wrapper.protocol.get_info)(ptr::from_mut(&mut wrapper.protocol), ptr::from_mut(&mut size), ptr::null_mut())
//         };

//         assert_eq!(status, efi::Status::BUFFER_TOO_SMALL);
//         assert_eq!(size, mem::size_of::<rng::Algorithm>());
//     }

//     #[test]
//     fn test_random_protocol_returns_algorithm_and_random_bytes() {
//         let mut wrapper = RngProtocol::new(FakeRandom);
//         let mut size = mem::size_of::<rng::Algorithm>();
//         let mut algorithm = efi::Guid::from_fields(0, 0, 0, 0, 0, &[0; 6]);
//         let mut output = [0; 7];

//         // SAFETY: All pointers refer to live values with the sizes supplied to the calls.
//         let info_status = unsafe {
//             (wrapper.protocol.get_info)(
//                 ptr::from_mut(&mut wrapper.protocol),
//                 ptr::from_mut(&mut size),
//                 ptr::from_mut(&mut algorithm),
//             )
//         };
//         // SAFETY: All pointers refer to live values with the sizes supplied to the calls.
//         let rng_status = unsafe {
//             (wrapper.protocol.get_rng)(
//                 ptr::from_mut(&mut wrapper.protocol),
//                 ptr::from_mut(&mut algorithm),
//                 output.len(),
//                 output.as_mut_ptr(),
//             )
//         };

//         assert_eq!(info_status, efi::Status::SUCCESS);
//         assert_eq!(algorithm, rng::ALGORITHM_SP800_90_CTR_256_GUID);
//         assert_eq!(rng_status, efi::Status::SUCCESS);
//         assert_eq!(output, [0xa5; 7]);
//     }

//     #[test]
//     fn test_random_protocol_rejects_unknown_algorithm() {
//         let mut wrapper = RngProtocol::new(FakeRandom);
//         let mut algorithm = efi::Guid::from_fields(1, 2, 3, 4, 5, &[6; 6]);
//         let mut output = [0; 1];

//         // SAFETY: All pointers refer to live values with the sizes supplied to the call.
//         let status = unsafe {
//             (wrapper.protocol.get_rng)(
//                 ptr::from_mut(&mut wrapper.protocol),
//                 ptr::from_mut(&mut algorithm),
//                 output.len(),
//                 output.as_mut_ptr(),
//             )
//         };

//         assert_eq!(status, efi::Status::UNSUPPORTED);
//     }
// }
