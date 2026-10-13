//! Random-number generation service interface.
//!
//! ## License
//!
//! Copyright (c) Microsoft Corporation.
//!
//! SPDX-License-Identifier: Apache-2.0
//!

use crate::standard::efi;
use core::mem::size_of;

#[cfg(any(test, feature = "mockall"))]
use mockall::automock;

/// A random-number generation algorithm exposed by the firmware.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RandomAlgorithm {
    /// NIST SP 800-90A AES-CTR with a 256-bit key.
    ///
    /// This algorithm is used by the x86 `RDRAND` instruction.
    Sp80090Ctr256,
    /// The implementation-defined "market requirement appropriate" algorithm provided by `AArch64` `RNDR`.
    ///
    /// Reference: [Properties of the generated random number.](https://support.arm.com/documentation/ddi0487/md/-Part-K-Appendixes/-Appendix-K12-Random-Number-Generation/-K12-1-Properties-of-the-generated-random-number)
    UnknownAArch64Rndr,
    /// An insecure, non-cryptographic pseudo-random number generator.
    InsecurePseudoRandom,
}

impl RandomAlgorithm {
    /// Returns `true` if the algorithm is considered insecure and should not be used for cryptographic
    /// or other security critical purposes.
    pub fn is_insecure(&self) -> bool {
        match self {
            RandomAlgorithm::Sp80090Ctr256 | RandomAlgorithm::UnknownAArch64Rndr => false,
            RandomAlgorithm::InsecurePseudoRandom => true,
        }
    }
}

/// An error returned by a [`Random`] service.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RandomError {
    /// The requested algorithm is not supported by this service.
    UnsupportedAlgorithm,
    /// The random source did not produce data within its bounded retry limit.
    NotReady,
}

impl From<RandomError> for efi::Status {
    fn from(error: RandomError) -> Self {
        match error {
            RandomError::UnsupportedAlgorithm => efi::Status::UNSUPPORTED,
            RandomError::NotReady => efi::Status::NOT_READY,
        }
    }
}

/// Provides random bytes using a platform random-number generator.
#[cfg_attr(any(test, feature = "mockall"), automock)]
pub trait Random: Send + Sync {
    /// Returns the supported algorithms in preference order.
    ///
    /// The first entry is used by [`fill_bytes`](Self::fill_bytes).
    fn supported_algorithms(&self) -> &[RandomAlgorithm];

    /// Fills `output` using `algorithm`.
    fn fill_bytes_with_algorithm(&self, algorithm: RandomAlgorithm, output: &mut [u8]) -> Result<(), RandomError>;
}

/// Extension trait for [`Random`] providing convenience methods.
pub trait RandomExt: Random {
    /// Fills `output` using the service's preferred algorithm.
    fn fill_bytes(&self, output: &mut [u8]) -> Result<(), RandomError> {
        let algorithm = self.supported_algorithms().first().copied().ok_or(RandomError::UnsupportedAlgorithm)?;
        self.fill_bytes_with_algorithm(algorithm, output)
    }

    /// Returns a random `u16` using the service's preferred algorithm.
    fn get_u16(&self) -> Result<u16, RandomError> {
        let mut bytes = [0; size_of::<u16>()];
        self.fill_bytes(&mut bytes)?;
        Ok(u16::from_ne_bytes(bytes))
    }

    /// Returns a random `u32` using the service's preferred algorithm.
    fn get_u32(&self) -> Result<u32, RandomError> {
        let mut bytes = [0; size_of::<u32>()];
        self.fill_bytes(&mut bytes)?;
        Ok(u32::from_ne_bytes(bytes))
    }

    /// Returns a random `u64` using the service's preferred algorithm.
    fn get_u64(&self) -> Result<u64, RandomError> {
        let mut bytes = [0; size_of::<u64>()];
        self.fill_bytes(&mut bytes)?;
        Ok(u64::from_ne_bytes(bytes))
    }

    /// Returns a random `usize` using the service's preferred algorithm.
    fn get_usize(&self) -> Result<usize, RandomError> {
        let mut bytes = [0; size_of::<usize>()];
        self.fill_bytes(&mut bytes)?;
        Ok(usize::from_ne_bytes(bytes))
    }
}

impl<T: Random + ?Sized> RandomExt for T {}
