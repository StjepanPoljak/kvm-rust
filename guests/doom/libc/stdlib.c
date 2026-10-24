#include <ctype.h>
#include <errno.h>
#include <limits.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include "kvm.h"

/* A bump allocator. Doom allocates its zone once at startup and frees
 * little else, so free() only gives back the most recent block. */
struct block {
	size_t size;
	size_t pad;	/* keeps the payload 16-byte aligned */
};

static uintptr_t heap_next, heap_end;
static struct block *heap_last;

void heap_init(uintptr_t start, uintptr_t end)
{
	heap_next = (start + 15) & ~(uintptr_t)15;
	heap_end = end;
}

void *malloc(size_t size)
{
	size = (size + 15) & ~(size_t)15;
	size_t avail = heap_end - heap_next;
	if (avail < sizeof(struct block) || size > avail - sizeof(struct block)) {
		errno = ENOMEM;
		return NULL;
	}
	struct block *b = (struct block *)heap_next;
	b->size = size;
	heap_next += sizeof(*b) + size;
	heap_last = b;
	return b + 1;
}

void free(void *ptr)
{
	struct block *b = (struct block *)ptr - 1;
	if (ptr && b == heap_last) {
		heap_next = (uintptr_t)b;
		heap_last = NULL;
	}
}

void *calloc(size_t n, size_t size)
{
	if (size && n > SIZE_MAX / size) {
		errno = ENOMEM;
		return NULL;
	}
	void *p = malloc(n * size);
	return p ? memset(p, 0, n * size) : NULL;
}

void *realloc(void *ptr, size_t size)
{
	if (!ptr)
		return malloc(size);
	struct block *b = (struct block *)ptr - 1;
	if (size <= b->size)
		return ptr;
	size_t grown = (size + 15) & ~(size_t)15;
	if (b == heap_last && grown - b->size <= heap_end - heap_next) {
		heap_next += grown - b->size;
		b->size = grown;
		return ptr;
	}
	void *p = malloc(size);
	if (p)
		memcpy(p, ptr, b->size);
	return p;
}

static int digit_value(int c)
{
	if (isdigit(c))
		return c - '0';
	if (isalpha(c))
		return tolower(c) - 'a' + 10;
	return 99;
}

unsigned long strtoul(const char *s, char **end, int base)
{
	const char *p = s;
	int neg = 0;
	while (isspace((unsigned char)*p))
		p++;
	if (*p == '+' || *p == '-')
		neg = *p++ == '-';
	if ((base == 0 || base == 16) && p[0] == '0' && (p[1] == 'x' || p[1] == 'X') && isxdigit((unsigned char)p[2])) {
		p += 2;
		base = 16;
	} else if (base == 0) {
		base = *p == '0' ? 8 : 10;
	}

	const char *digits = p;
	unsigned long v = 0;
	for (; digit_value((unsigned char)*p) < base; p++)
		v = v * base + digit_value((unsigned char)*p);
	if (end)
		*end = (char *)(p == digits ? s : p);
	return neg ? -v : v;
}

long strtol(const char *s, char **end, int base)
{
	return (long)strtoul(s, end, base);
}

int atoi(const char *s) { return (int)strtol(s, NULL, 10); }
long atol(const char *s) { return strtol(s, NULL, 10); }

double strtod(const char *s, char **end)
{
	const char *p = s;
	double v = 0, scale = 1;
	int neg = 0, any = 0;
	while (isspace((unsigned char)*p))
		p++;
	if (*p == '+' || *p == '-')
		neg = *p++ == '-';
	for (; isdigit((unsigned char)*p); p++, any = 1)
		v = v * 10 + (*p - '0');
	if (*p == '.')
		for (p++; isdigit((unsigned char)*p); p++, any = 1)
			v += (*p - '0') * (scale /= 10);
	if (any && (*p == 'e' || *p == 'E')) {
		char *exp_end;
		long exp = strtol(p + 1, &exp_end, 10);
		if (exp_end != p + 1) {
			p = exp_end;
			for (; exp > 0; exp--)
				v *= 10;
			for (; exp < 0; exp++)
				v /= 10;
		}
	}
	if (end)
		*end = (char *)(any ? p : s);
	return neg ? -v : v;
}

double atof(const char *s) { return strtod(s, NULL); }

int abs(int x) { return x < 0 ? -x : x; }
long labs(long x) { return x < 0 ? -x : x; }

char *getenv(const char *name)
{
	(void)name;
	return NULL;
}

int system(const char *command)
{
	(void)command;
	errno = ENOSYS;
	return -1;
}

/* Shell sort: small, and Doom does not sort anything large. */
void qsort(void *base, size_t n, size_t size, int (*cmp)(const void *, const void *))
{
	char *a = base;
	for (size_t gap = n / 2; gap; gap /= 2)
		for (size_t i = gap; i < n; i++)
			for (size_t j = i; j >= gap && cmp(a + (j - gap) * size, a + j * size) > 0; j -= gap)
				for (size_t k = 0; k < size; k++) {
					char t = a[(j - gap) * size + k];
					a[(j - gap) * size + k] = a[j * size + k];
					a[j * size + k] = t;
				}
}

_Noreturn void exit(int code)
{
	power_off(code);
}

_Noreturn void abort(void)
{
	puts("abort()");
	power_off(134);
}
