// A small scanf family on strings and files: the d, i, u, x, o, f, e, g, s, c, [, n and %
// conversions with widths, the * flag and the hh, h, l and ll lengths, spaces in the format skip
// any spaces. Floats are read with strtof, also into doubles with l.

#include <ctype.h>
#include <stdarg.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

// Where the characters come from: a string, or a file with one character of look ahead
typedef struct {
    const char* str;
    FILE* file;
    int next;  // the look ahead of a file, EOF at its end, -2 before it is read
    int count;
} Input;

static int peek(Input* in) {
    if (!in->file)
        return *in->str ? (unsigned char)*in->str : EOF;
    if (in->next == -2)
        in->next = fgetc(in->file);
    return in->next;
}

static void advance(Input* in) {
    if (!in->file)
        in->str++;
    else
        in->next = -2;
    in->count++;
}

// Whether c is in a scanset like "^a-z]": set starts after its [, end is at its ]
static bool in_scanset(const char* set, const char* end, int c) {
    bool negated = *set == '^';
    set += negated;
    bool found = false;
    for (const char* p = set; p < end; p++) {
        if (p[1] == '-' && p + 2 < end) {
            found |= c >= (unsigned char)p[0] && c <= (unsigned char)p[2];
            p += 2;
        } else {
            found |= c == (unsigned char)*p;
        }
    }
    return found != negated;
}

// Whether c can continue the text of a number read so far, scanf reads as much as could be one
static bool number_char(const char* text, int length, int c, bool floating, int base) {
    char last = length ? text[length - 1] : 0;
    if (c == '+' || c == '-')
        return !length || (floating && (last | 32) == 'e');
    if (floating) {
        // Letters after the sign only spell inf, infinity or nan
        int sign = length && (text[0] == '+' || text[0] == '-'), letters = length - sign;
        if (letters ? isalpha((unsigned char)text[sign]) : isalpha(c)) {
            const char* name = (letters ? text[sign] | 32 : c | 32) == 'n' ? "nan" : "infinity";
            return letters < (int)strlen(name) && (c | 32) == name[letters];
        }
        if (c == '.')
            return !memchr(text, '.', length) && !memchr(text, 'e', length) && !memchr(text, 'E', length);
        // The exponent comes after a digit, maybe followed by the point
        if ((c | 32) == 'e')
            return !memchr(text, 'e', length) && !memchr(text, 'E', length) &&
                   (isdigit((unsigned char)last) ||
                    (last == '.' && length > 1 && isdigit((unsigned char)text[length - 2])));
        return isdigit(c);
    }
    if (isdigit(c))
        return base != 8 || c < '8';
    // 0x after an optional sign
    if ((c | 32) == 'x')
        return (base == 16 || base == 0) && last == '0' && length == 1 + !isdigit((unsigned char)text[0]);
    return (base == 16 || (base == 0 && memchr(text, 'x', length))) && isxdigit(c);
}

static int vscan(Input* in, const char* format, va_list args) {
    int assigned = 0;
    for (const char* c = format; *c; c++) {
        if (isspace((unsigned char)*c)) {
            while (isspace(peek(in)))
                advance(in);
            continue;
        }
        if (*c != '%' || c[1] == '%') {
            c += *c == '%';
            if (peek(in) != (unsigned char)*c)
                break;
            advance(in);
            continue;
        }
        c++;
        bool skip = *c == '*';
        c += skip;
        int width = 0;
        for (; isdigit((unsigned char)*c); c++)
            width = width * 10 + (*c - '0');
        int size = 0;  // -2 char, -1 short, 1 long, 2 long long
        for (; *c == 'h' || *c == 'l'; c++)
            size += *c == 'h' ? -1 : 1;
        if (*c == 'n') {
            if (!skip)
                *va_arg(args, int*) = in->count;
            continue;
        }
        if (*c != 'c' && *c != '[') {
            while (isspace(peek(in)))
                advance(in);
        }
        if (peek(in) == EOF) {
            if (!assigned)
                assigned = EOF;
            break;
        }

        if (*c == '[' || *c == 's' || *c == 'c') {
            // A ] right after the [ or ^ belongs to the set
            const char *set = c + 1, *end = NULL;
            if (*c == '[') {
                end = set + (*set == '^');
                end += *end == ']';
                end = strchr(end, ']');
                if (!end)
                    break;
                c = end;
            }
            if (!width)
                width = *c == 'c' ? 1 : INT32_MAX;
            char* out = skip ? NULL : va_arg(args, char*);
            int length = 0;
            for (int ch; length < width && (ch = peek(in)) != EOF; length++) {
                if (end ? !in_scanset(set, end, ch) : *c == 's' && isspace(ch))
                    break;
                if (out)
                    out[length] = (char)ch;
                advance(in);
            }
            if (!length)
                break;
            if (out && *c != 'c')
                out[length] = '\0';
        } else if (strchr("diuxXofeEgG", *c)) {
            bool floating = strchr("feEgG", *c) != NULL;
            int base = *c == 'x' || *c == 'X' ? 16 : *c == 'o' ? 8 : *c == 'i' ? 0 : 10;
            char text[40];
            int length = 0;
            if (!width || width > (int)sizeof(text) - 1)
                width = sizeof(text) - 1;
            for (int ch; length < width && (ch = peek(in)) != EOF && number_char(text, length, ch, floating, base);) {
                text[length++] = (char)ch;
                advance(in);
            }
            text[length] = '\0';
            char* end;
            if (floating) {
                float value = strtof(text, &end);
                if (end == text)
                    break;
                if (!skip) {
                    if (size >= 1)
                        *va_arg(args, double*) = value;
                    else
                        *va_arg(args, float*) = value;
                }
            } else {
                long value = *c == 'd' || *c == 'i' ? strtol(text, &end, base) : (long)strtoul(text, &end, base);
                if (end == text)
                    break;
                if (!skip) {
                    void* out = va_arg(args, void*);
                    if (size <= -2)
                        *(char*)out = (char)value;
                    else if (size == -1)
                        *(short*)out = (short)value;
                    else if (size >= 2)
                        *(long long*)out = value;
                    else
                        *(int*)out = (int)value;
                }
            }
        } else {
            break;
        }
        assigned += !skip;
    }
    // A file keeps the character only looked at
    if (in->file && in->next >= 0)
        ungetc(in->next, in->file);
    return assigned;
}

int vsscanf(const char* restrict str, const char* restrict format, va_list args) {
    Input in = {.str = str};
    return vscan(&in, format, args);
}

int sscanf(const char* restrict str, const char* restrict format, ...) {
    va_list args;
    va_start(args, format);
    int assigned = vsscanf(str, format, args);
    va_end(args);
    return assigned;
}

int vfscanf(FILE* restrict file, const char* restrict format, va_list args) {
    Input in = {.file = file, .next = -2};
    return vscan(&in, format, args);
}

int fscanf(FILE* restrict file, const char* restrict format, ...) {
    va_list args;
    va_start(args, format);
    int assigned = vfscanf(file, format, args);
    va_end(args);
    return assigned;
}

int scanf(const char* restrict format, ...) {
    va_list args;
    va_start(args, format);
    int assigned = vfscanf(stdin, format, args);
    va_end(args);
    return assigned;
}
