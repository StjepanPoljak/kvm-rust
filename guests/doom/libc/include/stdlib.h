#ifndef _STDLIB_H
#define _STDLIB_H

#include <stddef.h>

#define EXIT_SUCCESS	0
#define EXIT_FAILURE	1

void *malloc(size_t size);
void *calloc(size_t n, size_t size);
void *realloc(void *ptr, size_t size);
void free(void *ptr);

int atoi(const char *s);
long atol(const char *s);
double atof(const char *s);
long strtol(const char *s, char **end, int base);
unsigned long strtoul(const char *s, char **end, int base);
double strtod(const char *s, char **end);

int abs(int x);
long labs(long x);

char *getenv(const char *name);
int system(const char *command);
void qsort(void *base, size_t n, size_t size, int (*cmp)(const void *, const void *));

_Noreturn void exit(int code);
_Noreturn void abort(void);

#endif
