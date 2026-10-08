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
        RandomAlgorithm::UnsafePseudoRandom
    }

    fn generate(&self) -> Result<u64, RandomError> {
        // To avoid returning the same timer value twice, keep a counter as well. This will ensure
        // a different value is used each time regardless of the timer's resolution.
        let tick = self.timer.cpu_count();
        let counter = self.counter.fetch_add(1, core::sync::atomic::Ordering::Relaxed);

        let mut hasher = Xorshift64starHasher::default();
        tick.wrapping_add(counter).hash(&mut hasher);
        Ok(hasher.finish())
    }
}
