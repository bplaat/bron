// A simple first-fit heap that grows with sbrk() into the free main RAM, by just what is needed
// as memory is tight: a game can malloc() all that is left after sbrk(0). Memory outside the main
// RAM, like the RAM of a Memory Expansion Pak, is a far heap of its own for far_malloc().

#include <errno.h>
#include <malloc.h>
#include <stdint.h>
#include <string.h>
#include <unistd.h>

#include "calico.h"

// calico's startup code sets the free main RAM
void* fake_heap_start;
void* fake_heap_end;
static char* heap_break;

void* sbrk(ptrdiff_t increment) {
    if (!heap_break)
        heap_break = fake_heap_start;
    if (increment > (char*)fake_heap_end - heap_break || increment < (char*)fake_heap_start - heap_break) {
        errno = ENOMEM;
        return (void*)-1;
    }
    char* previous = heap_break;
    heap_break += increment;
    return previous;
}

// Free chunks form a linked list sorted by address
typedef struct Chunk {
    size_t size;
    struct Chunk* next;
} Chunk;

// Stored just before every pointer we hand out
typedef struct {
    uintptr_t start;
    size_t size;
} Header;

static Chunk* free_list = NULL;
static Chunk* far_free_list = NULL;
static uintptr_t far_start, far_end;

static uintptr_t align_up(uintptr_t value, size_t alignment) {
    return (value + alignment - 1) & ~(uintptr_t)(alignment - 1);
}

static bool is_far(uintptr_t start) {
    return start >= far_start && start < far_end;
}

static void insert_chunk(uintptr_t start, size_t size) {
    Chunk** link = is_far(start) ? &far_free_list : &free_list;
    Chunk** list = link;
    while (*link && (uintptr_t)*link < start)
        link = &(*link)->next;

    Chunk* chunk = (Chunk*)start;
    chunk->size = size;
    chunk->next = *link;
    if (chunk->next && start + size == (uintptr_t)chunk->next) {
        chunk->size += chunk->next->size;
        chunk->next = chunk->next->next;
    }
    *link = chunk;

    // Merge with the previous chunk when they touch
    if (link != list) {
        Chunk* previous = (Chunk*)((uintptr_t)link - offsetof(Chunk, next));
        if ((uintptr_t)previous + previous->size == start) {
            previous->size += chunk->size;
            previous->next = chunk->next;
        }
    }
}

// Grows the heap, the free chunk at its end counts as part of what is needed
static bool grow(size_t size) {
    uintptr_t start = (uintptr_t)sbrk(0);
    Chunk* last = free_list;
    while (last && last->next)
        last = last->next;
    if (last && (uintptr_t)last + last->size == start)
        size = size > last->size ? size - last->size : 8;
    size = align_up(start + size, 8) - start;
    if (sbrk((ptrdiff_t)size) == (void*)-1)
        return false;
    insert_chunk(start, size);
    return true;
}

// Takes size bytes from the first free chunk of a list that has room, aligned
static void* take(Chunk** list, size_t alignment, size_t size) {
    for (Chunk** link = list; *link; link = &(*link)->next) {
        uintptr_t start = (uintptr_t)*link;
        size_t chunk_size = (*link)->size;
        uintptr_t pointer = align_up(start + sizeof(Header), alignment);
        uintptr_t end = pointer + size;
        if (end > start + chunk_size)
            continue;

        // Keep the leftover as a free chunk when it is big enough
        *link = (*link)->next;
        uintptr_t chunk_end = start + chunk_size;
        if (chunk_end - end >= 32)
            insert_chunk(end, chunk_end - end);
        else
            end = chunk_end;

        Header* header = (Header*)(pointer - sizeof(Header));
        header->start = start;
        header->size = end - start;
        return (void*)pointer;
    }
    return NULL;
}

static void* allocate(bool far, size_t alignment, size_t size) {
    // Bigger than all memory, also keeps the size calculations below from overflowing
    if (size > 0x10000000 || alignment > 0x10000000 || (alignment & (alignment - 1)) != 0)
        return NULL;
    if (alignment < 8)
        alignment = 8;
    size = align_up(size < sizeof(Chunk) ? sizeof(Chunk) : size, 8);
    void* pointer = far ? take(&far_free_list, alignment, size) : NULL;
    if (!pointer)
        pointer = take(&free_list, alignment, size);
    // The heap grows at its end, where a free chunk may already be
    if (!pointer && grow(size + alignment + sizeof(Header)))
        pointer = take(&free_list, alignment, size);
    return pointer;
}

void* memalign(size_t alignment, size_t size) {
    ArmIrqState state = armIrqLockByPsr();
    void* pointer = allocate(false, alignment, size);
    armIrqUnlockByPsr(state);
    return pointer;
}

void far_heap_init(void* start, size_t size) {
    ArmIrqState state = armIrqLockByPsr();
    far_start = align_up((uintptr_t)start, 8);
    far_end = ((uintptr_t)start + size) & ~(uintptr_t)7;
    insert_chunk(far_start, far_end - far_start);
    armIrqUnlockByPsr(state);
}

void* far_malloc(size_t size) {
    ArmIrqState state = armIrqLockByPsr();
    void* pointer = allocate(true, 8, size);
    armIrqUnlockByPsr(state);
    return pointer;
}

void* aligned_alloc(size_t alignment, size_t size) {
    return memalign(alignment, size);
}

void* malloc(size_t size) {
    return memalign(8, size);
}

void* calloc(size_t count, size_t size) {
    if (size != 0 && count > SIZE_MAX / size)
        return NULL;
    void* pointer = malloc(count * size);
    if (pointer)
        memset(pointer, 0, count * size);
    return pointer;
}

void free(void* pointer) {
    if (!pointer)
        return;
    Header* header = (Header*)((uintptr_t)pointer - sizeof(Header));
    ArmIrqState state = armIrqLockByPsr();
    insert_chunk(header->start, header->size);
    armIrqUnlockByPsr(state);
}

// Shrinks in place, giving the end back when it is big enough, and grows in the same heap
void* realloc(void* pointer, size_t size) {
    if (!pointer)
        return malloc(size);
    Header* header = (Header*)((uintptr_t)pointer - sizeof(Header));
    uintptr_t end = header->start + header->size;
    size_t old_size = end - (uintptr_t)pointer;
    if (size <= old_size) {
        uintptr_t new_end = align_up((uintptr_t)pointer + (size < sizeof(Chunk) ? sizeof(Chunk) : size), 8);
        if (end - new_end >= 32) {
            ArmIrqState state = armIrqLockByPsr();
            header->size = new_end - header->start;
            insert_chunk(new_end, end - new_end);
            armIrqUnlockByPsr(state);
        }
        return pointer;
    }
    void* new_pointer = is_far(header->start) ? far_malloc(size) : malloc(size);
    if (new_pointer) {
        memcpy(new_pointer, pointer, old_size);
        free(pointer);
    }
    return new_pointer;
}
