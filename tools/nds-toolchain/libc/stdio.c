// Buffered streams on the file descriptors, the standard streams are unbuffered

#include <fcntl.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <unistd.h>

#include "libc.h"

#define FILE_READ 1
#define FILE_WRITE 2
#define FILE_EOF 4
#define FILE_ERROR 8
#define FILE_APPEND 16

struct FILE {
    int fd;
    int flags;
    // Bytes buffered for writing, or bytes read ahead and the read position in them
    size_t size;
    size_t length;
    size_t position;
    bool writing;
    FILE* next;
    char buffer[];
};

static FILE standard_streams[] = {
    {.fd = 0, .flags = FILE_READ},
    {.fd = 1, .flags = FILE_WRITE},
    {.fd = 2, .flags = FILE_WRITE},
};
FILE* const stdin = &standard_streams[0];
FILE* const stdout = &standard_streams[1];
FILE* const stderr = &standard_streams[2];

// The open files, so exit() can flush them
static FILE* files = NULL;

FILE* fopen(const char* restrict path, const char* restrict mode) {
    int flags;
    if (mode[0] == 'r')
        flags = O_RDONLY;
    else if (mode[0] == 'w')
        flags = O_WRONLY | O_CREAT | O_TRUNC;
    else if (mode[0] == 'a')
        flags = O_WRONLY | O_CREAT | O_APPEND;
    else
        return NULL;
    if (strchr(mode, '+'))
        flags = (flags & ~O_ACCMODE) | O_RDWR;

    FILE* file = malloc(sizeof(FILE) + BUFSIZ);
    if (!file)
        return NULL;
    int fd = open(path, flags);
    if (fd < 0) {
        free(file);
        return NULL;
    }
    int access = flags & O_ACCMODE;
    *file = (FILE){
        .fd = fd,
        .flags = (access != O_WRONLY ? FILE_READ : 0) | (access != O_RDONLY ? FILE_WRITE : 0) |
                 (flags & O_APPEND ? FILE_APPEND : 0),
        .size = BUFSIZ,
        .next = files,
    };
    files = file;
    return file;
}

// Writes all bytes, in append mode always at the end of the file
static bool write_all(FILE* file, const char* buffer, size_t size) {
    if (size && file->flags & FILE_APPEND && lseek(file->fd, 0, SEEK_END) < 0)
        return false;
    for (size_t written = 0; written < size;) {
        ssize_t count = write(file->fd, buffer + written, size - written);
        if (count <= 0)
            return false;
        written += (size_t)count;
    }
    return true;
}

// Writes the buffered bytes, or forgets the bytes read ahead and moves the file position back
static int flush_buffer(FILE* file) {
    if (file->writing) {
        if (!write_all(file, file->buffer, file->length)) {
            file->flags |= FILE_ERROR;
            return EOF;
        }
        file->writing = false;
    } else if (file->position < file->length) {
        lseek(file->fd, -(off_t)(file->length - file->position), SEEK_CUR);
    }
    file->length = file->position = 0;
    return 0;
}

int fflush(FILE* file) {
    if (file)
        return flush_buffer(file);
    int value = 0;
    for (file = files; file; file = file->next) {
        if (flush_buffer(file))
            value = EOF;
    }
    return value;
}

void __stdio_exit(void) {
    fflush(NULL);
}

int fclose(FILE* file) {
    int value = flush_buffer(file);
    if (close(file->fd))
        value = EOF;
    for (FILE** link = &files; *link; link = &(*link)->next) {
        if (*link == file) {
            *link = file->next;
            break;
        }
    }
    free(file);
    return value;
}

size_t fread(void* restrict buffer, size_t size, size_t count, FILE* restrict file) {
    size_t total = size * count, done = 0;
    if (!total)
        return 0;
    if (!(file->flags & FILE_READ)) {
        file->flags |= FILE_ERROR;
        return 0;
    }
    if (file->writing && flush_buffer(file))
        return 0;
    char* b = buffer;
    while (done < total) {
        size_t buffered = file->length - file->position;
        if (buffered) {
            size_t part = total - done < buffered ? total - done : buffered;
            memcpy(b + done, file->buffer + file->position, part);
            file->position += part;
            done += part;
            continue;
        }
        // Big reads go straight to destinations in main RAM, the drivers may copy bytes, which
        // VRAM and the RAM of the GBA slot ignore
        bool direct = total - done >= file->size && (uintptr_t)b >> 24 == 0x02;
        ssize_t read_count = read(file->fd, direct ? b + done : file->buffer, direct ? total - done : file->size);
        if (read_count <= 0) {
            file->flags |= read_count == 0 ? FILE_EOF : FILE_ERROR;
            break;
        }
        if (direct) {
            done += (size_t)read_count;
        } else {
            file->length = (size_t)read_count;
            file->position = 0;
        }
    }
    return done / size;
}

size_t fwrite(const void* restrict buffer, size_t size, size_t count, FILE* restrict file) {
    size_t total = size * count;
    if (!total)
        return 0;
    if (!(file->flags & FILE_WRITE)) {
        file->flags |= FILE_ERROR;
        return 0;
    }
    if (!file->writing) {
        flush_buffer(file);
        file->writing = true;
    }
    // Unbuffered streams and big writes go straight to the file
    if (total >= file->size - file->length) {
        if (flush_buffer(file))
            return 0;
        file->writing = true;
        if (!write_all(file, buffer, total)) {
            file->flags |= FILE_ERROR;
            return 0;
        }
        return count;
    }
    memcpy(file->buffer + file->length, buffer, total);
    file->length += total;
    return count;
}

int fgetc(FILE* file) {
    unsigned char c;
    return fread(&c, 1, 1, file) ? c : EOF;
}

int getc(FILE* file) {
    return fgetc(file);
}

// Back into the buffer the character came from, or into the empty buffer of a file being read
int ungetc(int c, FILE* file) {
    if (c == EOF || file->writing || (file->position == 0 && (file->length || !file->size)))
        return EOF;
    if (file->position == 0)
        file->length = 1;
    else
        file->position--;
    file->buffer[file->position] = (char)c;
    file->flags &= ~FILE_EOF;
    return (unsigned char)c;
}

char* fgets(char* restrict str, int size, FILE* restrict file) {
    int length = 0;
    while (length + 1 < size) {
        int c = fgetc(file);
        if (c == EOF)
            break;
        str[length++] = (char)c;
        if (c == '\n')
            break;
    }
    if (length == 0 || size <= 0)
        return NULL;
    str[length] = '\0';
    return str;
}

int fputc(int c, FILE* file) {
    unsigned char byte = (unsigned char)c;
    return fwrite(&byte, 1, 1, file) ? byte : EOF;
}

int getchar(void) {
    return fgetc(stdin);
}

int putchar(int c) {
    return fputc(c, stdout);
}

int fputs(const char* restrict str, FILE* restrict file) {
    size_t length = strlen(str);
    return fwrite(str, 1, length, file) == length ? 0 : EOF;
}

int puts(const char* str) {
    return fputs(str, stdout) == EOF || fputc('\n', stdout) == EOF ? EOF : 0;
}

int fseek(FILE* file, long offset, int whence) {
    if (file->writing) {
        if (flush_buffer(file))
            return -1;
    } else {
        // The bytes read ahead are already past the position
        if (whence == SEEK_CUR)
            offset -= (long)(file->length - file->position);
        file->length = file->position = 0;
    }
    if (lseek(file->fd, offset, whence) < 0)
        return -1;
    file->flags &= ~FILE_EOF;
    return 0;
}

long ftell(FILE* file) {
    off_t position = lseek(file->fd, 0, SEEK_CUR);
    if (position < 0)
        return -1;
    if (file->writing)
        return (long)(position + (off_t)file->length);
    return (long)(position - (off_t)(file->length - file->position));
}

int feof(FILE* file) {
    return (file->flags & FILE_EOF) != 0;
}

// The standard streams are always unbuffered, files always have a buffer
int setvbuf(FILE* restrict file, char* restrict buffer, int mode, size_t size) {
    (void)file, (void)buffer, (void)mode, (void)size;
    return 0;
}

int remove(const char* path) {
    struct stat st;
    if (stat(path, &st) == 0 && S_ISDIR(st.st_mode))
        return rmdir(path);
    return unlink(path);
}

// Formats into a small buffer that is written to the file when full
static void flush_output(Output* out) {
    fwrite(out->buffer, 1, out->length, out->file);
    out->length = 0;
}

int vfprintf(FILE* restrict file, const char* restrict format, va_list args) {
    char buffer[64];
    Output out = {.buffer = buffer, .size = sizeof(buffer), .flush = flush_output, .file = file};
    return __format(&out, format, args);
}

int vprintf(const char* restrict format, va_list args) {
    return vfprintf(stdout, format, args);
}

int fprintf(FILE* restrict file, const char* restrict format, ...) {
    va_list args;
    va_start(args, format);
    int length = vfprintf(file, format, args);
    va_end(args);
    return length;
}

int printf(const char* restrict format, ...) {
    va_list args;
    va_start(args, format);
    int length = vfprintf(stdout, format, args);
    va_end(args);
    return length;
}
