//! Architecture-specific random-number generation.
//!
//! ## License
//!
//! Copyright (c) Microsoft Corporation.
//!
//! SPDX-License-Identifier: Apache-2.0
//!

use patina::component::service::{
    IntoService,
    random::{Random, RandomAlgorithm, RandomError},
};

const RETRY_LIMIT: usize = 10;

trait WordSource {
    fn next_word(&self) -> Option<u64>;
}

fn fill_from_source(source: &impl WordSource, output: &mut [u8]) -> Result<(), RandomError> {
    for chunk in output.chunks_mut(size_of::<u64>()) {
        let bytes = source.next_word().ok_or(RandomError::NotReady)?.to_ne_bytes();
        for (destination, value) in chunk.iter_mut().zip(bytes) {
            *destination = value;
        }
    }
    Ok(())
}

fn retry_word(mut draw: impl FnMut() -> Option<u64>) -> Option<u64> {
    (0..RETRY_LIMIT).find_map(|_| draw())
}

/// Random-number generator backed by the current architecture's random instruction.
#[derive(Clone, Copy, IntoService)]
#[service(dyn Random)]
pub(crate) struct ArchitectureRandom {
    _private: (),
}

impl ArchitectureRandom {
    pub(crate) fn new() -> Option<Self> {
        arch::is_supported().then_some(Self { _private: () })
    }
}

impl WordSource for ArchitectureRandom {
    fn next_word(&self) -> Option<u64> {
        retry_word(arch::draw_word)
    }
}

impl Random for ArchitectureRandom {
    fn supported_algorithms(&self) -> &'static [RandomAlgorithm] {
        &[arch::ALGORITHM]
    }

    fn fill_bytes_with_algorithm(&self, algorithm: RandomAlgorithm, output: &mut [u8]) -> Result<(), RandomError> {
        if algorithm != arch::ALGORITHM {
            return Err(RandomError::UnsupportedAlgorithm);
        }
        fill_from_source(self, output)
    }
}

#[cfg(target_arch = "x86_64")]
mod arch {
    use core::arch::{asm, x86_64::__cpuid};

    use patina::component::service::random::RandomAlgorithm;

    pub(super) const ALGORITHM: RandomAlgorithm = RandomAlgorithm::Sp80090Ctr256;
    const RDRAND_BIT: u32 = 1 << 30;
    const TEST_SAMPLES: usize = 8;
    const MIN_CHANGES: usize = 5;

    pub(super) fn is_supported() -> bool {
        let has_rdrand = __cpuid(1).ecx & RDRAND_BIT != 0;
        has_rdrand && test_rdrand()
    }

    fn test_rdrand() -> bool {
        let mut previous = None;
        let mut changes = 0;

        for _ in 0..TEST_SAMPLES {
            let Some(sample) = super::retry_word(draw_word) else {
                return false;
            };
            if previous.is_some_and(|value| value != sample) {
                changes += 1;
            }
            previous = Some(sample);
        }

        changes >= MIN_CHANGES
    }

    pub(super) fn draw_word() -> Option<u64> {
        let value: u64;
        let success: u8;

        // SAFETY: `ArchitectureRandom::new` checks CPUID before any instance can call this function.
        unsafe {
            asm!(
                "rdrand {value}",
                "setc {success}",
                value = out(reg) value,
                success = out(reg_byte) success,
                options(nomem, nostack),
            );
        }

        (success != 0).then_some(value)
    }
}

#[cfg(target_arch = "aarch64")]
mod arch {
    use core::arch::asm;

    use patina::component::service::random::RandomAlgorithm;

    pub(super) const ALGORITHM: RandomAlgorithm = RandomAlgorithm::UnknownNist;
    const RNDR_SHIFT: u64 = 60;
    const RNDR_MASK: u64 = 0xf;

    pub(super) fn is_supported() -> bool {
        let features: u64;

        // SAFETY: Reading ID_AA64ISAR0_EL1 is permitted at the UEFI execution level.
        unsafe {
            asm!("mrs {features}, ID_AA64ISAR0_EL1", features = out(reg) features, options(nomem, nostack));
        }

        (features >> RNDR_SHIFT) & RNDR_MASK != 0
    }

    pub(super) fn draw_word() -> Option<u64> {
        let value: u64;
        let success: u64;

        // SAFETY: `ArchitectureRandom::new` checks ID_AA64ISAR0_EL1 before any instance can call this function.
        unsafe {
            asm!(
                "mrs {value}, S3_3_C2_C4_0",
                "cset {success}, ne",
                value = out(reg) value,
                success = out(reg) success,
                options(nomem, nostack),
            );
        }

        (success != 0).then_some(value)
    }
}

#[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
mod arch {
    use patina::component::service::random::RandomAlgorithm;

    pub(super) const ALGORITHM: RandomAlgorithm = RandomAlgorithm::Sp80090Ctr256;

    pub(super) fn is_supported() -> bool {
        false
    }

    pub(super) fn draw_word() -> Option<u64> {
        None
    }
}

#[cfg(test)]
#[cfg_attr(coverage, coverage(off))]
mod tests {
    use core::cell::Cell;

    use super::*;

    struct CountingSource {
        next: Cell<u64>,
    }

    impl WordSource for CountingSource {
        fn next_word(&self) -> Option<u64> {
            let value = self.next.get();
            self.next.set(value + 1);
            Some(value)
        }
    }

    #[test]
    fn test_random_fills_complete_words_and_tail() {
        let source = CountingSource { next: Cell::new(1) };
        let mut output = [0; 10];

        fill_from_source(&source, &mut output).unwrap();

        assert_eq!(output, [1, 0, 0, 0, 0, 0, 0, 0, 2, 0]);
    }

    #[test]
    fn test_random_retries_until_a_word_is_available() {
        let attempts = Cell::new(0);
        let result = retry_word(|| {
            attempts.set(attempts.get() + 1);
            (attempts.get() == RETRY_LIMIT).then_some(42)
        });

        assert_eq!(result, Some(42));
        assert_eq!(attempts.get(), RETRY_LIMIT);
    }

    #[test]
    fn test_random_stops_after_retry_limit() {
        let attempts = Cell::new(0);
        let result = retry_word(|| {
            attempts.set(attempts.get() + 1);
            None
        });

        assert_eq!(result, None);
        assert_eq!(attempts.get(), RETRY_LIMIT);
    }
}
