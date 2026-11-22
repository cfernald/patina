// Copyright (c) Microsoft Corporation.
// SPDX-License-Identifier: Apache-2.0

.text
.p2align 4
.globl patina_arm_ap_start
patina_arm_ap_start:
    msr daifset, #0xf
    msr spsel, #1
    mov x19, x0
    mrs x1, CurrentEL
    ldr x2, [x19, #{el}]
    cmp x1, #8
    b.ne .Lcheck_el
    cmp x2, #4
    b.ne .Lcheck_el

    // PSCI may enter at EL2 even when the BSP executes at EL1.
    mov x3, #0x80000000
    msr hcr_el2, x3
    mrs x3, cptr_el2
    bic x3, x3, #0x400
    msr cptr_el2, x3
    mov x3, #3
    msr cnthctl_el2, x3
    msr cntvoff_el2, xzr
    adr x3, .Lentry_el1
    msr elr_el2, x3
    mov x3, #0x3c5
    msr spsr_el2, x3
    eret

.Lentry_el1:
    mrs x1, CurrentEL
.Lcheck_el:
    cmp x1, x2
    b.ne .Lbad_el
    adr x3, .Lap_vectors
    cmp x1, #8
    b.eq .Lsetup_el2

    msr vbar_el1, x3
    isb
    ldr x3, [x19, #{tcr}]
    msr tcr_el1, x3
    ldr x3, [x19, #{ttbr0}]
    msr ttbr0_el1, x3
    ldr x3, [x19, #{ttbr1}]
    msr ttbr1_el1, x3
    ldr x3, [x19, #{mair}]
    msr mair_el1, x3
    ldr x3, [x19, #{cpacr}]
    msr cpacr_el1, x3
    isb
    tlbi vmalle1
    dsb sy
    isb
    ic iallu
    dsb sy
    isb
    ldr x3, [x19, #{sctlr}]
    msr sctlr_el1, x3
    isb
    b .Lrust_entry

.Lsetup_el2:
    msr vbar_el2, x3
    isb
    ldr x3, [x19, #{hcr}]
    msr hcr_el2, x3
    isb
    ldr x3, [x19, #{tcr}]
    msr tcr_el2, x3
    ldr x3, [x19, #{ttbr0}]
    msr ttbr0_el2, x3
    ldr x3, [x19, #{mair}]
    msr mair_el2, x3
    ldr x3, [x19, #{cptr}]
    msr cptr_el2, x3
    ldr x3, [x19, #{cpacr}]
    msr cpacr_el1, x3
    isb
    tlbi alle2
    dsb sy
    isb
    ic iallu
    dsb sy
    isb
    ldr x3, [x19, #{sctlr}]
    msr sctlr_el2, x3
    isb

.Lrust_entry:
    ldr x3, [x19, #{stack}]
    mov sp, x3
    mov x29, xzr
    mov x0, x19
    b {main}

.Lbad_el:
    mov x0, #4
    b patina_arm_ap_fail

.p2align 11
.Lap_vectors:
    .rept 16
    mov x0, #1
    b patina_arm_ap_fail
    .balign 128
    .endr

.globl patina_arm_ap_fail
patina_arm_ap_fail:
    msr daifset, #0xf
    mrs x1, mpidr_el1
    mrs x2, CurrentEL
    cmp x2, #8
    b.eq .Lfailure_el2
    mrs x2, esr_el1
    mrs x3, far_el1
    b .Lrecord_failure
.Lfailure_el2:
    mrs x2, esr_el2
    mrs x3, far_el2
.Lrecord_failure:
    adrp x4, {failure}
    add x4, x4, :lo12:{failure}
.Lclaim_failure:
    ldaxr x5, [x4]
    cbnz x5, .Lhalt
    mov x5, #1
    stlxr w6, x5, [x4]
    cbnz w6, .Lclaim_failure
    str x0, [x4, #8]
    str x1, [x4, #16]
    str x2, [x4, #24]
    str x3, [x4, #32]
    mov x5, #2
    stlr x5, [x4]
.Lhalt:
    wfe
    b .Lhalt

.globl patina_arm_ap_end
patina_arm_ap_end:
