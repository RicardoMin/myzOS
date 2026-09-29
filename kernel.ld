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