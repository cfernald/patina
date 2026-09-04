#
# Exception handler for application processors.
#
# Copyright (c) Microsoft Corporation.
# SPDX-License-Identifier: Apache-2.0
#

.code64
.globl ap_exception_halt

ap_exception_halt:
    cli
.Lhalt:
    hlt
    jmp .Lhalt
