#include <ctype.h>
#include <errno.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include "kvm.h"

/* Files are either the serial console or read-only memory registered
 * with memfs_add() (the PVH modules). */
enum file_kind { FILE_CONSOLE, FILE_MEMORY };

struct FILE {
	enum file_kind kind;
	const unsigned char *data;
	size_t size;
	size_t pos;
	int eof;
	int error;
};

static FILE console = { .kind = FILE_CONSOLE };
FILE *stdin = &console, *stdout = &console, *stderr = &console;

#define MEMFS_MAX 16

static struct {
	const char *name;
	const void *data;
	size_t size;
} memfs[MEMFS_MAX];
static int memfs_count;

int memfs_add(const char *name, const void *data, size_t size)
{
	if (memfs_count == MEMFS_MAX)
		return -1;
	memfs[memfs_count].name = name;
	memfs[memfs_count].data = data;
	memfs[memfs_count].size = size;
	memfs_count++;
	return 0;
}

static const char *base_name(const char *path)
{
	const char *slash = strrchr(path, '/');
	return slash ? slash + 1 : path;
}

FILE *fopen(const char *path, const char *mode)
{
	if (strchr(mode, 'w') || strchr(mode, 'a') || strchr(mode, '+')) {
		errno = EROFS;
		return NULL;
	}
	for (int i = 0; i < memfs_count; i++) {
		if (strcasecmp(base_name(path), memfs[i].name))
			continue;
		FILE *f = calloc(1, sizeof(*f));
		if (!f)
			return NULL;
		f->kind = FILE_MEMORY;
		f->data = memfs[i].data;
		f->size = memfs[i].size;
		return f;
	}
	errno = ENOENT;
	return NULL;
}

int fclose(FILE *f)
{
	if (f != &console)
		free(f);
	return 0;
}

size_t fread(void *ptr, size_t size, size_t n, FILE *f)
{
	if (f->kind != FILE_MEMORY || !size) {
		f->eof = 1;
		return 0;
	}
	size_t items = (f->size - f->pos) / size;
	if (items < n) {
		n = items;
		f->eof = 1;
	}
	memcpy(ptr, f->data + f->pos, n * size);
	f->pos += n * size;
	return n;
}

size_t fwrite(const void *ptr, size_t size, size_t n, FILE *f)
{
	if (f->kind != FILE_CONSOLE) {
		f->error = 1;
		errno = EBADF;
		return 0;
	}
	const char *p = ptr;
	for (size_t i = 0; i < size * n; i++)
		serial_putc(p[i]);
	return n;
}

int fseek(FILE *f, long offset, int whence)
{
	long base = whence == SEEK_SET ? 0 : whence == SEEK_CUR ? (long)f->pos : (long)f->size;
	if (f->kind != FILE_MEMORY || base + offset < 0 || base + offset > (long)f->size) {
		errno = EINVAL;
		return -1;
	}
	f->pos = base + offset;
	f->eof = 0;
	return 0;
}

long ftell(FILE *f)
{
	return f->kind == FILE_MEMORY ? (long)f->pos : -1;
}

int fflush(FILE *f) { (void)f; return 0; }
int feof(FILE *f) { return f->eof; }
int ferror(FILE *f) { return f->error; }

int fgetc(FILE *f)
{
	if (f->kind != FILE_MEMORY || f->pos == f->size) {
		f->eof = 1;
		return EOF;
	}
	return f->data[f->pos++];
}

char *fgets(char *s, int size, FILE *f)
{
	int i = 0, c = 0;
	while (i < size - 1 && (c = fgetc(f)) != EOF) {
		s[i++] = c;
		if (c == '\n')
			break;
	}
	if (!i)
		return NULL;
	s[i] = '\0';
	return s;
}

int fputc(int c, FILE *f)
{
	return fwrite(&(char){ c }, 1, 1, f) ? (unsigned char)c : EOF;
}

int putc(int c, FILE *f) { return fputc(c, f); }
int putchar(int c) { return fputc(c, stdout); }

int fputs(const char *s, FILE *f)
{
	return fwrite(s, 1, strlen(s), f) || !*s ? 0 : EOF;
}

int puts(const char *s)
{
	return fputs(s, stdout) == EOF ? EOF : putchar('\n');
}

int remove(const char *path)
{
	(void)path;
	errno = EROFS;
	return -1;
}

int rename(const char *from, const char *to)
{
	(void)from; (void)to;
	errno = EROFS;
	return -1;
}

/* --- printf */

/* Formatted output goes to a bounded buffer or straight to the console;
 * `len` counts every character, as snprintf's return value needs. */
struct out {
	char *buf;
	size_t size;
	size_t len;
	int console;
};

static void out_char(struct out *o, char c)
{
	if (o->console)
		serial_putc(c);
	else if (o->len + 1 < o->size)
		o->buf[o->len] = c;
	o->len++;
}

static void out_pad(struct out *o, char c, int n)
{
	for (; n > 0; n--)
		out_char(o, c);
}

enum {
	F_LEFT = 1 << 0,
	F_PLUS = 1 << 1,
	F_SPACE = 1 << 2,
	F_ALT = 1 << 3,
	F_ZERO = 1 << 4,
};

static void out_str(struct out *o, const char *s, int len, int width, int flags)
{
	if (!(flags & F_LEFT))
		out_pad(o, ' ', width - len);
	for (int i = 0; i < len; i++)
		out_char(o, s[i]);
	if (flags & F_LEFT)
		out_pad(o, ' ', width - len);
}

static void out_num(struct out *o, unsigned long long v, int neg, int base, int upper,
		    int width, int prec, int flags)
{
	const char *set = upper ? "0123456789ABCDEF" : "0123456789abcdef";
	char digits[24];
	int n = 0;
	if (v == 0 && prec != 0)
		digits[n++] = '0';
	for (; v; v /= base)
		digits[n++] = set[v % base];

	char sign = neg ? '-' : flags & F_PLUS ? '+' : flags & F_SPACE ? ' ' : 0;
	const char *prefix = "";
	if (flags & F_ALT && base == 16 && n && digits[n - 1] != '0')
		prefix = upper ? "0X" : "0x";
	else if (flags & F_ALT && base == 8 && (!n || digits[n - 1] != '0'))
		prefix = "0";

	int zeros = prec > n ? prec - n : 0;
	int len = n + zeros + (sign != 0) + (int)strlen(prefix);
	if (flags & F_ZERO && !(flags & F_LEFT) && prec < 0 && width > len) {
		zeros += width - len;
		len = width;
	}
	if (!(flags & F_LEFT))
		out_pad(o, ' ', width - len);
	if (sign)
		out_char(o, sign);
	for (; *prefix; prefix++)
		out_char(o, *prefix);
	out_pad(o, '0', zeros);
	while (n)
		out_char(o, digits[--n]);
	if (flags & F_LEFT)
		out_pad(o, ' ', width - len);
}

/* Fixed-point only, which is all Doom prints. */
static void out_double(struct out *o, double d, int width, int prec, int flags)
{
	if (prec < 0)
		prec = 6;
	if (prec > 9)
		prec = 9;
	int neg = d < 0;
	if (neg)
		d = -d;
	unsigned long long scale = 1;
	for (int i = 0; i < prec; i++)
		scale *= 10;
	unsigned long long ip = (unsigned long long)d;
	unsigned long long fp = (unsigned long long)((d - (double)ip) * scale + 0.5);
	if (fp >= scale) {
		ip++;
		fp -= scale;
	}

	char tmp[48];
	struct out t = { .buf = tmp, .size = sizeof(tmp) };
	out_num(&t, ip, neg, 10, 0, 0, -1, flags & (F_PLUS | F_SPACE));
	if (prec) {
		out_char(&t, '.');
		out_num(&t, fp, 0, 10, 0, 0, prec, 0);
	}
	out_str(o, tmp, t.len < sizeof(tmp) ? (int)t.len : (int)sizeof(tmp) - 1, width, flags);
}

static void out_format(struct out *o, const char *fmt, va_list ap)
{
	for (; *fmt; fmt++) {
		if (*fmt != '%') {
			out_char(o, *fmt);
			continue;
		}

		int flags = 0;
		for (;; fmt++) {
			if (fmt[1] == '-') flags |= F_LEFT;
			else if (fmt[1] == '+') flags |= F_PLUS;
			else if (fmt[1] == ' ') flags |= F_SPACE;
			else if (fmt[1] == '#') flags |= F_ALT;
			else if (fmt[1] == '0') flags |= F_ZERO;
			else break;
		}
		fmt++;

		int width = 0;
		if (*fmt == '*') {
			width = va_arg(ap, int);
			if (width < 0) {
				flags |= F_LEFT;
				width = -width;
			}
			fmt++;
		}
		for (; isdigit((unsigned char)*fmt); fmt++)
			width = width * 10 + (*fmt - '0');

		int prec = -1;
		if (*fmt == '.') {
			fmt++;
			prec = 0;
			if (*fmt == '*') {
				prec = va_arg(ap, int);
				fmt++;
			}
			for (; isdigit((unsigned char)*fmt); fmt++)
				prec = prec * 10 + (*fmt - '0');
		}

		/* 0: int, 1: long, 2: long long; size_t and friends are long here. */
		int lng = 0;
		for (; *fmt && strchr("hlzjtL", *fmt); fmt++)
			if (*fmt == 'l' || *fmt == 'z' || *fmt == 'j' || *fmt == 't')
				lng++;

		switch (*fmt) {
		case 'd':
		case 'i': {
			long long v = lng ? va_arg(ap, long) : va_arg(ap, int);
			out_num(o, v < 0 ? -(unsigned long long)v : (unsigned long long)v, v < 0, 10, 0, width, prec, flags);
			break;
		}
		case 'u':
		case 'x':
		case 'X':
		case 'o': {
			unsigned long long v = lng ? va_arg(ap, unsigned long) : va_arg(ap, unsigned int);
			int base = *fmt == 'u' ? 10 : *fmt == 'o' ? 8 : 16;
			out_num(o, v, 0, base, *fmt == 'X', width, prec, flags);
			break;
		}
		case 'p':
			out_num(o, (uintptr_t)va_arg(ap, void *), 0, 16, 0, width, prec, flags | F_ALT);
			break;
		case 'c': {
			char c = (char)va_arg(ap, int);
			out_str(o, &c, 1, width, flags);
			break;
		}
		case 's': {
			const char *s = va_arg(ap, const char *);
			if (!s)
				s = "(null)";
			int len = 0;
			while (s[len] && (prec < 0 || len < prec))
				len++;
			out_str(o, s, len, width, flags);
			break;
		}
		case 'f':
		case 'F':
		case 'e':
		case 'E':
		case 'g':
		case 'G':
			out_double(o, va_arg(ap, double), width, prec, flags);
			break;
		case 'n':
			*va_arg(ap, int *) = (int)o->len;
			break;
		case '%':
			out_char(o, '%');
			break;
		case '\0':
			return;
		default:
			out_char(o, '%');
			out_char(o, *fmt);
		}
	}
}

int vsnprintf(char *buf, size_t size, const char *fmt, va_list ap)
{
	struct out o = { .buf = buf, .size = size };
	out_format(&o, fmt, ap);
	if (size)
		buf[o.len < size ? o.len : size - 1] = '\0';
	return (int)o.len;
}

int vsprintf(char *buf, const char *fmt, va_list ap)
{
	return vsnprintf(buf, SIZE_MAX, fmt, ap);
}

int vfprintf(FILE *f, const char *fmt, va_list ap)
{
	if (f->kind != FILE_CONSOLE) {
		errno = EBADF;
		return -1;
	}
	struct out o = { .console = 1 };
	out_format(&o, fmt, ap);
	return (int)o.len;
}

int vprintf(const char *fmt, va_list ap)
{
	return vfprintf(stdout, fmt, ap);
}

int printf(const char *fmt, ...)
{
	va_list ap;
	va_start(ap, fmt);
	int n = vfprintf(stdout, fmt, ap);
	va_end(ap);
	return n;
}

int fprintf(FILE *f, const char *fmt, ...)
{
	va_list ap;
	va_start(ap, fmt);
	int n = vfprintf(f, fmt, ap);
	va_end(ap);
	return n;
}

int sprintf(char *buf, const char *fmt, ...)
{
	va_list ap;
	va_start(ap, fmt);
	int n = vsprintf(buf, fmt, ap);
	va_end(ap);
	return n;
}

int snprintf(char *buf, size_t size, const char *fmt, ...)
{
	va_list ap;
	va_start(ap, fmt);
	int n = vsnprintf(buf, size, fmt, ap);
	va_end(ap);
	return n;
}

/* --- sscanf: integers, strings and characters, which is what Doom parses. */

int sscanf(const char *str, const char *fmt, ...)
{
	va_list ap;
	va_start(ap, fmt);
	const char *s = str;
	int count = 0;

	while (*fmt) {
		if (isspace((unsigned char)*fmt)) {
			while (isspace((unsigned char)*s))
				s++;
			fmt++;
			continue;
		}
		if (*fmt != '%' || fmt[1] == '%') {
			if (*fmt == '%')
				fmt++;
			if (*s != *fmt)
				break;
			s++;
			fmt++;
			continue;
		}

		fmt++;
		int suppress = *fmt == '*';
		if (suppress)
			fmt++;
		int width = 0;
		for (; isdigit((unsigned char)*fmt); fmt++)
			width = width * 10 + (*fmt - '0');
		int lng = 0;
		for (; *fmt == 'l' || *fmt == 'h'; fmt++)
			lng += *fmt == 'l';
		char conv = *fmt++;

		if (conv == 'n') {
			if (!suppress)
				*va_arg(ap, int *) = (int)(s - str);
			continue;
		}
		if (conv != 'c')
			while (isspace((unsigned char)*s))
				s++;
		if (!*s) {
			if (!count)
				count = EOF;
			break;
		}

		if (strchr("diuxXo", conv)) {
			int base = conv == 'i' ? 0 : conv == 'o' ? 8 : (conv == 'x' || conv == 'X') ? 16 : 10;
			char *end;
			long v = strtol(s, &end, base);
			if (end == s)
				break;
			s = end;
			if (!suppress) {
				if (lng)
					*va_arg(ap, long *) = v;
				else
					*va_arg(ap, int *) = (int)v;
				count++;
			}
		} else if (conv == 's' || conv == 'c') {
			char *dst = suppress ? NULL : va_arg(ap, char *);
			int max = width ? width : conv == 'c' ? 1 : INT32_MAX;
			int n = 0;
			for (; *s && n < max && (conv == 'c' || !isspace((unsigned char)*s)); n++, s++)
				if (dst)
					dst[n] = *s;
			if (dst && conv == 's')
				dst[n] = '\0';
			if (!suppress)
				count++;
		} else {
			break;
		}
	}

	va_end(ap);
	return count;
}
