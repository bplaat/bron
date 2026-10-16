#include <ctype.h>
#include <errno.h>
#include <limits.h>
#include <stdint.h>
#include <stdlib.h>

int abs(int value) {
    return value < 0 ? -value : value;
}

long labs(long value) {
    return value < 0 ? -value : value;
}

int atoi(const char* str) {
    return (int)strtol(str, NULL, 10);
}

long atol(const char* str) {
    return strtol(str, NULL, 10);
}

double atof(const char* str) {
    return strtod(str, NULL);
}

// Parses the digits after the sign, the base 0 means C notation (0x hex, 0 octal or decimal)
static unsigned long parse(const char* str, char** end, int base, bool* negative, bool* overflow) {
    const char* s = str;
    while (isspace((unsigned char)*s))
        s++;
    *negative = *s == '-';
    if (*s == '-' || *s == '+')
        s++;
    if ((base == 0 || base == 16) && s[0] == '0' && (s[1] | 32) == 'x' && isxdigit((unsigned char)s[2])) {
        s += 2;
        base = 16;
    } else if (base == 0) {
        base = s[0] == '0' ? 8 : 10;
    }

    unsigned long value = 0;
    const char* digits = s;
    *overflow = false;
    for (;; s++) {
        int digit;
        if (isdigit((unsigned char)*s))
            digit = *s - '0';
        else if (isalpha((unsigned char)*s))
            digit = (*s | 32) - 'a' + 10;
        else
            break;
        if (digit >= base)
            break;
        if (value > (ULONG_MAX - (unsigned)digit) / (unsigned)base)
            *overflow = true;
        value = value * base + digit;
    }
    if (end)
        *end = (char*)(s == digits ? str : s);
    return value;
}

long strtol(const char* restrict str, char** restrict end, int base) {
    bool negative, overflow;
    unsigned long value = parse(str, end, base, &negative, &overflow);
    if (overflow || value > (negative ? -(unsigned long)LONG_MIN : LONG_MAX)) {
        errno = ERANGE;
        return negative ? LONG_MIN : LONG_MAX;
    }
    return negative ? (long)-value : (long)value;
}

unsigned long strtoul(const char* restrict str, char** restrict end, int base) {
    bool negative, overflow;
    unsigned long value = parse(str, end, base, &negative, &overflow);
    if (overflow) {
        errno = ERANGE;
        return ULONG_MAX;
    }
    return negative ? -value : value;
}

// Whether str starts with word, ignoring case
static bool starts_with(const char* str, const char* word) {
    for (; *word; str++, word++) {
        if ((*str | 32) != *word)
            return false;
    }
    return true;
}

// The first 9 significant digits go into an integer, which is then scaled by the power of ten of
// the exponent. Up to 2^24 with a power up to 10^10 both are exact floats and the result is
// rounded right, the usual numbers. Others are scaled by the squares 10^1, 10^2, 10^4 up to 10^32,
// within 3 units in the last place. Dividing by them for negative exponents rounds better than
// multiplying by their inverses. All in single precision, the DS has no floating point unit.
float strtof(const char* restrict str, char** restrict end) {
    static const float powers[] = {1e1f, 1e2f, 1e4f, 1e8f, 1e16f, 1e32f};
    const char* s = str;
    while (isspace((unsigned char)*s))
        s++;
    bool negative = *s == '-';
    if (*s == '-' || *s == '+')
        s++;

    float value;
    if (starts_with(s, "inf") || starts_with(s, "nan")) {
        value = (*s | 32) == 'i' ? __builtin_inff() : __builtin_nanf("");
        s += (*s | 32) == 'i' && starts_with(s, "infinity") ? 8 : 3;
    } else {
        uint32_t mantissa = 0;
        int digits = 0, exponent = 0;
        const char* start = s;
        for (; isdigit((unsigned char)*s); s++) {
            if (digits < 9) {
                mantissa = mantissa * 10 + (*s - '0');
                digits += mantissa != 0;
            } else {
                exponent++;
            }
        }
        if (*s == '.') {
            for (s++; isdigit((unsigned char)*s); s++) {
                if (digits < 9) {
                    mantissa = mantissa * 10 + (*s - '0');
                    digits += mantissa != 0;
                    exponent--;
                }
            }
        }
        if (s == start || (s == start + 1 && *start == '.')) {
            if (end)
                *end = (char*)str;
            return 0;
        }
        if ((*s | 32) == 'e') {
            const char* e = s + 1;
            bool negative_exponent = *e == '-';
            if (*e == '-' || *e == '+')
                e++;
            if (isdigit((unsigned char)*e)) {
                int power = 0;
                for (; isdigit((unsigned char)*e); e++) {
                    if (power < 10000)
                        power = power * 10 + (*e - '0');
                }
                exponent += negative_exponent ? -power : power;
                s = e;
            }
        }

        value = (float)mantissa;
        unsigned scale = (unsigned)(exponent < 0 ? -exponent : exponent);
        if (mantissa < 1u << 24 && scale <= 10) {
            // The digits and the power of ten are exact floats, one rounding is the right one
            static const float exact[] = {1e0f, 1e1f, 1e2f, 1e3f, 1e4f, 1e5f, 1e6f, 1e7f, 1e8f, 1e9f, 1e10f};
            value = exponent < 0 ? value / exact[scale] : value * exact[scale];
            scale = 0;
        }
        for (int i = 0; scale && mantissa && i < (int)(sizeof(powers) / sizeof(powers[0])); i++, scale >>= 1) {
            if (scale & 1)
                value = exponent < 0 ? value / powers[i] : value * powers[i];
        }
        // Still more than 10^63 is out of range either way
        if (scale && mantissa)
            value = exponent < 0 ? 0 : __builtin_inff();
        if (mantissa && (value == 0 || __builtin_isinf(value)))
            errno = ERANGE;
    }
    if (end)
        *end = (char*)s;
    return negative ? -value : value;
}

// With float precision, which is all the DS works in
double strtod(const char* restrict str, char** restrict end) {
    return strtof(str, end);
}

// A word at a time when the elements are word aligned, like structs
static void swap(char* a, char* b, size_t size) {
    if ((((uintptr_t)a | (uintptr_t)b | size) & 3) == 0) {
        for (uint32_t *x = (uint32_t*)a, *y = (uint32_t*)b; size; size -= 4) {
            uint32_t c = *x;
            *x++ = *y;
            *y++ = c;
        }
        return;
    }
    while (size--) {
        char c = *a;
        *a++ = *b;
        *b++ = c;
    }
}

// Moves the element at root down the heap of count elements until its children are smaller
static void sift_down(char* base, size_t root, size_t count, size_t size, int (*compare)(const void*, const void*)) {
    for (size_t child; (child = 2 * root + 1) < count; root = child) {
        if (child + 1 < count && compare(base + child * size, base + (child + 1) * size) < 0)
            child++;
        if (compare(base + root * size, base + child * size) >= 0)
            return;
        swap(base + root * size, base + child * size, size);
    }
}

// Heapsort: in place, no recursion and O(n log n) for every input
void qsort(void* base, size_t count, size_t size, int (*compare)(const void*, const void*)) {
    char* b = base;
    for (size_t i = count / 2; i-- > 0;)
        sift_down(b, i, count, size, compare);
    for (size_t end = count; end-- > 1;) {
        swap(b, b + end * size, size);
        sift_down(b, 0, end, size, compare);
    }
}

void* bsearch(const void* key, const void* base, size_t count, size_t size, int (*compare)(const void*, const void*)) {
    const char* b = base;
    while (count) {
        const char* middle = b + (count / 2) * size;
        int order = compare(key, middle);
        if (order == 0)
            return (void*)middle;
        if (order > 0) {
            b = middle + size;
            count -= count / 2 + 1;
        } else {
            count /= 2;
        }
    }
    return NULL;
}

// xorshift32, the state may never be 0
static uint32_t random_state = 1;

int rand(void) {
    random_state ^= random_state << 13;
    random_state ^= random_state >> 17;
    random_state ^= random_state << 5;
    return (int)(random_state & RAND_MAX);
}

void srand(unsigned seed) {
    random_state = seed ? seed : 1;
}
