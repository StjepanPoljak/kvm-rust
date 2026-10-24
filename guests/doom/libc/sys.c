#include <errno.h>
#include <stdio.h>
#include <sys/stat.h>
#include <unistd.h>
#include "kvm.h"

int errno;

void serial_putc(char c)
{
	while (!(inb(SERIAL_LSR) & SERIAL_LSR_THRE))
		;
	outb(SERIAL_DATA, c);
}

uint32_t pv_clock_ms(void)
{
	return inl(PV_CLOCK_MS);
}

void pv_sleep_ms(uint32_t ms)
{
	outl(PV_SLEEP_MS, ms);
}

_Noreturn void pv_power_off(int code)
{
	outl(PV_POWER_OFF, (uint32_t)code);
	/* The VMM stops this vcpu before it gets here. */
	for (;;)
		__asm__ volatile("hlt");
}

_Noreturn void __assert_fail(const char *expr, const char *file, int line)
{
	printf("assertion failed: %s (%s:%d)\n", expr, file, line);
	pv_power_off(134);
}

/* There is nowhere to create or change files. */
int mkdir(const char *path, mode_t mode)
{
	(void)path; (void)mode;
	errno = EROFS;
	return -1;
}

int access(const char *path, int mode)
{
	(void)mode;
	FILE *f = fopen(path, "r");
	if (!f)
		return -1;
	fclose(f);
	return 0;
}

int unlink(const char *path)
{
	(void)path;
	errno = EROFS;
	return -1;
}
