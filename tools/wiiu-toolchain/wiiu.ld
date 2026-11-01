/* Linker script for Wii U programs, a stripped down version of wut's wut.ld. elf2rpl turns the
   linked program with its relocations into an RPX, Cafe OS loads its code at 0x02000000, its data
   at 0x10000000 and reads the imports and symbols from the loader sections at 0xC0000000. */
ENTRY(__rpx_start)

MEMORY
{
	codemem (rwx) : ORIGIN = 0x02000000, LENGTH = 224M
	datamem (rw) : ORIGIN = 0x10000000, LENGTH = 800M
	loadmem (rwx) : ORIGIN = 0xC0000000, LENGTH = 128M
}

SECTIONS
{
	. = ORIGIN(codemem);
	.text : { *(.text .text.*) } > codemem

	. = ORIGIN(datamem);
	.rodata : {
		*(.rodata .rodata.*)
		/* The constructors and finalizers picolibc's __libc_init_array runs */
		. = ALIGN(4);
		PROVIDE_HIDDEN(__preinit_array_start = .);
		KEEP(*(.preinit_array))
		PROVIDE_HIDDEN(__preinit_array_end = .);
		PROVIDE_HIDDEN(__init_array_start = .);
		KEEP(*(SORT_BY_INIT_PRIORITY(.init_array.*))) KEEP(*(.init_array))
		PROVIDE_HIDDEN(__init_array_end = .);
		PROVIDE_HIDDEN(__fini_array_start = .);
		KEEP(*(SORT_BY_INIT_PRIORITY(.fini_array.*))) KEEP(*(.fini_array))
		PROVIDE_HIDDEN(__fini_array_end = .);
	} > datamem
	.data : { *(.data .data.* .sdata .sdata.*) } > datamem
	.bss (NOLOAD) : { *(.sbss .sbss.* .bss .bss.*) *(COMMON) } > datamem

	/* The loader sections, the import sections of the system libraries follow from imports.ld */
	. = ORIGIN(loadmem);
	.fexports ALIGN(32) : { KEEP(*(.fexports)) } > loadmem
	.dexports ALIGN(32) : { KEEP(*(.dexports)) } > loadmem

	/DISCARD/ : { *(.comment) *(.eh_frame) }
}

INCLUDE imports.ld
