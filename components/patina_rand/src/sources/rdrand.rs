use core::arch::{asm, x86_64::__cpuid};

use patina::component::service::random::RandomAlgorithm;

pub(super) const ALGORITHM: RandomAlgorithm = RandomAlgorithm::Sp80090Ctr256;
const RDRAND_BIT: u32 = 1 << 30;
const TEST_SAMPLES: usize = 8;
const MIN_CHANGES: usize = 5;

struct RdRand;

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
