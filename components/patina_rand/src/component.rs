//! Random-number generator component.
//!
//! ## License
//!
//! Copyright (c) Microsoft Corporation.
//!
//! SPDX-License-Identifier: Apache-2.0
//!

use alloc::boxed::Box;

use patina::{
    component::service::{IntoService, random::Random},
    component::{Storage, component},
    error::{EfiError, Result},
    uefi::boot_services::BootServices,
};

use crate::{architecture::ArchitectureRandom, protocol::RngProtocol};

/// Provides architecture-backed random numbers as a Rust service and the UEFI RNG protocol.
#[derive(Default)]
pub struct RandomNumberGenerator;

#[component]
impl RandomNumberGenerator {
    /// Creates a random-number generator component.
    pub const fn new() -> Self {
        Self
    }

    fn entry_point(self, storage: &mut Storage) -> Result<()> {
        let random = ArchitectureRandom::new().ok_or(EfiError::Unsupported)?;
        let boot_services = storage.boot_services().clone();
        Self::install(storage, &boot_services, random)
    }

    fn install<B, R>(storage: &mut Storage, boot_services: &B, random: R) -> Result<()>
    where
        B: BootServices,
        R: Random + IntoService + Copy + 'static,
    {
        boot_services.install_protocol_interface(None, Box::new(RngProtocol::new(random)))?;
        storage.add_service(random);
        Ok(())
    }
}

#[cfg(test)]
#[cfg_attr(coverage, coverage(off))]
mod tests {
    use super::*;
    use patina::{
        c_ptr::CPtr,
        component::service::{
            IntoService,
            random::{RandomAlgorithm, RandomError},
        },
        uefi::boot_services::MockBootServices,
    };

    #[derive(Clone, Copy, IntoService)]
    #[service(dyn Random)]
    struct FakeRandom;

    impl Random for FakeRandom {
        fn supported_algorithms(&self) -> &'static [RandomAlgorithm] {
            &[RandomAlgorithm::Sp80090Ctr256]
        }

        fn fill_bytes_with_algorithm(
            &self,
            _algorithm: RandomAlgorithm,
            output: &mut [u8],
        ) -> core::result::Result<(), RandomError> {
            output.fill(0x5a);
            Ok(())
        }
    }

    #[test]
    fn test_random_number_generator_installs_protocol_and_registers_service() {
        let mut boot_services = MockBootServices::new();
        boot_services
            .expect_install_protocol_interface::<RngProtocol<FakeRandom>, Box<_>>()
            .once()
            .returning(|_, protocol| Ok((core::ptr::dangling_mut::<core::ffi::c_void>(), protocol.metadata())));
        let mut storage = Storage::new();

        RandomNumberGenerator::install(&mut storage, &boot_services, FakeRandom).unwrap();

        let service = storage.get_service::<dyn Random>().expect("random service should be registered");
        let mut output = [0; 4];
        service.fill_bytes(&mut output).unwrap();
        assert_eq!(output, [0x5a; 4]);
    }
}
