#pragma once

#include <limits.h>
#include <sys/iosupport.h>

#define DT_UNKNOWN 0
#define DT_DIR 4
#define DT_REG 8

struct dirent {
    ino_t d_ino;
    unsigned char d_type;
    char d_name[NAME_MAX + 1];
};

// The device's directory state follows the struct
typedef struct {
    const devoptab_t* device;
    DIR_ITER iterator;
    struct dirent entry;
} DIR;

DIR* opendir(const char* path);
struct dirent* readdir(DIR* dir);
int closedir(DIR* dir);
