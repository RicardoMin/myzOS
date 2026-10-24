.section .trampoline, "ax"
.option push
.option norvc
.balign 4

.global _trampoline_probe
.type _trampoline_probe, @function

_trampoline_probe:
    auipc a0, 0
    ret

.size _trampoline_probe, . - _trampoline_probe
.option pop