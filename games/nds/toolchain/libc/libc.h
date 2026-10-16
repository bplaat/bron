// Internals shared by the libc sources

#pragma once

#include <stdarg.h>
#include <stddef.h>
#include <stdio.h>

// Where __format() puts its characters: a string of size bytes, or when flush is set a buffer of
// size bytes that flush() empties into file
typedef struct Output {
    char* buffer;
    size_t size;
    size_t length;
    size_t count;
    void (*flush)(struct Output* out);
    FILE* file;
} Output;

int __format(Output* out, const char* restrict format, va_list args);
