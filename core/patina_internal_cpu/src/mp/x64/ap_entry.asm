#
# 64-bit AP entry point.
#
# This assembly stub is intended to take a long-mode enabled AP from another execution
# context (PEI handoff) and bring it into the Patina environment. To do this, it does the
# the fllowing:
#   1. Reads the APIC ID (xAPIC or x2APIC)
#   2. Searches ApContext[] (AP_CONTEXTS) for the matching APIC ID to determine
#      this processor's context
#   3. Loads the per-processor stack from ApContext[n].stack_top
#   4. Switches to the BSP's GDT (AP_BSP_GDTR) for selector consistency
#   5. Calls the AP entry point (`ap_entry`, passed as the {ap_entry} operand)
#      with the ApContext pointer in RCX (Microsoft x64 ABI). The entry point does not return.
#
# Copyright (c) Microsoft Corporation.
# SPDX-License-Identifier: Apache-2.0
#

.code64
.globl ap_entry_64

ap_entry_64:
    # APs are not expecting to handle interrupts, and the EFI ABI requires DF clear.
    cli
    cld

    # Check if x2APIC is enabled via IA32_APIC_BASE MSR (0x1B), bit 10.
    mov ecx, 0x1B
    rdmsr
    test eax, (1 << 10)
    jnz x2apic_id

    # xAPIC mode
    mov eax, 1
    cpuid
    shr ebx, 24
    mov eax, ebx
    jmp got_apic_id

x2apic_id:
    # x2APIC mode.
    mov eax, 0xB
    xor ecx, ecx
    cpuid
    mov eax, edx

got_apic_id:
    # EAX = APIC ID. Save in EDX.
    mov edx, eax

    # Load AP_CONTEXTS pointer and AP_CONTEXT_COUNT.
    mov rsi, qword ptr [rip + AP_CONTEXTS]
    mov r8d, dword ptr [rip + AP_CONTEXT_COUNT]

    # Search ApContext for our APIC ID.
    xor ecx, ecx

    # ECX = index
    # EDX = APIC ID
    # RSI = AP_CONTEXTS pointer
    # R8D = AP_CONTEXT_COUNT
search_loop:
    cmp ecx, r8d
    jae bounds_halt

    # Calculate entry address: AP_CONTEXTS + AP_CONTEXT_COUNT * AP_CONTEXT_SIZE
    mov rdi, rcx
    imul rdi, {ap_context_size}
    cmp dword ptr [rsi + rdi + {ap_ctx_apic_off}], edx
    je found_id
    inc ecx
    jmp search_loop

found_id:
    # ECX = processor number (index)
    # EDX = APIC ID
    # RSI = AP_CONTEXTS pointer
    # RDI = Context offset (index * AP_CONTEXT_SIZE)

    # Load per-processor stack from ApContext[processor_number].stack_top.
    mov rsp, [rsi + rdi + {ap_ctx_stack_off}]

    # Switch to BSP's GDT
    lgdt [rip + AP_BSP_GDTR]

    # Reload CS via far return.
    lea rax, [rip + bsp_gdt_loaded]
    push {bsp_code64_sel}
    push rax
    retfq

bsp_gdt_loaded:
    # Reload data segments with BSP's data64 selector.
    mov ax, {bsp_data64_sel}
    mov ds, ax
    mov es, ax
    mov ss, ax
    xor ax, ax
    mov fs, ax
    mov gs, ax

    # Pass the selected ApContext as the first argument (RCX for efiapi).
    lea rcx, [rsi + rdi]

    # Reserve the EFIAPI required 32-byte home space and call Rust. RSP was
    # page-aligned when loaded above, so it is also correctly aligned here.
    sub rsp, 0x20
    call {ap_entry}
    add rsp, 0x20

    # If the AP entry point ever returns, halt permanently.

bounds_halt:
    cli
    hlt
    jmp bounds_halt
