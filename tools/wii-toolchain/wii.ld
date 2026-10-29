/* Linker script for Wii programs, a stripped down version of devkitPPC's rvl.ld */
ENTRY(_start)

PHDRS
{
	text PT_LOAD FLAGS(5);
	data PT_LOAD FLAGS(6);
}

SECTIONS
{
	. = 0x80004000;

	/* IOS can only load DOL segments with a size that is a multiple of 32 bytes,
	   so the last section of each segment ends aligned */
	.init : { KEEP(*(.init)) } :text
	.text : { *(.text .text.*) }
	.rodata : { *(.rodata .rodata.*) . = ALIGN(32); }

	.sdata : { _SDA_BASE_ = .; _SDA2_BASE_ = .; *(.sdata .sdata.* .sdata2 .sdata2.*) } :data
	.data : {
		*(.data .data.*)
		/* The constructors and finalizers libogc's __crtmain runs */
		. = ALIGN(4);
		__init_array_start = .;
		KEEP(*(SORT_BY_INIT_PRIORITY(.init_array.*))) KEEP(*(.init_array))
		__init_array_end = .;
		__fini_array_start = .;
		KEEP(*(SORT_BY_INIT_PRIORITY(.fini_array.*))) KEEP(*(.fini_array))
		__fini_array_end = .;
		. = ALIGN(32);
	}

	/* Clang doesn't use small data, so .sbss just goes into .bss */
	__bss_start = .;
	.bss : { *(.sbss .sbss.* .bss .bss.*) *(COMMON) . = ALIGN(32); }
	__bss_end = .;

	/DISCARD/ : { *(.comment) *(.eh_frame) }
}

/* Main thread stack, interrupt stack and the memory arenas used by libogc */
__isIPL = 0;
__stack_end = __bss_end;
__stack_addr = __stack_end + 0x20000;
__intrstack_end = __stack_addr;
__intrstack_addr = __intrstack_end + 0x4000;
__Arena1Lo = __intrstack_addr;
__Arena1Hi = 0x817FEFF0;
__Arena2Lo = 0x90002000;
__Arena2Hi = 0x933E0000;
__gxregs = (__Arena1Hi + 31) & (-32);
__ipcbufferLo = 0x933E0000;
__ipcbufferHi = 0x93400000;
