#pragma once

#include <stddef.h>

// Memory on the stack of the calling function, the compiler makes it
#define alloca(size) __builtin_alloca(size)
