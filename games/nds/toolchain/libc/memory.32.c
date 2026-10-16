// memcpy, memmove, memset and memcmp as ARM code, which the ARM9 runs from ITCM: they move
// words, eight at a time, or halfwords when only those line up. VRAM and the RAM of the GBA slot,
// like a Memory Expansion Pak's, ignore byte writes, so the bytes at the ends of a copy or fill
// are written with their halfword there.

#include <stdint.h>
#include <string.h>

typedef uint32_t __attribute__((may_alias)) word;
typedef uint16_t __attribute__((may_alias)) half;

// A byte in main RAM, or with the other byte of its halfword in VRAM and the GBA slot
static inline void put_byte(uint8_t* p, uint8_t value) {
    if ((uintptr_t)p < 0x04000000) {
        *p = value;
        return;
    }
    // volatile, else the compiler sees it's a byte and stores that
    volatile half* h = (volatile half*)((uintptr_t)p & ~1u);
    *h = (uintptr_t)p & 1 ? (half)((*h & 0x00ff) | value << 8) : (half)((*h & 0xff00) | value);
}

void* memcpy(void* restrict dst, const void* restrict src, size_t size) {
    uint8_t* d = dst;
    const uint8_t* s = src;
    if ((((uintptr_t)d ^ (uintptr_t)s) & 1) == 0) {
        if ((uintptr_t)d & 1 && size) {
            put_byte(d++, *s++);
            size--;
        }
        if ((((uintptr_t)d ^ (uintptr_t)s) & 3) == 0) {
            if ((uintptr_t)d & 2 && size >= 2) {
                *(half*)d = *(const half*)s;
                d += 2, s += 2, size -= 2;
            }
            word* dw = (word*)d;
            const word* sw = (const word*)s;
            for (; size >= 32; size -= 32, dw += 8, sw += 8) {
                word a = sw[0], b = sw[1], c = sw[2], e = sw[3], f = sw[4], g = sw[5], h = sw[6], i = sw[7];
                dw[0] = a, dw[1] = b, dw[2] = c, dw[3] = e, dw[4] = f, dw[5] = g, dw[6] = h, dw[7] = i;
            }
            for (; size >= 4; size -= 4)
                *dw++ = *sw++;
            d = (uint8_t*)dw;
            s = (const uint8_t*)sw;
        }
        for (; size >= 2; size -= 2, d += 2, s += 2)
            *(half*)d = *(const half*)s;
    } else {
        // Only one of them is odd: the halfwords are put together from bytes
        if ((uintptr_t)d & 1 && size) {
            put_byte(d++, *s++);
            size--;
        }
        for (; size >= 2; size -= 2, d += 2, s += 2)
            *(half*)d = (half)(s[0] | s[1] << 8);
    }
    while (size--)
        put_byte(d++, *s++);
    return dst;
}

// Copies from the end down, for when the destination overlaps the end of the source
void* memmove(void* dst, const void* src, size_t size) {
    if ((uintptr_t)dst <= (uintptr_t)src || (uintptr_t)dst >= (uintptr_t)src + size)
        return memcpy(dst, src, size);
    uint8_t* d = (uint8_t*)dst + size;
    const uint8_t* s = (const uint8_t*)src + size;
    if ((((uintptr_t)d ^ (uintptr_t)s) & 1) == 0) {
        if ((uintptr_t)d & 1 && size) {
            put_byte(--d, *--s);
            size--;
        }
        if ((((uintptr_t)d ^ (uintptr_t)s) & 3) == 0) {
            if ((uintptr_t)d & 2 && size >= 2) {
                d -= 2, s -= 2, size -= 2;
                *(half*)d = *(const half*)s;
            }
            word* dw = (word*)d;
            const word* sw = (const word*)s;
            for (; size >= 4; size -= 4)
                *--dw = *--sw;
            d = (uint8_t*)dw;
            s = (const uint8_t*)sw;
        }
        for (; size >= 2; size -= 2) {
            d -= 2, s -= 2;
            *(half*)d = *(const half*)s;
        }
    } else {
        if ((uintptr_t)d & 1 && size) {
            put_byte(--d, *--s);
            size--;
        }
        for (; size >= 2; size -= 2) {
            d -= 2, s -= 2;
            *(half*)d = (half)(s[0] | s[1] << 8);
        }
    }
    while (size--) {
        --d;
        put_byte(d, *--s);
    }
    return dst;
}

void* memset(void* dst, int value, size_t size) {
    uint8_t* d = dst;
    word fill = (uint8_t)value * 0x01010101u;
    if ((uintptr_t)d & 1 && size) {
        put_byte(d++, (uint8_t)value);
        size--;
    }
    if ((uintptr_t)d & 2 && size >= 2) {
        *(half*)d = (half)fill;
        d += 2, size -= 2;
    }
    word* dw = (word*)d;
    if (size >= 32) {
        // Two stm of four registers store 32 bytes, the compiler would keep the fill in one register
        // and store a word at a time. The registers are outputs too, so they aren't set again for
        // every loop
        register word a __asm__("r4") = fill, b __asm__("r5") = fill, c __asm__("r6") = fill,
                      e __asm__("r7") = fill;
        for (; size >= 32; size -= 32)
            __asm__ volatile("stmia %0!, {%1, %2, %3, %4}\n\tstmia %0!, {%1, %2, %3, %4}"
                             : "+r"(dw), "+r"(a), "+r"(b), "+r"(c), "+r"(e)
                             :
                             : "memory");
    }
    for (; size >= 4; size -= 4)
        *dw++ = fill;
    d = (uint8_t*)dw;
    if (size >= 2) {
        *(half*)d = (half)fill;
        d += 2, size -= 2;
    }
    if (size)
        put_byte(d, (uint8_t)value);
    return dst;
}

// Equal words are skipped four bytes at a time when both line up, the bytes find the difference
int memcmp(const void* a, const void* b, size_t size) {
    const uint8_t *x = a, *y = b;
    if ((((uintptr_t)x | (uintptr_t)y) & 3) == 0) {
        for (; size >= 4 && *(const word*)x == *(const word*)y; size -= 4)
            x += 4, y += 4;
    }
    for (; size; size--, x++, y++) {
        if (*x != *y)
            return *x - *y;
    }
    return 0;
}
