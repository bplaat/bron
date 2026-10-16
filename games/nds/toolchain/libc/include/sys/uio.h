#pragma once

#include <sys/types.h>

struct iovec {
    void* iov_base;
    size_t iov_len;
};

ssize_t readv(int fd, const struct iovec* vector, int count);
ssize_t writev(int fd, const struct iovec* vector, int count);
