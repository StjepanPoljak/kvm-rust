#ifndef _STDIO_H
#define _STDIO_H

#include <stddef.h>
#include <stdarg.h>

typedef struct FILE FILE;

extern FILE *stdin, *stdout, *stderr;

#define EOF		(-1)
#define SEEK_SET	0
#define SEEK_CUR	1
#define SEEK_END	2
#define BUFSIZ		1024
#define FILENAME_MAX	256

FILE *fopen(const char *path, const char *mode);
int fclose(FILE *f);
size_t fread(void *ptr, size_t size, size_t n, FILE *f);
size_t fwrite(const void *ptr, size_t size, size_t n, FILE *f);
int fseek(FILE *f, long offset, int whence);
long ftell(FILE *f);
int fflush(FILE *f);
int feof(FILE *f);
int ferror(FILE *f);
int fgetc(FILE *f);
char *fgets(char *s, int size, FILE *f);
int fputc(int c, FILE *f);
int fputs(const char *s, FILE *f);
int putc(int c, FILE *f);
int putchar(int c);
int puts(const char *s);
int remove(const char *path);
int rename(const char *from, const char *to);

int printf(const char *fmt, ...) __attribute__((format(printf, 1, 2)));
int fprintf(FILE *f, const char *fmt, ...) __attribute__((format(printf, 2, 3)));
int sprintf(char *buf, const char *fmt, ...) __attribute__((format(printf, 2, 3)));
int snprintf(char *buf, size_t size, const char *fmt, ...) __attribute__((format(printf, 3, 4)));
int vprintf(const char *fmt, va_list ap);
int vfprintf(FILE *f, const char *fmt, va_list ap);
int vsprintf(char *buf, const char *fmt, va_list ap);
int vsnprintf(char *buf, size_t size, const char *fmt, va_list ap);
int sscanf(const char *str, const char *fmt, ...);

#endif
