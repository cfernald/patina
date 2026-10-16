#![doc = include_str!("../README.md")]
#![cfg_attr(all(not(feature = "std"), not(test), not(feature = "mockall")), no_std)]
#![deny(missing_docs)]
#![cfg_attr(coverage, feature(coverage_attribute))]

extern crate alloc;

mod component;
mod protocol;
mod sources;
mod test;

pub use component::RandomNumberGenerator;
pub use component::RngProtocolPublisher;
