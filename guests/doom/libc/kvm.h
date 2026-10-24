#ifndef _KVM_H
#define _KVM_H

/* What this libc needs from the VMM: the serial port, the paravirt ports
 * (src/arch/x86_64/paravirt.rs) and memory to manage. */

#include <stddef.h>
#include <stdint.h>

#define SERIAL_DATA	0x3f8
#define SERIAL_LSR	0x3fd
#define SERIAL_LSR_THRE	0x20

#define PV_CLOCK_MS	0x600
#define PV_SLEEP_MS	0x604
#define PV_POWER_OFF	0x608

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
uint32_t pv_clock_ms(void);
void pv_sleep_ms(uint32_t ms);
_Noreturn void pv_power_off(int code);

/* The heap is [start, end); the platform calls this before any malloc. */
void heap_init(uintptr_t start, uintptr_t end);
/* Makes `data` readable through fopen(name); the name is matched against
 * the last component of the path, ignoring case. */
int memfs_add(const char *name, const void *data, size_t size);

#endif
