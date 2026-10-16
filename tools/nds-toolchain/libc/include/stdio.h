#pragma once

#include <stdarg.h>
#include <stddef.h>
#include <sys/types.h>

#define EOF (-1)
#define BUFSIZ 512
#define FILENAME_MAX 1024
#define SEEK_SET 0
#define SEEK_CUR 1
#define SEEK_END 2
#define _IOFBF 0
#define _IOLBF 1
#define _IONBF 2

typedef struct FILE FILE;
extern FILE* const stdin;
extern FILE* const stdout;
extern FILE* const stderr;

FILE* fopen(const char* restrict path, const char* restrict mode);
int fclose(FILE* file);
size_t fread(void* restrict buffer, size_t size, size_t count, FILE* restrict file);
size_t fwrite(const void* restrict buffer, size_t size, size_t count, FILE* restrict file);
int fseek(FILE* file, long offset, int whence);
long ftell(FILE* file);
int fflush(FILE* file);
int feof(FILE* file);
int setvbuf(FILE* restrict file, char* restrict buffer, int mode, size_t size);
int fgetc(FILE* file);
char* fgets(char* restrict str, int size, FILE* restrict file);
int fputc(int c, FILE* file);
int fputs(const char* restrict str, FILE* restrict file);
int puts(const char* str);
int remove(const char* path);
int rename(const char* old_path, const char* new_path);
int getc(FILE* file);
// Puts back one character read from a buffered file
int ungetc(int c, FILE* file);
// There is no input, stdin is always at its end
int getchar(void);
int putchar(int c);

// Supports the d, i, u, x, X, o, p, s, c and % conversions with flags, width, precision and the
// hh, h, l, ll, z, j and t lengths, floats print as %f with float precision
int printf(const char* restrict format, ...) __attribute__((format(printf, 1, 2)));
int fprintf(FILE* restrict file, const char* restrict format, ...) __attribute__((format(printf, 2, 3)));
int sprintf(char* restrict buffer, const char* restrict format, ...) __attribute__((format(printf, 2, 3)));
int snprintf(char* restrict buffer, size_t size, const char* restrict format, ...)
    __attribute__((format(printf, 3, 4)));
int vprintf(const char* restrict format, va_list args);
int vfprintf(FILE* restrict file, const char* restrict format, va_list args);
int vsprintf(char* restrict buffer, const char* restrict format, va_list args);
int vsnprintf(char* restrict buffer, size_t size, const char* restrict format, va_list args);

// Supports the d, i, u, x, o, f, e, g, s, c, [, n and % conversions with widths, the * flag and
// the hh, h, l and ll lengths, floats with float precision
int sscanf(const char* restrict str, const char* restrict format, ...) __attribute__((format(scanf, 2, 3)));
int fscanf(FILE* restrict file, const char* restrict format, ...) __attribute__((format(scanf, 2, 3)));
int scanf(const char* restrict format, ...) __attribute__((format(scanf, 1, 2)));
int vsscanf(const char* restrict str, const char* restrict format, va_list args);
int vfscanf(FILE* restrict file, const char* restrict format, va_list args);

// newlib's names for its integer only printf and scanf functions
#define iprintf printf
#define fiprintf fprintf
#define siprintf sprintf
#define sniprintf snprintf
#define viprintf vprintf
#define vfiprintf vfprintf
#define vsiprintf vsprintf
#define vsniprintf vsnprintf
#define siscanf sscanf
#define vsiscanf vsscanf
#define fiscanf fscanf
