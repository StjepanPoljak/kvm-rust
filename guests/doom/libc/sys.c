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

/* Until there is ACPI PM1a_CNT to write to: an empty IDT makes the int3
 * a triple fault, which the VMM reports as "Guest shutdown" and stops.
 * The exit code only gets printed. */
_Noreturn void power_off(int code)
{
	static const struct { uint16_t limit; uint64_t base; } __attribute__((packed)) no_idt;

	printf("[kvm] power off (exit code %d)\n", code);
	__asm__ volatile("lidt %0; int3" : : "m"(no_idt));
	__builtin_unreachable();
}

_Noreturn void __assert_fail(const char *expr, const char *file, int line)
{
	printf("assertion failed: %s (%s:%d)\n", expr, file, line);
	power_off(134);
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
