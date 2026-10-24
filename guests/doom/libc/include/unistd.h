#ifndef _UNISTD_H
#define _UNISTD_H

#include <sys/types.h>

int access(const char *path, int mode);
int unlink(const char *path);

#define F_OK	0
#define R_OK	4

#endif
