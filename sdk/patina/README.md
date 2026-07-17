# Patina

The Patina crate provides shared primitives used throughout the Patina project and serves as a "Software Development
Kit (SDK)" for other Patina code.

The crate implements foundational elements used throughout Patina such as the dependency-injected component model,
typed access to interfaces like the UEFI Boot and Runtime Services, Platform Initialization (PI) Specification
content, protocol helpers, logging, performance measurement, and the Patina on-platform testing infrastructure. The
crate builds in `no_std` environments by default, can be paired with either firmware or host tooling, and configured
with optional features.

## Getting started

Add the crate to your manifest and enable the features needed by your firmware or host tooling target.

```toml
[dependencies]
patina = { version = "X.X.X", default-features = false }
```

The crate is `no_std` unless `std` is selected. Tests or host utilities can enable `std` or `mockall` as needed.

## Modules

The SDK is organized into the following modules.

| Module | Description |
|--------|-------------|
| **arch** | Abstractions for architecture specific functionality (e.g. caching) and architecture specific functions. |
| **base** | Basic definitions and utilities: errors, GUIDs (and GUID constants), hashing, C-pointer helpers, size constants, and macro utilities. |
| **component** | Component and service definitions. |
| **debug** | Macros and definitions for logging and diagnostics. |
| **management_mode** | Definitions for management mode interactions and implementations. |
| **mmio** | Re-export of the `safe-mmio` crate for memory-mapped I/O access. |
| **performance** | TODO: will also move, but in another refactor |
| **peripheral** | Abstractions and implementations for core device operations. |
| **pi** | Platform Initialization (PI) specification definitions and wrappers. |
| **uefi** | UEFI specification definitions and wrappers. |

## Feature Overview

| Feature | Purpose |
|---------|---------|
| `core` | Expose dispatcher-facing types such as `Storage` (enables `alloc`). |
| `alloc` | Allow allocation APIs when targeting `no_std` firmware environments with a custom allocator. |
| `std` | Link the standard library. For example, when building host utilities. |
| `mockall` | Provide mock implementations for Boot Services and other traits (implies `std`). |
| `global_allocator` | Install the global allocator support used by Patina firmware images. |
| `serde` | Enable serialization support for configuration and PI data structures. |
| `unstable` | Opt into experimental APIs gated behind `unstable-*` flags, including device path helpers. |
| `unstable-device-path` | Activate the current device-path parsing and construction prototypes. |

## Additional resources

- [Patina background](https://opendevicepartnership.github.io/patina/patina.html) for project context and design goals.
- [Component getting started guide](https://opendevicepartnership.github.io/patina/component/getting_started.html) for a
    walkthrough that builds on the `component` module.
- The `examples` directory contains host-run samples such as
    [`basic_hob_usage.rs`](https://github.com/OpenDevicePartnership/patina/blob/main/sdk/patina/examples/basic_hob_usage.rs)
    that demonstrate HOB parsing.
