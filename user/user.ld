OUTPUT_ARCH(riscv)
ENTRY(_start)

SECTIONS
{
    . = 0x1000;
    __user_start = .;

    .text : {
        KEEP(*(.text.entry))
        *(.text .text.*)
    }

    .rodata : {
        *(.rodata .rodata.* .srodata .srodata.*)
    }

    .data : {
        *(.data .data.* .sdata .sdata.* .got .got.*)
    }

    .bss (NOLOAD) : {
        *(.bss .bss.* .sbss .sbss.*)
        *(COMMON)
    }

    __user_end = .;

    ASSERT(__user_end <= 0x2000, "user image exceeds one page")
    ASSERT(_start == 0x1000, "user entry must be 0x1000")

    /DISCARD/ : {
        *(.eh_frame .eh_frame.* .comment)
    }
}