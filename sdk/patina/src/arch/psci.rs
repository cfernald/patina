//! Arm Power State Coordination Interface (PSCI) calls.
//!
//! The conduit must be supplied by the platform; calling an unsupported SMC or HVC
//! conduit can raise an exception rather than return a PSCI error.
//!
//! ## License
//!
//! Copyright (c) Microsoft Corporation.
//!
//! SPDX-License-Identifier: Apache-2.0
//!

use core::{convert::Infallible, fmt, num::NonZeroU64};

const VERSION: u32 = 0x8400_0000;
const CPU_OFF: u32 = 0x8400_0002;
const CPU_ON: u32 = 0xC400_0003;
const AFFINITY_INFO: u32 = 0xC400_0004;

/// MPIDR affinity fields accepted by PSCI, including Aff3 in bits 39:32.
pub const MPIDR_AFFINITY_MASK: u64 = 0x0000_00FF_00FF_FFFF;

/// Platform-selected transport for PSCI calls.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Conduit {
    /// Secure Monitor Call.
    Smc,
    /// Hypervisor Call.
    Hvc,
}

/// Power state reported by PSCI for a single processor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AffinityState {
    /// The processor is on.
    On,
    /// The processor is off and can be started with `CPU_ON`.
    Off,
    /// A preceding `CPU_ON` is still in progress.
    OnPending,
}

/// PSCI failure returned by the platform firmware.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    /// The requested operation is unsupported.
    NotSupported,
    /// A parameter is invalid.
    InvalidParameters,
    /// The operation was denied.
    Denied,
    /// The target processor is already on.
    AlreadyOn,
    /// The target processor is already being powered on.
    OnPending,
    /// The platform encountered an internal failure.
    InternalFailure,
    /// The target is not present.
    NotPresent,
    /// The target is disabled.
    Disabled,
    /// The entry-point address is invalid.
    InvalidAddress,
    /// Firmware returned an unexpected response, including a returning successful `CPU_OFF`.
    UnexpectedResponse(i32),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "PSCI {self:?}")
    }
}

impl core::error::Error for Error {}

fn response_error(value: i32) -> Error {
    match value {
        -1 => Error::NotSupported,
        -2 => Error::InvalidParameters,
        -3 => Error::Denied,
        -4 => Error::AlreadyOn,
        -5 => Error::OnPending,
        -6 => Error::InternalFailure,
        -7 => Error::NotPresent,
        -8 => Error::Disabled,
        -9 => Error::InvalidAddress,
        value => Error::UnexpectedResponse(value),
    }
}

fn status(value: u64) -> Result<(), Error> {
    match (value as u32).cast_signed() {
        0 => Ok(()),
        value => Err(response_error(value)),
    }
}

fn version(value: u64) -> Result<u32, Error> {
    let value = value as u32;
    if value.cast_signed() < 0 {
        status(u64::from(value))?;
    }
    Ok(value)
}

fn affinity_state(value: u64) -> Result<AffinityState, Error> {
    match value as u32 {
        0 => Ok(AffinityState::On),
        1 => Ok(AffinityState::Off),
        2 => Ok(AffinityState::OnPending),
        value => Err(response_error(value.cast_signed())),
    }
}

impl Conduit {
    /// Returns the PSCI version, with the major version in bits 31:16 and minor in bits 15:0.
    ///
    /// Requires PSCI 0.2 or newer. Host builds return [`Error::NotSupported`].
    pub fn version(self) -> Result<u32, Error> {
        version(self.call(VERSION, 0, 0, 0)?)
    }

    /// Starts an off processor at a physical `AArch64` entry point.
    ///
    /// `context_id` is delivered in X0 at the entry point. Success acknowledges
    /// the request, not completion of the processor's initialization.
    ///
    /// # Safety
    ///
    /// The caller must exclusively own the target processor and provide an entry
    /// point valid in the PSCI secondary-entry environment. Code, context, and all
    /// referenced memory must be physically accessible and remain alive until the
    /// processor has stopped using them. Cache maintenance must publish that memory
    /// before this call.
    pub unsafe fn cpu_on(self, target: u64, entry_point: NonZeroU64, context_id: u64) -> Result<(), Error> {
        status(self.call(CPU_ON, target, entry_point.get(), context_id)?)
    }

    /// Powers off the calling processor. Successful calls do not return.
    ///
    /// # Safety
    ///
    /// The caller must own the processor and ensure abandoning its execution
    /// cannot leave locks held or violate the lifetimes of shared data.
    pub unsafe fn cpu_off(self) -> Result<Infallible, Error> {
        let value = self.call(CPU_OFF, 0, 0, 0)?;
        status(value)?;
        Err(Error::UnexpectedResponse(0))
    }

    /// Queries one processor's power state using its MPIDR affinity fields.
    pub fn affinity_info(self, target: u64) -> Result<AffinityState, Error> {
        affinity_state(self.call(AFFINITY_INFO, target, 0, 0)?)
    }

    #[cfg_attr(coverage, coverage(off))]
    fn call(self, function: u32, argument1: u64, argument2: u64, argument3: u64) -> Result<u64, Error> {
        #[cfg(all(target_os = "uefi", target_arch = "aarch64"))]
        {
            let mut result = u64::from(function);
            // SAFETY: The platform selects a valid conduit. Only PSCI operations
            // with their documented register arguments reach this function.
            // CPU lifetime contracts are enforced by the unsafe public methods.
            unsafe {
                match self {
                    Self::Smc => core::arch::asm!(
                        "smc #0",
                        inout("x0") result,
                        inlateout("x1") argument1 => _,
                        inlateout("x2") argument2 => _,
                        inlateout("x3") argument3 => _,
                        clobber_abi("C"),
                        options(nostack),
                    ),
                    Self::Hvc => core::arch::asm!(
                        "hvc #0",
                        inout("x0") result,
                        inlateout("x1") argument1 => _,
                        inlateout("x2") argument2 => _,
                        inlateout("x3") argument3 => _,
                        clobber_abi("C"),
                        options(nostack),
                    ),
                }
            }
            Ok(result)
        }
        #[cfg(not(all(target_os = "uefi", target_arch = "aarch64")))]
        {
            let _ = (self, function, argument1, argument2, argument3);
            Err(Error::NotSupported)
        }
    }
}

#[cfg(test)]
#[cfg_attr(coverage, coverage(off))]
mod tests {
    use super::*;

    #[test]
    fn test_psci_status_decodes_all_standard_errors() {
        let errors = [
            Error::NotSupported,
            Error::InvalidParameters,
            Error::Denied,
            Error::AlreadyOn,
            Error::OnPending,
            Error::InternalFailure,
            Error::NotPresent,
            Error::Disabled,
            Error::InvalidAddress,
        ];
        assert_eq!(status(0), Ok(()));
        for (index, error) in errors.into_iter().enumerate() {
            let value = -i32::try_from(index + 1).unwrap();
            assert_eq!(status(u64::from(value.cast_unsigned())), Err(error));
            assert_eq!(status(i64::from(value).cast_unsigned()), Err(error));
        }
        assert_eq!(status(42), Err(Error::UnexpectedResponse(42)));
    }

    #[test]
    fn test_psci_version_preserves_major_and_minor() {
        assert_eq!(version(2), Ok(2));
        assert_eq!(version(0x0001_0003), Ok(0x0001_0003));
        assert_eq!(version(u64::from(u32::MAX)), Err(Error::NotSupported));
    }

    #[test]
    fn test_psci_affinity_state_distinguishes_pending_startup() {
        assert_eq!(affinity_state(0), Ok(AffinityState::On));
        assert_eq!(affinity_state(1), Ok(AffinityState::Off));
        assert_eq!(affinity_state(2), Ok(AffinityState::OnPending));
        assert_eq!(affinity_state(0xFFFF_FFFF_0000_0002), Ok(AffinityState::OnPending));
        assert_eq!(affinity_state(u64::MAX), Err(Error::NotSupported));
        assert_eq!(affinity_state(3), Err(Error::UnexpectedResponse(3)));
    }

    #[test]
    fn test_psci_host_calls_are_unsupported() {
        for conduit in [Conduit::Smc, Conduit::Hvc] {
            assert_eq!(conduit.version(), Err(Error::NotSupported));
            assert_eq!(conduit.affinity_info(0), Err(Error::NotSupported));
            // SAFETY: Host calls return Unsupported without accessing hardware.
            unsafe {
                assert_eq!(conduit.cpu_on(1, NonZeroU64::new(4).unwrap(), 0), Err(Error::NotSupported));
                assert_eq!(conduit.cpu_off(), Err(Error::NotSupported));
            }
        }
    }
}
