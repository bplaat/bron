// The printf formatting: the d, i, u, x, X, o, p, s, c and % conversions with the -, 0, +, space
// and # flags, width, precision (also as *) and the hh, h, l, ll, z, j and t lengths. Floats are
// printed as %f with float precision, rounding halves up, the DS has no floating point unit.

#include <stdint.h>

#include "libc.h"

static void put(Output* out, char c) {
    out->count++;
    if (out->flush) {
        if (out->length == out->size)
            out->flush(out);
        out->buffer[out->length++] = c;
    } else if (out->length + 1 < out->size) {
        out->buffer[out->length++] = c;
    }
}

static void put_repeated(Output* out, char c, int count) {
    for (int i = 0; i < count; i++)
        put(out, c);
}

// Puts a prefix (sign or 0x), zeros and digits, padded to width
static void put_number(Output* out, const char* prefix, int zeros, const char* digits, int length, int width, bool left,
                       bool zero_pad) {
    int prefix_length = 0;
    while (prefix[prefix_length])
        prefix_length++;
    int padding = width - prefix_length - zeros - length;
    if (zero_pad && !left && padding > 0) {
        zeros += padding;
        padding = 0;
    }
    if (!left)
        put_repeated(out, ' ', padding);
    for (int i = 0; i < prefix_length; i++)
        put(out, prefix[i]);
    put_repeated(out, '0', zeros);
    for (int i = 0; i < length; i++)
        put(out, digits[i]);
    if (left)
        put_repeated(out, ' ', padding);
}

int __format(Output* out, const char* restrict format, va_list args) {
    for (const char* c = format; *c; c++) {
        if (*c != '%') {
            put(out, *c);
            continue;
        }
        c++;
        bool left = false, zero_pad = false, alternate = false;
        char sign = '\0';
        for (;; c++) {
            if (*c == '-')
                left = true;
            else if (*c == '0')
                zero_pad = true;
            else if (*c == '+')
                sign = '+';
            else if (*c == ' ' && !sign)
                sign = ' ';
            else if (*c == '#')
                alternate = true;
            else
                break;
        }
        int width = 0;
        if (*c == '*') {
            width = va_arg(args, int);
            if (width < 0) {
                left = true;
                width = -width;
            }
            c++;
        }
        for (; *c >= '0' && *c <= '9'; c++)
            width = width * 10 + (*c - '0');
        // Minimum digits for integers, digits after the point for floats, maximum characters for strings
        int precision = -1;
        if (*c == '.') {
            c++;
            precision = 0;
            if (*c == '*') {
                precision = va_arg(args, int);
                c++;
            }
            for (; *c >= '0' && *c <= '9'; c++)
                precision = precision * 10 + (*c - '0');
        }
        // Everything is 32 bits except long long and intmax_t, h and hh cut the value to 16 and 8 bits
        bool wide = false;
        int halves = 0;
        for (; *c == 'h' || *c == 'l' || *c == 'z' || *c == 'j' || *c == 't' || *c == 'L'; c++) {
            if ((c[0] == 'l' && c[1] == 'l') || *c == 'j')
                wide = true;
            halves += *c == 'h';
        }

        // 20 digits of a 64-bit integer, the point and 9 decimals
        char digits[32];
        char* end = digits + sizeof(digits);
        char* start = end;
        char prefix[3] = {0};
        switch (*c) {
            case 'd':
            case 'i':
            case 'u':
            case 'x':
            case 'X':
            case 'o':
            case 'p': {
                uint64_t value;
                if (*c == 'd' || *c == 'i') {
                    int64_t number = wide ? va_arg(args, int64_t) : va_arg(args, int32_t);
                    if (halves)
                        number = halves == 1 ? (int16_t)number : (int8_t)number;
                    value = number < 0 ? -(uint64_t)number : (uint64_t)number;
                    prefix[0] = number < 0 ? '-' : sign;
                } else if (*c == 'p') {
                    value = (uintptr_t)va_arg(args, void*);
                    alternate = true;
                } else {
                    value = wide ? va_arg(args, uint64_t) : va_arg(args, uint32_t);
                    if (halves)
                        value = halves == 1 ? (uint16_t)value : (uint8_t)value;
                }
                uint32_t base = *c == 'o' ? 8 : *c == 'x' || *c == 'X' || *c == 'p' ? 16 : 10;
                const char* hex = *c == 'X' ? "0123456789ABCDEF" : "0123456789abcdef";
                // Most numbers fit in 32 bits, which divides much faster
                for (; value > UINT32_MAX; value /= base)
                    *--start = hex[value % base];
                for (uint32_t small = (uint32_t)value; small; small /= base)
                    *--start = hex[small % base];
                if (alternate && base == 16 && start != end) {
                    prefix[0] = '0';
                    prefix[1] = *c == 'X' ? 'X' : 'x';
                }
                if (alternate && base == 8 && (start == end || *start != '0'))
                    *--start = '0';
                // A precision turns off zero padding
                int length = (int)(end - start);
                int zeros = precision < 0 ? (length == 0) : precision - length;
                put_number(out, prefix, zeros > 0 ? zeros : 0, start, length, width, left, zero_pad && precision < 0);
                break;
            }
            case 'f':
            case 'F':
            case 'e':
            case 'E':
            case 'g':
            case 'G': {
                float value = (float)va_arg(args, double);
                if (precision < 0)
                    precision = 6;
                if (precision > 9)
                    precision = 9;
                if (__builtin_signbit(value)) {
                    prefix[0] = '-';
                    value = -value;
                } else {
                    prefix[0] = sign;
                }
                // Up to 2^64, the biggest floats don't fit in the integer part
                if (value != value || value >= 18446744073709551616.0f) {
                    const char* text = value != value ? "nan" : "inf";
                    put_number(out, prefix, 0, text, 3, width, left, false);
                    break;
                }
                // Round, then print the integer part, the point and the fraction digits
                uint32_t scale = 1;
                for (int i = 0; i < precision; i++)
                    scale *= 10;
                uint64_t integer = (uint64_t)value;
                uint32_t fraction = (uint32_t)((value - (float)integer) * (float)scale + 0.5f);
                if (fraction >= scale) {
                    fraction -= scale;
                    integer++;
                }
                for (int i = 0; i < precision; i++, fraction /= 10)
                    *--start = (char)('0' + fraction % 10);
                if (precision > 0 || alternate)
                    *--start = '.';
                for (; integer > UINT32_MAX; integer /= 10)
                    *--start = (char)('0' + integer % 10);
                uint32_t small = (uint32_t)integer;
                do {
                    *--start = (char)('0' + small % 10);
                } while (small /= 10);
                put_number(out, prefix, 0, start, (int)(end - start), width, left, zero_pad);
                break;
            }
            case 's': {
                const char* str = va_arg(args, const char*);
                if (!str)
                    str = "(null)";
                int length = 0;
                while ((precision < 0 || length < precision) && str[length])
                    length++;
                put_number(out, "", 0, str, length, width, left, false);
                break;
            }
            case 'c':
                *--start = (char)va_arg(args, int);
                put_number(out, "", 0, start, 1, width, left, false);
                break;
            case '%':
                put(out, '%');
                break;
            default:
                // Unknown conversion or a lone % at the end, stop here
                goto done;
        }
    }
done:
    if (out->flush)
        out->flush(out);
    else if (out->size > 0)
        out->buffer[out->length] = '\0';
    return (int)out->count;
}

int vsnprintf(char* restrict buffer, size_t size, const char* restrict format, va_list args) {
    Output out = {.buffer = buffer, .size = size};
    return __format(&out, format, args);
}

int vsprintf(char* restrict buffer, const char* restrict format, va_list args) {
    return vsnprintf(buffer, SIZE_MAX, format, args);
}

int snprintf(char* restrict buffer, size_t size, const char* restrict format, ...) {
    va_list args;
    va_start(args, format);
    int length = vsnprintf(buffer, size, format, args);
    va_end(args);
    return length;
}

int sprintf(char* restrict buffer, const char* restrict format, ...) {
    va_list args;
    va_start(args, format);
    int length = vsnprintf(buffer, SIZE_MAX, format, args);
    va_end(args);
    return length;
}
