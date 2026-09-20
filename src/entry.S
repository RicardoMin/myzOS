.section .text.entry, "ax"
.globl _entry

_entry:
    la sp, boot_stack_top
    call kernel_main

.Lhang:
    j .Lhang

.section .stack, "aw", @nobits
.balign 16

boot_stack_bottom:
    .space 16384
boot_stack_top: