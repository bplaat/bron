#pragma once

#include <stddef.h>
#include <sys/types.h>

#define STDIN_FILENO 0
#define STDOUT_FILENO 1
#define STDERR_FILENO 2
#define F_OK 0
#define X_OK 1
#define W_OK 2
#define R_OK 4
#ifndef SEEK_SET
#define SEEK_SET 0
#define SEEK_CUR 1
#define SEEK_END 2
#endif

ssize_t read(int fd, void* buffer, size_t size);
ssize_t write(int fd, const void* buffer, size_t size);
off_t lseek(int fd, off_t offset, int whence);
int close(int fd);
// Only checks that the path exists, everything may be read and written
int access(const char* path, int mode);
int unlink(const char* path);
int rmdir(const char* path);
int chdir(const char* path);
char* getcwd(char* buffer, size_t size);
void* sbrk(ptrdiff_t increment);
int usleep(unsigned microseconds);
