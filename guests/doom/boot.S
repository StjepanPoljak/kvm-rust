/*
 * PVH entry. The VMM jumps to pvh_start in 32-bit protected mode with
 * flat segments, paging off, interrupts off and %ebx pointing to the
 * struct hvm_start_info. Switch to 64-bit long mode with the first 4 GiB
 * identity-mapped, enable SSE (the x86-64 ABI uses it for floats) and
 * call kmain(start_info). There is no IDT: any exception triple-faults,
 * which the VMM reports as "Guest shutdown".
 */

#define CR0_PE		(1 << 0)
#define CR0_MP		(1 << 1)
#define CR0_EM		(1 << 2)
#define CR0_PG		(1 << 31)
#define CR4_PAE		(1 << 5)
#define CR4_OSFXSR	(1 << 9)
#define CR4_OSXMMEXCPT	(1 << 10)
#define MSR_EFER	0xc0000080
#define EFER_LME	(1 << 8)
#define PTE_P		(1 << 0)
#define PTE_W		(1 << 1)
#define PTE_PS		(1 << 7)	/* 2 MiB page in a page directory */
#define GDT_CODE	0x08
#define GDT_DATA	0x10

/* The VMM finds pvh_start through this note, in a section named .notes. */
	.section .notes, "a"
	.align 4
	.long	4			/* name size, "Xen\0" */
	.long	4			/* descriptor size */
	.long	18			/* XEN_ELFNOTE_PHYS32_ENTRY */
	.asciz	"Xen"
	.long	pvh_start

	.section .text.boot, "ax"
	.code32
	.global	pvh_start
pvh_start:
	cli
	cld
	mov	%ebx, %esi

	/* Clear .bss before using it: the page tables and stack live there. */
	mov	$__bss_start, %edi
	mov	$__bss_end, %ecx
	sub	%edi, %ecx
	xor	%eax, %eax
	rep stosb
	mov	%esi, start_info

	/* PML4[0] -> PDPT, PDPT[0..3] -> four page directories. */
	mov	$(pdpt + (PTE_P | PTE_W)), %eax
	mov	%eax, pml4
	mov	$(pd + (PTE_P | PTE_W)), %eax
	xor	%ecx, %ecx
1:	mov	%eax, pdpt(, %ecx, 8)
	add	$0x1000, %eax
	inc	%ecx
	cmp	$4, %ecx
	jne	1b

	/* 2048 entries of 2 MiB: physical address == virtual address. */
	mov	$(PTE_P | PTE_W | PTE_PS), %eax
	xor	%ecx, %ecx
2:	mov	%eax, pd(, %ecx, 8)
	add	$0x200000, %eax
	inc	%ecx
	cmp	$2048, %ecx
	jne	2b

	mov	%cr4, %eax
	or	$(CR4_PAE | CR4_OSFXSR | CR4_OSXMMEXCPT), %eax
	mov	%eax, %cr4
	mov	$pml4, %eax
	mov	%eax, %cr3

	mov	$MSR_EFER, %ecx
	rdmsr
	or	$EFER_LME, %eax
	wrmsr

	/* Paging on with EFER.LME set activates long mode; EM off for SSE. */
	mov	%cr0, %eax
	and	$~CR0_EM, %eax
	or	$(CR0_PG | CR0_MP | CR0_PE), %eax
	mov	%eax, %cr0

	lgdt	gdt_ptr
	ljmp	$GDT_CODE, $long_mode

	.code64
long_mode:
	mov	$GDT_DATA, %ax
	mov	%ax, %ds
	mov	%ax, %es
	mov	%ax, %ss
	mov	%ax, %fs
	mov	%ax, %gs
	lea	stack_top(%rip), %rsp
	fninit
	mov	start_info(%rip), %edi
	call	kmain
3:	hlt
	jmp	3b

	.section .rodata
	.align 8
gdt:
	.quad	0
	.quad	0x00af9a000000ffff	/* GDT_CODE: 64-bit, ring 0 */
	.quad	0x00cf92000000ffff	/* GDT_DATA */
gdt_end:
gdt_ptr:
	.word	gdt_end - gdt - 1
	.long	gdt

	.section .bss
	.align 4096
pml4:	.skip	4096
pdpt:	.skip	4096
pd:	.skip	4096 * 4
	.align 16
stack:	.skip	1 << 20
stack_top:
	.align 4
start_info:
	.skip	4

	.section .note.GNU-stack, "", @progbits
