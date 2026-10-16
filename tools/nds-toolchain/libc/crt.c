// C runtime startup and exit, calico's startup code calls these

#include <assert.h>
#include <errno.h>
#include <stdint.h>
#include <stdlib.h>

#include "calico.h"

int errno;

// Splits the null separated argument string from the loader into argv, which follows it. The
// ARM7 startup code calls this from the crt0 section, before the rest of the program is loaded,
// so it calls nothing.
void build_argv(EnvNdsArgvHeader* header) {
    char* arg = header->args_str;
    char* end = arg + header->args_str_size;
    char** argv = (char**)(((uintptr_t)end + sizeof(char*)) & ~(uintptr_t)(sizeof(char*) - 1));
    int argc = 0;
    while (arg < end) {
        argv[argc++] = arg;
        while (arg < end && *arg)
            arg++;
        arg++;
    }
    if (end > header->args_str)
        end[-1] = '\0';
    header->argc = argc;
    header->argv = argv;
    header->argv_end = &argv[argc];
}

// Runs the constructors, before main()
extern void (*__preinit_array_start[])(void), (*__preinit_array_end[])(void);
extern void (*__init_array_start[])(void), (*__init_array_end[])(void);

void __libc_init_array(void) {
    for (void (**function)(void) = __preinit_array_start; function < __preinit_array_end; function++)
        (*function)();
    for (void (**function)(void) = __init_array_start; function < __init_array_end; function++)
        (*function)();
}

#define MAX_EXIT_FUNCTIONS 32

static void (*exit_functions[MAX_EXIT_FUNCTIONS])(void);
static int exit_function_count;

int atexit(void (*function)(void)) {
    if (exit_function_count == MAX_EXIT_FUNCTIONS)
        return -1;
    exit_functions[exit_function_count++] = function;
    return 0;
}

// Flushes the open files, when the program uses them
extern void __stdio_exit(void) __attribute__((weak));

// main() returns here too
void exit(int status) {
    while (exit_function_count > 0)
        exit_functions[--exit_function_count]();
    if (__stdio_exit)
        __stdio_exit();
    __syscall_exit(status);
}

// Crashes with a trap, so an exception handler like libnds' shows where
void abort(void) {
    __builtin_trap();
}

// A failed assertion, programs can show it with their own
__attribute__((weak)) void __assert_func(const char* file, int line, const char* function, const char* expression) {
    (void)file, (void)line, (void)function, (void)expression;
    abort();
}
