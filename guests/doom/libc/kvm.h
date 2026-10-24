#ifndef _KVM_H
#define _KVM_H

/* What this libc needs from the platform: the serial port, a clock
 * (the KVM pvclock, clock.c) and memory to manage. */

#include <stddef.h>
#include <stdint.h>

#define SERIAL_DATA	0x3f8
#define SERIAL_LSR	0x3fd
#define SERIAL_LSR_THRE	0x20

static inline void outb(uint16_t port, uint8_t val)
{
	__asm__ volatile("outb %0, %1" : : "a"(val), "Nd"(port));
}

static inline uint8_t inb(uint16_t port)
{
	uint8_t val;
	__asm__ volatile("inb %1, %0" : "=a"(val) : "Nd"(port));
	return val;
}

static inline void outl(uint16_t port, uint32_t val)
{
	__asm__ volatile("outl %0, %1" : : "a"(val), "Nd"(port));
}

static inline uint32_t inl(uint16_t port)
{
	uint32_t val;
	__asm__ volatile("inl %1, %0" : "=a"(val) : "Nd"(port));
	return val;
}

void serial_putc(char c);
/* Milliseconds and nanoseconds since clock_init(). */
void clock_init(void);
uint64_t clock_ns(void);
uint32_t clock_ms(void);
void sleep_ms(uint32_t ms);
_Noreturn void power_off(int code);

/* The heap is [start, end); the platform calls this before any malloc. */
void heap_init(uintptr_t start, uintptr_t end);
/* Makes `data` readable through fopen(name); the name is matched against
 * the last component of the path, ignoring case. */
int memfs_add(const char *name, const void *data, size_t size);

#endif
