.section .text

.macro curr_el_to reg
	mrs	\reg, CurrentEL
	lsr	\reg, \reg, #2
	and	\reg, \reg, #0xf
.endm

.globl _start
_start:
	mrs	x0, mpidr_el1
	and	x0, x0, #0xFF
	cbz	x0, control
	b .

control:
	curr_el_to x0
	b .
