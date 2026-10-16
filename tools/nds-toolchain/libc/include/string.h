#pragma once

#include <stddef.h>

void* memcpy(void* restrict dst, const void* restrict src, size_t size);
void* memmove(void* dst, const void* src, size_t size);
void* memset(void* dst, int value, size_t size);
int memcmp(const void* a, const void* b, size_t size);
void* memchr(const void* str, int c, size_t size);
size_t strlen(const char* str);
size_t strnlen(const char* str, size_t max);
int strcmp(const char* a, const char* b);
int strncmp(const char* a, const char* b, size_t max);
int strcasecmp(const char* a, const char* b);
int strncasecmp(const char* a, const char* b, size_t max);
char* strcpy(char* restrict dst, const char* restrict src);
char* strncpy(char* restrict dst, const char* restrict src, size_t max);
char* strcat(char* restrict dst, const char* restrict src);
char* strncat(char* restrict dst, const char* restrict src, size_t max);
char* strchr(const char* str, int c);
char* strrchr(const char* str, int c);
char* strstr(const char* str, const char* find);
size_t strspn(const char* str, const char* chars);
size_t strcspn(const char* str, const char* chars);
char* strtok(char* restrict str, const char* restrict separators);
char* strdup(const char* str);
char* strerror(int error);
