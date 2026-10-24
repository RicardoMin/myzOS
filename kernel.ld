OUTPUT_ARCH(riscv)
ENTRY(_entry)

SECTIONS
{
    . = 0x80200000;
    __kernel_start = .;

    .text : {
        KEEP(*(.text.entry))
        *(.text .text.*)
    }

    . = ALIGN(4096);
    .trampoline : {
        __trampoline_start = .;
        KEEP(*(.trampoline))
        __trampoline_code_end = .;

        . = ALIGN(4096);
        __trampoline_end = .;

        ASSERT(__trampoline_code_end > __trampoline_start,
                "trampoline is empty")
        ASSERT(__trampoline_end - __trampoline_start == 4096,
                "trampoline must fit in one page")
    }
    . = ALIGN(4096);
    __text_end = .;

    .rodata : ALIGN(16) {
        *(.rodata .rodata.* .srodata .srodata.*)
        *(.eh_frame .eh_frame.*)
    }

    . = ALIGN(4096);
    __rodata_end = .;

    .data : ALIGN(16) {
        *(.data .data.* .sdata .sdata.*)
    }

    .bss (NOLOAD) : ALIGN(16) {
        *(.bss .bss.* .sbss .sbss.*)
        *(COMMON)
    }

    .stack (NOLOAD) : ALIGN(16) {
        KEEP(*(.stack))
    }

    . = ALIGN(4096);
    __kernel_end = .;
}