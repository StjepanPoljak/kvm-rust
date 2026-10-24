/* Loaded by the VMM's PVH ELF loader at the physical addresses below. */
OUTPUT_FORMAT(elf64-x86-64)
ENTRY(pvh_start)

SECTIONS
{
	. = 0x100000;

	.text : { *(.text.boot) *(.text .text.*) }
	.rodata : { *(.rodata .rodata.*) }
	.notes : { KEEP(*(.notes)) }
	.data : { *(.data .data.*) }

	/* Part of the last PT_LOAD, so the VMM places modules after it. */
	.bss : ALIGN(4096) {
		__bss_start = .;
		*(.bss .bss.*)
		*(COMMON)
		__bss_end = .;
	}
	_end = .;

	/DISCARD/ : { *(.comment) *(.eh_frame*) *(.note.*) }
}
