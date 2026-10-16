// Everything the C library uses from calico, the same hooks devkitARM's newlib uses. The other
// way around calico's startup code calls build_argv, __libc_init_array and exit from crt.c and
// sets fake_heap_start and fake_heap_end from malloc.c, libdvm and libnds plug their devices into
// the devoptab of io.c.

#pragma once

#include <calico/arm/common.h>
#include <calico/nds/env.h>
#include <calico/system/tick.h>
#include <sys/reent.h>
#include <sys/time.h>

// Leaves the program, back to the loader
void __syscall_exit(int status) __attribute__((noreturn));
// Puts the current thread to sleep
int __syscall_nanosleep(const struct timespec* request, struct timespec* remaining);
// The time of the DS clock, which has no time zone
int __syscall_gettod_r(struct _reent* r, struct timeval* time, struct timezone* zone);
