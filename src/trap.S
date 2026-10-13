.section .text.trap, "ax"
.balign 4
.globl _trap_entry

_trap_entry:
    csrr a0, scause
    csrr a1, sepc
    csrr a2, stval
    csrr a3, sstatus

    mv a4, sp 
    andi sp, sp, -16

    tail rust_trap_fatal