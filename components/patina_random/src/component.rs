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
            uefi_services::protocol::{ProtocolServices, ProtocolServicesExt},
        },
    },
    error::EfiError,
};

use crate::{
    protocol::RngProtocol,
    sources::{self, RngSource},
};

/// Provides architecture-backed random numbers as a Rust service and the UEFI RNG protocol.
#[derive(IntoService)]
#[service(dyn Random)]
pub struct RandomNumberGenerator {
    algorithms: Vec<RandomAlgorithm>,
    sources: Vec<Box<dyn RngSource>>,
    allow_insecure: bool,
}

#[component]
impl RandomNumberGenerator {
    /// Creates a random-number generator component.
    pub const fn new() -> Self {
        Self { algorithms: Vec::new(), sources: Vec::new(), allow_insecure: false }
    }

    /// Enables the use of insecure pseudo-random number generator sources.
    pub const fn with_allow_insecure_algorithms(mut self) -> Self {
        self.allow_insecure = true;
        self
    }

    /// Adds a random-number generator source to the component.
    ///
    /// This could be made public in the future to allow for platform extensions, but is being
    /// left private until there is a clear need for non-standard sources.
    fn add_source(&mut self, source: Box<dyn RngSource>) {
        debug_assert!(self.allow_insecure || !source.algorithm().is_insecure());
        self.algorithms.push(source.algorithm());
        self.sources.push(source);
    }

    fn entry_point(
        mut self,
        storage: &mut Storage,
        timer: Option<Service<dyn ArchTimerFunctionality>>,
    ) -> patina::error::Result<()> {
        // Prioritize architecture-backed RNG sources first.
        if let Some(arch_rng) = crate::sources::get_cpu_rng_source() {
            self.add_source(arch_rng);
        } else {
            log::warn!("No CPU-backed RNG source found. Only timer-based RNG will be available.");
        }

        // Add a timer-based RNG source as a fallback if allowed.
        if self.allow_insecure
            && let Some(timer) = timer
        {
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
        let rng: &dyn RngSource = self.get_rng(algorithm)?;
        for chunk in output.chunks_mut(8) {
            let rand_bytes = rng.generate()?.to_ne_bytes();
            chunk.copy_from_slice(&rand_bytes[..chunk.len()]);
        }
        Ok(())
    }
}

/// Component responsible for installing the RNG protocol using the [`Random`] service.
///
/// This component takes the [`Random`] service as a dependency and will install the corresponding RNG protocol
/// wrapping its functionality. This component takes no dependency on the specific [`Random`] trait implementation
/// provided.
pub struct RngProtocolPublisher;

#[component]
impl RngProtocolPublisher {
    fn entry_point(
        self,
        rng: Service<dyn Random>,
        protocols: Service<dyn ProtocolServices>,
    ) -> patina::error::Result<()> {
        let rng_protocol = Box::new(RngProtocol::new(rng));
        protocols.install_protocol::<RngProtocol>(None, rng_protocol)?;
        Ok(())
    }
}
