# Patina Random Number Generator

`patina_rand` provides the Patina random-number service and publishes the UEFI RNG protocol. It uses the processor's
architectural random-number instruction: `RDRAND` on `x86_64` and `RNDR` on `AArch64`.

## License

Copyright (c) Microsoft Corporation.

SPDX-License-Identifier: Apache-2.0
