//! Random-number generator component.
//!
//! ## License
//!
//! Copyright (c) Microsoft Corporation.
//!
//! SPDX-License-Identifier: Apache-2.0
//!

use alloc::{boxed::Box, vec::Vec};

use patina::{
    component::{
        Storage, component,
        service::{
            IntoService, Service,
            perf_timer::ArchTimerFunctionality,
            random::{Random, RandomAlgorithm, RandomError},
        },
    },
    error::EfiError,
};

use crate::sources::{self, RngSource};

/// Provides architecture-backed random numbers as a Rust service and the UEFI RNG protocol.
#[derive(IntoService)]
#[service(dyn Random)]
pub struct RandomNumberGenerator {
    algorithms: Vec<RandomAlgorithm>,
    sources: Vec<Box<dyn RngSource>>,
}

#[component]
impl RandomNumberGenerator {
    /// Creates a random-number generator component.
    pub const fn new() -> Self {
        Self { algorithms: Vec::new(), sources: Vec::new() }
    }

    fn add_source(&mut self, source: Box<dyn RngSource>) {
        self.algorithms.push(source.algorithm());
        self.sources.push(source);
    }

    fn entry_point(
        mut self,
        storage: &mut Storage,
        timer: Option<Service<dyn ArchTimerFunctionality>>,
    ) -> patina::error::Result<()> {
        if let Some(arch_rng) = crate::sources::get_architecture_rng_source() {
            self.add_source(arch_rng);
        }

        if let Some(timer) = timer {
            let timer_rng = sources::timer::TimerRngSource::new(*timer);
            self.add_source(Box::new(timer_rng));
        }

        if self.sources.is_empty() {
            log::error!("No RNG sources available.");
            return Err(EfiError::NotFound);
        }

        storage.add_service(self);
        Ok(())
    }

    fn get_rng(&self, algorithm: RandomAlgorithm) -> Result<&dyn RngSource, RandomError> {
        self.sources
            .iter()
            .find(|source| source.algorithm() == algorithm)
            .map(core::convert::AsRef::as_ref)
            .ok_or(RandomError::UnsupportedAlgorithm)
    }
}

impl Default for RandomNumberGenerator {
    fn default() -> Self {
        Self::new()
    }
}

impl Random for RandomNumberGenerator {
    fn supported_algorithms(&self) -> &[RandomAlgorithm] {
        self.algorithms.as_slice()
    }

    fn fill_bytes_with_algorithm(
        &self,
        algorithm: RandomAlgorithm,
        output: &mut [u8],
    ) -> core::result::Result<(), RandomError> {
        let rng = self.get_rng(algorithm)?;
        for chunk in output.chunks_mut(8) {
            let word = rng.generate()?;
            let bytes = word.to_ne_bytes();
            chunk.copy_from_slice(&bytes);
        }
        Ok(())
    }
}
