#pragma once

#include <alloca.h>
#include <stddef.h>

#define EXIT_SUCCESS 0
#define EXIT_FAILURE 1
#define RAND_MAX 0x7fffffff

void* malloc(size_t size);
void* calloc(size_t count, size_t size);
void* realloc(void* pointer, size_t size);
void* aligned_alloc(size_t alignment, size_t size);
void free(void* pointer);
int abs(int value);
long labs(long value);
int atoi(const char* str);
long atol(const char* str);
double atof(const char* str);
long strtol(const char* restrict str, char** restrict end, int base);
unsigned long strtoul(const char* restrict str, char** restrict end, int base);
// Decimal numbers with an exponent, inf and nan, in float precision, strtod and atof too
float strtof(const char* restrict str, char** restrict end);
double strtod(const char* restrict str, char** restrict end);
void qsort(void* base, size_t count, size_t size, int (*compare)(const void*, const void*));
void* bsearch(const void* key, const void* base, size_t count, size_t size, int (*compare)(const void*, const void*));
int rand(void);
void srand(unsigned seed);
// There is no environment, nothing can be added to it
static inline char* getenv(const char* name) {
    (void)name;
    return NULL;
}
static inline int putenv(char* string) {
    (void)string;
    return -1;
}
static inline int setenv(const char* name, const char* value, int overwrite) {
    (void)name, (void)value, (void)overwrite;
    return -1;
}
int atexit(void (*function)(void));
void exit(int status) __attribute__((noreturn));
void abort(void) __attribute__((noreturn));
