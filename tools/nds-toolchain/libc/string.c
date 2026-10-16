#include <ctype.h>
#include <errno.h>
#include <stdlib.h>
#include <string.h>

void* memchr(const void* str, int c, size_t size) {
    const unsigned char* s = str;
    for (size_t i = 0; i < size; i++) {
        if (s[i] == (unsigned char)c)
            return (void*)(s + i);
    }
    return NULL;
}

size_t strlen(const char* str) {
    size_t length = 0;
    while (str[length])
        length++;
    return length;
}

size_t strnlen(const char* str, size_t max) {
    size_t length = 0;
    while (length < max && str[length])
        length++;
    return length;
}

int strcmp(const char* a, const char* b) {
    while (*a && *a == *b)
        a++, b++;
    return (unsigned char)*a - (unsigned char)*b;
}

int strncmp(const char* a, const char* b, size_t max) {
    for (size_t i = 0; i < max; i++) {
        if (a[i] != b[i] || !a[i])
            return (unsigned char)a[i] - (unsigned char)b[i];
    }
    return 0;
}

int strcasecmp(const char* a, const char* b) {
    return strncasecmp(a, b, (size_t)-1);
}

int strncasecmp(const char* a, const char* b, size_t max) {
    for (size_t i = 0; i < max; i++) {
        int x = tolower((unsigned char)a[i]), y = tolower((unsigned char)b[i]);
        if (x != y || !x)
            return x - y;
    }
    return 0;
}

char* strcpy(char* restrict dst, const char* restrict src) {
    char* d = dst;
    while ((*d++ = *src++))
        ;
    return dst;
}

char* strncpy(char* restrict dst, const char* restrict src, size_t max) {
    size_t i = 0;
    for (; i < max && src[i]; i++)
        dst[i] = src[i];
    for (; i < max; i++)
        dst[i] = '\0';
    return dst;
}

char* strcat(char* restrict dst, const char* restrict src) {
    strcpy(dst + strlen(dst), src);
    return dst;
}

char* strncat(char* restrict dst, const char* restrict src, size_t max) {
    char* d = dst + strlen(dst);
    for (; max && *src; max--)
        *d++ = *src++;
    *d = '\0';
    return dst;
}

char* strchr(const char* str, int c) {
    for (;; str++) {
        if (*str == (char)c)
            return (char*)str;
        if (!*str)
            return NULL;
    }
}

char* strrchr(const char* str, int c) {
    const char* last = NULL;
    for (;; str++) {
        if (*str == (char)c)
            last = str;
        if (!*str)
            return (char*)last;
    }
}

char* strstr(const char* str, const char* find) {
    size_t length = strlen(find);
    for (; *str; str++) {
        if (!strncmp(str, find, length))
            return (char*)str;
    }
    return length ? NULL : (char*)str;
}

size_t strspn(const char* str, const char* chars) {
    size_t length = 0;
    while (str[length] && strchr(chars, str[length]))
        length++;
    return length;
}

size_t strcspn(const char* str, const char* chars) {
    size_t length = 0;
    while (str[length] && !strchr(chars, str[length]))
        length++;
    return length;
}

char* strtok(char* restrict str, const char* restrict separators) {
    static char* next;
    if (!str)
        str = next;
    if (!str)
        return NULL;
    str += strspn(str, separators);
    if (!*str) {
        next = NULL;
        return NULL;
    }
    char* end = str + strcspn(str, separators);
    next = *end ? end + 1 : NULL;
    *end = '\0';
    return str;
}

char* strdup(const char* str) {
    size_t size = strlen(str) + 1;
    char* copy = malloc(size);
    return copy ? memcpy(copy, str, size) : NULL;
}

char* strerror(int error) {
    switch (error) {
        case 0:
            return "Success";
        case ENOENT:
            return "No such file or directory";
        case EIO:
            return "I/O error";
        case EBADF:
            return "Bad file number";
        case ENOMEM:
            return "Not enough memory";
        case EACCES:
            return "Permission denied";
        case EEXIST:
            return "File exists";
        case ENODEV:
            return "No such device";
        case ENOTDIR:
            return "Not a directory";
        case EISDIR:
            return "Is a directory";
        case EINVAL:
            return "Invalid argument";
        case EMFILE:
            return "Too many open files";
        case ENOSPC:
            return "No space left on device";
        case EROFS:
            return "Read-only file system";
        case ENOSYS:
            return "Function not implemented";
        default:
            return "Unknown error";
    }
}
