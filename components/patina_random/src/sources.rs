use alloc::boxed::Box;
use patina::component::service::random::{RandomAlgorithm, RandomError};

#[cfg(target_arch = "x86_64")]
mod rdrand;
#[cfg(target_arch = "aarch64")]
mod rndr;
pub(crate) mod timer;

pub(crate) trait RngSource: Send + Sync {
    fn algorithm(&self) -> RandomAlgorithm;
    fn generate(&self) -> Result<u64, RandomError>;
}

#[cfg_attr(coverage, coverage(off))]
pub(crate) fn get_cpu_rng_source() -> Option<Box<dyn RngSource>> {
    #[cfg(target_arch = "x86_64")]
    if let Some(rdrand) = rdrand::RdRand::new() {
        return Some(Box::new(rdrand) as Box<dyn RngSource>);
    }
    #[cfg(target_arch = "aarch64")]
    if let Some(rndr) = rndr::Rndr::new() {
        return Some(Box::new(rndr) as Box<dyn RngSource>);
    }

    None
}
