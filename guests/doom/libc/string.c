#include <ctype.h>
#include <stdint.h>
#include <stdlib.h>
#include <string.h>

/* rep movsb/stosb are fast on current CPUs, and keep the compiler from
 * turning a copy loop in here back into a call to memcpy. */
void *memcpy(void *dst, const void *src, size_t n)
{
	void *ret = dst;
	__asm__ volatile("rep movsb" : "+D"(dst), "+S"(src), "+c"(n) : : "memory");
	return ret;
}

void *memmove(void *dst, const void *src, size_t n)
{
	if ((uintptr_t)dst - (uintptr_t)src >= n)
		return memcpy(dst, src, n);
	/* dst overlaps the end of src: copy backwards. */
	void *ret = dst;
	dst = (char *)dst + n - 1;
	src = (const char *)src + n - 1;
	__asm__ volatile("std; rep movsb; cld" : "+D"(dst), "+S"(src), "+c"(n) : : "memory");
	return ret;
}

void *memset(void *s, int c, size_t n)
{
	void *ret = s;
	__asm__ volatile("rep stosb" : "+D"(s), "+c"(n) : "a"(c) : "memory");
	return ret;
}

int memcmp(const void *a, const void *b, size_t n)
{
	const unsigned char *x = a, *y = b;
	for (; n; n--, x++, y++)
		if (*x != *y)
			return *x - *y;
	return 0;
}

void *memchr(const void *s, int c, size_t n)
{
	const unsigned char *p = s;
	for (; n; n--, p++)
		if (*p == (unsigned char)c)
			return (void *)p;
	return NULL;
}

size_t strlen(const char *s)
{
	const char *p = s;
	while (*p)
		p++;
	return p - s;
}

char *strcpy(char *dst, const char *src)
{
	char *d = dst;
	while ((*d++ = *src++))
		;
	return dst;
}

char *strncpy(char *dst, const char *src, size_t n)
{
	size_t i = 0;
	for (; i < n && src[i]; i++)
		dst[i] = src[i];
	for (; i < n; i++)
		dst[i] = '\0';
	return dst;
}

char *strcat(char *dst, const char *src)
{
	strcpy(dst + strlen(dst), src);
	return dst;
}

char *strncat(char *dst, const char *src, size_t n)
{
	char *d = dst + strlen(dst);
	for (; n && *src; n--)
		*d++ = *src++;
	*d = '\0';
	return dst;
}

int strcmp(const char *a, const char *b)
{
	for (; *a && *a == *b; a++, b++)
		;
	return (unsigned char)*a - (unsigned char)*b;
}

int strncmp(const char *a, const char *b, size_t n)
{
	for (; n && *a && *a == *b; n--, a++, b++)
		;
	return n ? (unsigned char)*a - (unsigned char)*b : 0;
}

int strcasecmp(const char *a, const char *b)
{
	for (; *a && tolower((unsigned char)*a) == tolower((unsigned char)*b); a++, b++)
		;
	return tolower((unsigned char)*a) - tolower((unsigned char)*b);
}

int strncasecmp(const char *a, const char *b, size_t n)
{
	for (; n && *a && tolower((unsigned char)*a) == tolower((unsigned char)*b); n--, a++, b++)
		;
	return n ? tolower((unsigned char)*a) - tolower((unsigned char)*b) : 0;
}

char *strchr(const char *s, int c)
{
	for (; *s; s++)
		if (*s == (char)c)
			return (char *)s;
	return c ? NULL : (char *)s;
}

char *strrchr(const char *s, int c)
{
	const char *last = NULL;
	do {
		if (*s == (char)c)
			last = s;
	} while (*s++);
	return (char *)last;
}

char *strstr(const char *haystack, const char *needle)
{
	size_t n = strlen(needle);
	for (; *haystack; haystack++)
		if (!strncmp(haystack, needle, n))
			return (char *)haystack;
	return n ? NULL : (char *)haystack;
}

char *strdup(const char *s)
{
	size_t n = strlen(s) + 1;
	char *d = malloc(n);
	return d ? memcpy(d, s, n) : NULL;
}

char *strerror(int errnum)
{
	(void)errnum;
	return "error";
}
