.section .text.entry, "ax"
.globl _entry

_entry:
    mv tp, a0
    la sp, boot_stack_top
    call kernel_main

.Lhang:
    j .Lhang

.section .stack, "aw", @nobits
.balign 16

boot_stack_bottom:
    .space 16384
boot_stack_top:

# 启动第二个 hart 的代码
.section .text.entry, "ax"
.balign 4
.globl _secondary_entry

_secondary_entry:
    mv tp, a0
    la sp, secondary_stack_top
    call secondary_main

.Lsecondary_hang:
    j .Lsecondary_hang

.section .stack, "aw", @nobits
.balign 16

secondary_stack_bottom:
    .space 16384
secondary_stack_top: