/*
 * PS/2 keyboard through the i8042 controller. With translation on (the
 * default, and set again here) the keyboard's bytes arrive as scancode
 * set 1. IRQ 1 comes through the IOAPIC; the handler only queues the
 * bytes, DG_GetKey() turns them into Doom keys.
 *
 * Built with -mgeneral-regs-only, like idt.c: it runs in interrupts.
 */

#include <stdio.h>
#include "kvm.h"

#define I8042_DATA		0x60
#define I8042_STATUS		0x64	/* read */
#define I8042_COMMAND		0x64	/* write */
#define I8042_STATUS_OBF	0x01
#define I8042_CMD_READ_CONFIG	0x20
#define I8042_CMD_WRITE_CONFIG	0x60
#define I8042_CONFIG_KBD_INT	0x01
#define I8042_CONFIG_KBD_DISABLE 0x10
#define I8042_CONFIG_TRANSLATE	0x40

#define KEYBOARD_IRQ		1
#define WAIT_LOOPS		100000

/* A power of two, so the indices can wrap freely. */
#define QUEUE_SIZE		64
static volatile uint8_t queue[QUEUE_SIZE];
static volatile uint32_t queue_head, queue_tail;

static void queue_push(uint8_t byte)
{
	if (queue_head - queue_tail < QUEUE_SIZE)
		queue[queue_head++ % QUEUE_SIZE] = byte;
}

int keyboard_read(uint8_t *scancode)
{
	if (queue_tail == queue_head)
		return 0;
	*scancode = queue[queue_tail++ % QUEUE_SIZE];
	return 1;
}

void keyboard_interrupt(void)
{
	while (inb(I8042_STATUS) & I8042_STATUS_OBF)
		queue_push(inb(I8042_DATA));
}

static int wait_output(void)
{
	for (int i = 0; i < WAIT_LOOPS; i++)
		if (inb(I8042_STATUS) & I8042_STATUS_OBF)
			return 1;
	return 0;
}

void keyboard_init(void)
{
	__asm__ volatile("cli");

	while (inb(I8042_STATUS) & I8042_STATUS_OBF)
		inb(I8042_DATA);

	outb(I8042_COMMAND, I8042_CMD_READ_CONFIG);
	if (!wait_output()) {
		puts("[kvm] no i8042 keyboard controller");
		__asm__ volatile("sti");
		return;
	}
	uint8_t config = inb(I8042_DATA);
	config |= I8042_CONFIG_KBD_INT | I8042_CONFIG_TRANSLATE;
	config &= ~I8042_CONFIG_KBD_DISABLE;
	outb(I8042_COMMAND, I8042_CMD_WRITE_CONFIG);
	outb(I8042_DATA, config);

	ioapic_route(KEYBOARD_IRQ, VECTOR_KEYBOARD);
	/* IRQ 1 is edge-triggered: a byte that arrived while the pin was
	 * masked raised no interrupt, and the next one cannot until the
	 * buffer is read. */
	keyboard_interrupt();

	__asm__ volatile("sti");
}
