#pragma once

#include <stdlib.h>

void* memalign(size_t alignment, size_t size);

// Memory outside the main RAM, like the RAM of a Memory Expansion Pak, as a far heap: far_malloc()
// takes from it first, then from the main RAM like malloc(), which never takes from it. free()
// and realloc() take both.
void far_heap_init(void* start, size_t size);
void* far_malloc(size_t size);
