//! Arch-specific timer functionality
//! By default, this module attempts to determine the timer frequency via architecture specific methods.
//! (cpuid for x86, `CNTFRQ_EL0` for aarch64)
//!
//! Platforms can override this with a custom performance frequency by providing the Core with the correct frequency:
//!
//! <!-- (The below test has to be ignore because `patna` cannot depend on `patina_dxe_core` - circular dependency.) -->
//! ```rust,ignore
//!     let frequency_hz: u64 = 1_000_000_000; // Compute with platform-specific methods.
//!
//!     Core::default()
//!        .init_timer_frequency(Some(frequency_hz))
//!```
//!
//! ## License
//!
//! Copyright (c) Microsoft Corporation.
//!
//! SPDX-License-Identifier: Apache-2.0
//!

#[cfg(any(test, feature = "mockall"))]
use mockall::automock;

/// Trait that provides architecture-specific timer functionality.
/// Components that need timing functionality can request this service.
#[cfg_attr(any(test, feature = "mockall"), automock)]
pub trait ArchTimerFunctionality: Send + Sync {
    /// Value of the counter (ticks).
    fn cpu_count(&self) -> u64;

    /// Value in Hz of how often the counter increment.
    fn perf_frequency(&self) -> u64;

    /// Value that the performance counter starts with.
    fn cpu_count_start(&self) -> u64 {
        0
    }

    /// Value that the performance counter ends with before it rolls over.
    fn cpu_count_end(&self) -> u64 {
        u64::MAX
    }
}

/// Extension methods for [`ArchTimerFunctionality`].
pub trait ArchTimerFunctionalityExt: ArchTimerFunctionality {
    /// Creates and starts a stopwatch instance.
    ///
    /// Returns `None` if the timer frequency is zero.
    fn start_stopwatch(&self) -> Option<Stopwatch<'_, Self>> {
        Stopwatch::new(self)
    }
}

impl<T: ArchTimerFunctionality + ?Sized> ArchTimerFunctionalityExt for T {}

pub struct Stopwatch<'a, T: ArchTimerFunctionality + ?Sized> {
    timer: &'a T,
    frequency: u64,
    start_count: u64,
}

impl<'a, T: ArchTimerFunctionality + ?Sized> Stopwatch<'a, T> {
    /// Creates and starts a new stopwatch instance.
    ///
    /// Returns `None` if the timer frequency is zero.
    pub fn new(timer: &'a T) -> Option<Self> {
        let frequency = timer.perf_frequency();
        if frequency == 0 {
            return None;
        }

        let start_count = timer.cpu_count();
        Some(Self { timer, frequency, start_count })
    }

    /// Restarts the stopwatch by updating the start count to the current counter value.
    pub fn restart(&mut self) {
        self.start_count = self.timer.cpu_count();
    }

    /// Returns the elapsed time in microseconds since the stopwatch was started or last restarted.
    pub fn elapsed_microseconds(&mut self) -> u64 {
        let current = self.timer.cpu_count();
        let elapsed_ticks = current - self.start_count;
        (elapsed_ticks * 1_000_000) / self.frequency
    }

    /// Returns the elapsed time in milliseconds since the stopwatch was started or last restarted.
    pub fn elapsed_milliseconds(&mut self) -> u64 {
        let current = self.timer.cpu_count();
        let elapsed_ticks = current - self.start_count;
        (elapsed_ticks * 1_000) / self.frequency
    }

    /// Returns the elapsed time in seconds since the stopwatch was started or last restarted.
    pub fn elapsed_seconds(&mut self) -> u64 {
        let current = self.timer.cpu_count();
        let elapsed_ticks = current - self.start_count;
        elapsed_ticks / self.frequency
    }
}

#[cfg(test)]
#[cfg_attr(coverage, coverage(off))]
mod tests {
    use super::*;
    use mockall::Sequence;

    fn mock_timer(frequency: u64, counts: &[u64]) -> MockArchTimerFunctionality {
        let mut timer = MockArchTimerFunctionality::new();
        timer.expect_perf_frequency().once().return_const(frequency);

        let mut sequence = Sequence::new();
        for &count in counts {
            timer.expect_cpu_count().once().in_sequence(&mut sequence).return_const(count);
        }

        timer
    }

    #[test]
    fn test_arch_timer_rejects_zero_frequency() {
        let timer = mock_timer(0, &[]);

        assert!(timer.start_stopwatch().is_none());
    }

    #[test]
    fn test_arch_timer_reports_elapsed_time() {
        let timer = mock_timer(1_000_000, &[100, 1_500_100, 1_500_100, 1_500_100]);
        let mut stopwatch = timer.start_stopwatch().expect("timer frequency is nonzero");

        assert_eq!(stopwatch.elapsed_microseconds(), 1_500_000);
        assert_eq!(stopwatch.elapsed_milliseconds(), 1_500);
        assert_eq!(stopwatch.elapsed_seconds(), 1);
    }

    #[test]
    fn test_arch_timer_restart_resets_start_count() {
        let timer = mock_timer(1, &[10, 20, 25]);
        let mut stopwatch = timer.start_stopwatch().expect("timer frequency is nonzero");

        stopwatch.restart();

        assert_eq!(stopwatch.elapsed_seconds(), 5);
    }

    #[test]
    fn test_arch_timer_supports_trait_objects() {
        let timer = mock_timer(1_000, &[100, 1_100]);
        let dyn_timer: &dyn ArchTimerFunctionality = &timer;
        let mut stopwatch = dyn_timer.start_stopwatch().expect("timer frequency is nonzero");

        assert_eq!(stopwatch.elapsed_milliseconds(), 1_000);
    }
}
