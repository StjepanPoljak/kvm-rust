ENTRY (_start)

SECTIONS
{
	. = 0x40000;

	.text : {
		   _start = .;
		   KEEP(*(.text._start))
		   KEEP(*(.text*))
	}

	.data : {
	      KEEP(*(.data*))
	}
}
