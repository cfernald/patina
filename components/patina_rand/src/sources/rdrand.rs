use core::arch::{asm, x86_64::__cpuid};

use patina::{
    bit,
    component::service::random::{RandomAlgorithm, RandomError},
};

use crate::sources::RngSource;

pub(crate) struct RdRand {
    // Forces creation to go through the `new` method, ensuring the CPU supports RDRAND.
    _private: (),
}

impl RdRand {
    const CPU_ID_LEAF: u32 = 1;
    const CPU_ID_SUPPORT: u32 = bit!(30);

    /// Creates a new instance of `RdRand` if the CPU supports the RDRAND instruction and it passes the self-test.
    /// Returns `None` otherwise.
    pub fn new() -> Option<Self> {
        if !Self::is_supported() {
            return None;
        }

        let rdrand = Self { _private: () };
        if !rdrand.test_rdrand() {
            log::error!("RDRAND test failed");
            debug_assert!(false, "RDRAND test failed");
            return None;
        }

        Some(rdrand)
    }

    /// Checks if the CPU supports the RDRAND instruction.
    fn is_supported() -> bool {
        __cpuid(Self::CPU_ID_LEAF).ecx & Self::CPU_ID_SUPPORT != 0
    }

    /// A test of the RDRAND instruction to check for CPU errata.
    ///
    /// This is unexpected to fail on a modern CPU, but left in out of an abundance of caution and parity
    /// with EDKII.
    fn test_rdrand(&self) -> bool {
        const RETRY_LIMIT: usize = 10;
        const SAMPLES: usize = 10;
        const MIN_CHANGES: usize = 5;

        let mut previous = None;
        let mut changes = 0;

        for _ in 0..SAMPLES {
            let mut sample = None;
            for _ in 0..RETRY_LIMIT {
                if let Some(s) = self.rdrand() {
                    sample = Some(s);
                    break;
                }
            }

            let Some(sample) = sample else {
                return false;
            };

            if previous.is_some_and(|value| value != sample) {
                changes += 1;
            }

            previous = Some(sample);
        }

        changes >= MIN_CHANGES
    }

    /// Draws a random 64-bit word using the RDRAND instruction.
    pub fn rdrand(&self) -> Option<u64> {
        let value: u64;
        let success: u8;

        // SAFETY: This structure can only be created if the CPU supports RDRAND, as checked in `Self::new()`.
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

impl RngSource for RdRand {
    fn algorithm(&self) -> RandomAlgorithm {
        RandomAlgorithm::Sp80090Ctr256
    }

    fn generate(&self) -> Result<u64, RandomError> {
        self.rdrand().ok_or(RandomError::NotReady)
    }
}
