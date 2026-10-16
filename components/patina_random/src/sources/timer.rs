//! Performance time & counter based pseudo-random number generator source.
//!
//! ## License
//!
//! Copyright (c) Microsoft Corporation.
//!
//! SPDX-License-Identifier: Apache-2.0
//!

use core::hash::{Hash, Hasher};

use core::sync::atomic::AtomicU64;

use patina::{
    component::service::{
        perf_timer::ArchTimerFunctionality,
        random::{RandomAlgorithm, RandomError},
    },
    hash::Xorshift64starHasher,
};

use crate::sources::RngSource;

pub(crate) struct TimerRngSource<'a> {
    timer: &'a dyn ArchTimerFunctionality,
    counter: AtomicU64,
}

impl<'a> TimerRngSource<'a> {
    pub fn new(timer: &'a dyn ArchTimerFunctionality) -> Self {
        Self { timer, counter: AtomicU64::new(0) }
    }
}

impl<'a> RngSource for TimerRngSource<'a> {
    fn algorithm(&self) -> RandomAlgorithm {
        RandomAlgorithm::InsecurePseudoRandom
    }

    fn generate(&self) -> Result<u64, RandomError> {
        // To avoid returning the same timer value twice, keep a counter as well. This will ensure
        // a different value is used each time regardless of the timer resolution.
        let tick = self.timer.cpu_count();
        let counter = self.counter.fetch_add(1, core::sync::atomic::Ordering::Relaxed);

        let mut hasher = Xorshift64starHasher::default();
        tick.wrapping_add(counter).hash(&mut hasher);
        Ok(hasher.finish())
    }
}

#[cfg(test)]
#[cfg_attr(coverage, coverage(off))]
mod tests {
    use patina::component::service::perf_timer::MockArchTimerFunctionality;

    use super::*;

    #[test]
    fn test_timer_rng() {
        const ITERATIONS: usize = 10;
        let mut values = vec![0u64; ITERATIONS];

        let mut timer: MockArchTimerFunctionality = MockArchTimerFunctionality::new();
        for index in 0..ITERATIONS {
            timer.expect_cpu_count().returning(move || index as u64 / 2);
        }

        let rng = TimerRngSource::new(&timer);
        assert!(rng.algorithm() == RandomAlgorithm::InsecurePseudoRandom);
        for index in 0..ITERATIONS {
            values[index] = rng.generate().unwrap();
        }

        values.sort_unstable();
        values.dedup();
        assert!(values.len() == ITERATIONS);
    }
}
