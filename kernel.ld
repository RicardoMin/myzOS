OUTPUT_ARCH(riscv)
ENTRY(_entry)

SECTIONS
{
    . = 0x80200000;

    .text : {
        KEEP(*(.text.entry))
        *(.text .text.*)
    }

    .rodata : ALIGN(16) {
        *(.rodata .rodata.* .srodata .srodata.*)
    }

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
}