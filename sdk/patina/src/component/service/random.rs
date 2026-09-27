//! Random-number generation service interface.
//!
//! ## License
//!
//! Copyright (c) Microsoft Corporation.
//!
//! SPDX-License-Identifier: Apache-2.0
//!

use core::fmt;

#[cfg(any(test, feature = "mockall"))]
use mockall::automock;

/// A random-number generation algorithm exposed by the firmware.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RandomAlgorithm {
    /// NIST SP 800-90A AES-CTR with a 256-bit key, as provided by x86 `RDRAND`.
    Sp80090Ctr256,
    /// The implementation-defined NIST SP 800-90A compliant algorithm provided by `AArch64` `RNDR`.
    UnknownNist,
}

/// An error returned by a [`Random`] service.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RandomError {
    /// The requested algorithm is not supported by this service.
    UnsupportedAlgorithm,
    /// The random source did not produce data within its bounded retry limit.
    NotReady,
}

impl fmt::Display for RandomError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedAlgorithm => write!(formatter, "random algorithm is not supported"),
            Self::NotReady => write!(formatter, "random source is not ready"),
        }
    }
}

impl core::error::Error for RandomError {}

/// Provides random bytes using a platform random-number generator.
#[cfg_attr(any(test, feature = "mockall"), automock)]
pub trait Random: Send + Sync {
    /// Returns the supported algorithms in preference order.
    ///
    /// The first entry is used by [`fill_bytes`](Self::fill_bytes).
    fn supported_algorithms(&self) -> &'static [RandomAlgorithm];

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

    /// Fills `output` using `algorithm`.
    fn fill_bytes_with_algorithm(&self, algorithm: RandomAlgorithm, output: &mut [u8]) -> Result<(), RandomError>;
}

#[cfg(test)]
#[cfg_attr(coverage, coverage(off))]
mod tests {
    use super::*;

    const TEST_ALGORITHMS: &[RandomAlgorithm] = &[RandomAlgorithm::UnknownNist];

    struct EmptyRandom;

    impl Random for EmptyRandom {
        fn supported_algorithms(&self) -> &'static [RandomAlgorithm] {
            &[]
        }

        fn fill_bytes_with_algorithm(
            &self,
            _algorithm: RandomAlgorithm,
            _output: &mut [u8],
        ) -> Result<(), RandomError> {
            unreachable!()
        }
    }

    struct SequentialRandom;

    impl Random for SequentialRandom {
        fn supported_algorithms(&self) -> &'static [RandomAlgorithm] {
            TEST_ALGORITHMS
        }

        fn fill_bytes_with_algorithm(&self, _algorithm: RandomAlgorithm, output: &mut [u8]) -> Result<(), RandomError> {
            output.iter_mut().enumerate().for_each(|(index, byte)| *byte = index as u8);
            Ok(())
        }
    }

    #[test]
    fn test_random_fill_bytes_rejects_service_without_algorithms() {
        assert_eq!(EmptyRandom.fill_bytes(&mut [0; 8]), Err(RandomError::UnsupportedAlgorithm));
    }

    #[test]
    fn test_random_get_integer_values_through_trait_object() {
        let random: &dyn Random = &SequentialRandom;

        assert_eq!(random.get_u16(), Ok(u16::from_ne_bytes([0, 1])));
        assert_eq!(random.get_u32(), Ok(u32::from_ne_bytes([0, 1, 2, 3])));
        assert_eq!(random.get_u64(), Ok(u64::from_ne_bytes([0, 1, 2, 3, 4, 5, 6, 7])));
        assert_eq!(random.get_usize(), Ok(usize::from_ne_bytes(core::array::from_fn(|index| index as u8))));
    }

    #[test]
    fn test_random_get_integer_propagates_error() {
        assert_eq!(EmptyRandom.get_u32(), Err(RandomError::UnsupportedAlgorithm));
    }
}
