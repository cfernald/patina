//! Arch Specific abstractions for Patina.
//!
//! ## License
//!
//! Copyright (c) Microsoft Corporation.
//!
//! SPDX-License-Identifier: Apache-2.0
//!

cfg_if::cfg_if! {
    if #[cfg(test)] {
        #[coverage(off)]
        mod null;
        type Arch = null::NullArch;
    } else if #[cfg(target_arch = "aarch64")] {
        #[coverage(off)] // Architecture code cannot be unit tested.
        pub mod aarch64;
        type Arch = aarch64::AArch64;
    } else if #[cfg(target_arch = "x86_64")] {
        #[coverage(off)] // Architecture code cannot be unit tested.
        pub mod x64;
        type Arch = x64::X64;
    }
}

#[allow(unused)]
/// A trait for architecture-specific functionality. It inherits from subtraits
/// for code organization.
trait ArchSupport: Interrupts {}

//
// Interrupts support.
//
// Only `with_interrupts_disabled` is being exposed publically for now to
// avoid misuse of direct interrupt manipulation.
//

trait Interrupts {
    fn enable_interrupts();
    fn disable_interrupts();
    fn interrupts_enabled() -> bool;
}

/// Enables CPU interrupts on the current architecture.
fn enable_interrupts() {
    <Arch as Interrupts>::enable_interrupts();
}

/// Disables CPU interrupts on the current architecture.
fn disable_interrupts() {
    <Arch as Interrupts>::disable_interrupts();
}

/// Returns whether CPU interrupts are currently enabled on the current architecture.
fn interrupts_enabled() -> bool {
    <Arch as Interrupts>::interrupts_enabled()
}

/// Executes a closure with CPU interrupts disabled, interrupts will be restored to their
/// previous state after the closure is executed.
///
/// ## Important
///
/// This function is only intended for low-level operations, and should not be used in
/// components. Components should use the TPL (Task Priority Level) to manage interrupts.
pub(crate) fn with_interrupts_disabled<F, R>(f: F) -> R
where
    F: FnOnce() -> R,
{
    let enabled = interrupts_enabled();
    if enabled {
        disable_interrupts();
    }
    let result = f();
    if enabled {
        enable_interrupts();
    }
    result
}
