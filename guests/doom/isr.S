/*
 * Interrupt entry points. Each stub pushes a dummy error code when the
 * CPU does not push one, then its vector, so interrupt_dispatch() always
 * gets the same struct interrupt_frame (idt.c).
 */

	.macro	isr vector, has_error_code=0
	.global	isr_\vector
isr_\vector:
	.if	!\has_error_code
	pushq	$0
	.endif
	pushq	$\vector
	jmp	isr_common
	.endm

	.text
	isr	0
	isr	1
	isr	2
	isr	3
	isr	4
	isr	5
	isr	6
	isr	7
	isr	8, 1
	isr	9
	isr	10, 1
	isr	11, 1
	isr	12, 1
	isr	13, 1
	isr	14, 1
	isr	15
	isr	16
	isr	17, 1
	isr	18
	isr	19
	isr	20
	isr	21, 1
	isr	22
	isr	23
	isr	24
	isr	25
	isr	26
	isr	27
	isr	28
	isr	29, 1
	isr	30, 1
	isr	31

	/* VECTOR_TIMER in idt.c */
	.global	isr_timer
isr_timer:
	pushq	$0
	pushq	$0x30
	jmp	isr_common

/* 5 words pushed by the CPU + 2 by the stub + 15 here keep %rsp 16-byte
 * aligned for the call. */
isr_common:
	push	%rax
	push	%rbx
	push	%rcx
	push	%rdx
	push	%rsi
	push	%rdi
	push	%rbp
	push	%r8
	push	%r9
	push	%r10
	push	%r11
	push	%r12
	push	%r13
	push	%r14
	push	%r15
	mov	%rsp, %rdi
	call	interrupt_dispatch
	pop	%r15
	pop	%r14
	pop	%r13
	pop	%r12
	pop	%r11
	pop	%r10
	pop	%r9
	pop	%r8
	pop	%rbp
	pop	%rdi
	pop	%rsi
	pop	%rdx
	pop	%rcx
	pop	%rbx
	pop	%rax
	add	$16, %rsp
	iretq

/* Spurious LAPIC interrupts must not be acknowledged with an EOI. */
	.global	isr_spurious
isr_spurious:
	iretq

	.section .rodata
	.align	8
	.global	isr_exceptions
isr_exceptions:
	.irp	v, 0,1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16,17,18,19,20,21,22,23,24,25,26,27,28,29,30,31
	.quad	isr_\v
	.endr

	.section .note.GNU-stack, "", @progbits
