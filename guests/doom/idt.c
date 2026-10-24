/*
 * Interrupts: an IDT with the CPU exceptions, the LAPIC timer and the
 * keyboard, the legacy PIC remapped out of the way of the exceptions and
 * masked, the IOAPIC for device interrupts, and a one-shot LAPIC timer
 * that sleep_ms() halts on. The KVM in-kernel LAPIC
 * runs the timer, so a sleeping guest costs the host nothing.
 *
 * Built with -mgeneral-regs-only: the stubs in isr.S do not save the SSE
 * registers that interrupted code may be using.
 */

#include <stdio.h>
#include "kvm.h"

#define GDT_CODE		0x08
#define IDT_INTERRUPT_GATE	0x8e	/* present, ring 0, 64-bit interrupt gate */

#define VECTOR_PIC_BASE		0x20	/* 16 vectors, all masked */
#define VECTOR_SPURIOUS		0xff

#define PIC1_CMD		0x20
#define PIC1_DATA		0x21
#define PIC2_CMD		0xa0
#define PIC2_DATA		0xa1

#define MSR_APIC_BASE		0x1b
#define APIC_BASE_ENABLE	(1 << 11)
#define APIC_BASE_MASK		0xfffff000ULL

#define LAPIC_ID		0x020
#define LAPIC_TPR		0x080
#define LAPIC_EOI		0x0b0
#define LAPIC_SVR		0x0f0
#define LAPIC_SVR_ENABLE	(1 << 8)
#define LAPIC_LVT_TIMER		0x320
#define LAPIC_LVT_LINT0		0x350
#define LAPIC_LVT_MASKED	(1 << 16)
#define LAPIC_TIMER_INITIAL	0x380
#define LAPIC_TIMER_CURRENT	0x390
#define LAPIC_TIMER_DIVIDE	0x3e0
#define LAPIC_TIMER_DIVIDE_1	0xb

#define CALIBRATE_MS		10

/* Its pins start out masked; KVM's has 24. */
#define IOAPIC_BASE		0xfec00000
#define IOAPIC_REGSEL		0x00
#define IOAPIC_WINDOW		0x10
#define IOAPIC_REDTBL(pin)	(0x10 + 2 * (pin))

struct idt_entry {
	uint16_t offset_low;
	uint16_t selector;
	uint8_t ist;
	uint8_t type;
	uint16_t offset_mid;
	uint32_t offset_high;
	uint32_t reserved;
} __attribute__((packed));

/* What isr.S leaves on the stack, lowest address first. */
struct interrupt_frame {
	uint64_t r15, r14, r13, r12, r11, r10, r9, r8;
	uint64_t rbp, rdi, rsi, rdx, rcx, rbx, rax;
	uint64_t vector, error_code;
	uint64_t rip, cs, rflags, rsp, ss;
};

extern const uint64_t isr_exceptions[32];
extern char isr_timer[], isr_keyboard[], isr_spurious[];

static struct idt_entry idt[256] __attribute__((aligned(16)));
static volatile uint32_t *lapic;
static uint32_t lapic_ticks_per_ms;
static volatile int timer_fired;

static const char *const exception_names[32] = {
	"divide error", "debug", "NMI", "breakpoint", "overflow", "bound range",
	"invalid opcode", "device not available", "double fault", "coprocessor overrun",
	"invalid TSS", "segment not present", "stack fault", "general protection",
	"page fault", "reserved", "x87 FPU error", "alignment check", "machine check",
	"SIMD exception", "virtualization", "control protection",
};

static inline uint64_t rdmsr(uint32_t msr)
{
	uint32_t lo, hi;
	__asm__ volatile("rdmsr" : "=a"(lo), "=d"(hi) : "c"(msr));
	return ((uint64_t)hi << 32) | lo;
}

static inline uint32_t lapic_read(uint32_t reg)
{
	return lapic[reg / 4];
}

static inline void lapic_write(uint32_t reg, uint32_t val)
{
	lapic[reg / 4] = val;
}

static void idt_set(int vector, uintptr_t handler)
{
	idt[vector] = (struct idt_entry) {
		.offset_low = handler & 0xffff,
		.selector = GDT_CODE,
		.type = IDT_INTERRUPT_GATE,
		.offset_mid = (handler >> 16) & 0xffff,
		.offset_high = handler >> 32,
	};
}

void interrupt_dispatch(struct interrupt_frame *frame)
{
	if (frame->vector == VECTOR_TIMER) {
		timer_fired = 1;
		lapic_write(LAPIC_EOI, 0);
		return;
	}
	if (frame->vector == VECTOR_KEYBOARD) {
		keyboard_interrupt();
		lapic_write(LAPIC_EOI, 0);
		return;
	}

	uint64_t cr2;
	__asm__ volatile("mov %%cr2, %0" : "=r"(cr2));
	const char *name = frame->vector < 32 && exception_names[frame->vector] ?
		exception_names[frame->vector] : "unknown";
	printf("[kvm] exception %lu (%s), error code %#lx\n"
	       "[kvm]   rip %#lx rsp %#lx rflags %#lx cr2 %#lx\n",
	       frame->vector, name, frame->error_code,
	       frame->rip, frame->rsp, frame->rflags, cr2);
	power_off(128 + (int)frame->vector);
}

/* ICW1-4: edge-triggered, cascaded, vectors 0x20-0x2f, then mask every
 * line. The PIC is not used, but left at its reset vectors any stray
 * interrupt from it would look like an exception. */
static void pic_disable(void)
{
	outb(PIC1_CMD, 0x11);
	outb(PIC2_CMD, 0x11);
	outb(PIC1_DATA, VECTOR_PIC_BASE);
	outb(PIC2_DATA, VECTOR_PIC_BASE + 8);
	outb(PIC1_DATA, 4);	/* slave on IRQ2 */
	outb(PIC2_DATA, 2);
	outb(PIC1_DATA, 1);	/* 8086 mode */
	outb(PIC2_DATA, 1);
	outb(PIC1_DATA, 0xff);
	outb(PIC2_DATA, 0xff);
}

/* The LAPIC timer counts at the APIC bus rate, which the CPU does not
 * report; measure it against the pvclock. */
static void lapic_timer_calibrate(void)
{
	lapic_write(LAPIC_LVT_TIMER, LAPIC_LVT_MASKED | VECTOR_TIMER);
	lapic_write(LAPIC_TIMER_DIVIDE, LAPIC_TIMER_DIVIDE_1);

	uint64_t start = clock_ns();
	lapic_write(LAPIC_TIMER_INITIAL, 0xffffffff);
	while (clock_ns() - start < CALIBRATE_MS * 1000000ULL)
		;
	uint32_t elapsed = 0xffffffff - lapic_read(LAPIC_TIMER_CURRENT);
	lapic_write(LAPIC_TIMER_INITIAL, 0);

	lapic_ticks_per_ms = elapsed / CALIBRATE_MS;
	printf("[kvm] LAPIC timer at %u kHz\n", lapic_ticks_per_ms);
}

void interrupts_init(void)
{
	for (int v = 0; v < 32; v++)
		idt_set(v, isr_exceptions[v]);
	idt_set(VECTOR_TIMER, (uintptr_t)isr_timer);
	idt_set(VECTOR_KEYBOARD, (uintptr_t)isr_keyboard);
	idt_set(VECTOR_SPURIOUS, (uintptr_t)isr_spurious);

	struct { uint16_t limit; uint64_t base; } __attribute__((packed)) idtr = {
		sizeof(idt) - 1, (uintptr_t)idt
	};
	__asm__ volatile("lidt %0" : : "m"(idtr));

	pic_disable();

	/* The first 4 GiB are identity-mapped, so the LAPIC page is too. */
	uint64_t apic_base = rdmsr(MSR_APIC_BASE);
	if (!(apic_base & APIC_BASE_ENABLE)) {
		puts("[kvm] LAPIC disabled");
		power_off(1);
	}
	lapic = (volatile uint32_t *)(uintptr_t)(apic_base & APIC_BASE_MASK);
	lapic_write(LAPIC_SVR, LAPIC_SVR_ENABLE | VECTOR_SPURIOUS);
	lapic_write(LAPIC_TPR, 0);
	lapic_write(LAPIC_LVT_LINT0, LAPIC_LVT_MASKED);
	lapic_timer_calibrate();

	/* One-shot (mode bits 0), unmasked. */
	lapic_write(LAPIC_LVT_TIMER, VECTOR_TIMER);
	__asm__ volatile("sti");
}

static void ioapic_write(uint32_t reg, uint32_t val)
{
	volatile uint32_t *ioapic = (volatile uint32_t *)IOAPIC_BASE;
	ioapic[IOAPIC_REGSEL / 4] = reg;
	ioapic[IOAPIC_WINDOW / 4] = val;
}

/* ISA IRQs map to the same IOAPIC pin (no source overrides here except
 * IRQ 0) and are edge-triggered, active high: the low word's zero
 * fields. Delivered to this CPU's LAPIC. */
void ioapic_route(int irq, int vector)
{
	uint32_t apic_id = lapic_read(LAPIC_ID) >> 24;
	ioapic_write(IOAPIC_REDTBL(irq) + 1, apic_id << 24);
	ioapic_write(IOAPIC_REDTBL(irq), vector);
}

/* Arms the one-shot timer and halts until it fires. Checking the flag
 * with interrupts off and then `sti; hlt` cannot miss the interrupt:
 * sti only takes effect after the next instruction. */
void sleep_ms(uint32_t ms)
{
	while (ms) {
		uint32_t chunk = ms < 0xffffffff / lapic_ticks_per_ms ? ms : 0xffffffff / lapic_ticks_per_ms;
		ms -= chunk;
		timer_fired = 0;
		lapic_write(LAPIC_TIMER_INITIAL, chunk * lapic_ticks_per_ms);
		for (;;) {
			__asm__ volatile("cli");
			if (timer_fired)
				break;
			__asm__ volatile("sti; hlt");
		}
		__asm__ volatile("sti");
	}
}
