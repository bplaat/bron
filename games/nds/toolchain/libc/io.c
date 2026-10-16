// The POSIX file functions on the devoptab devices: paths are made absolute with the current
// directory and go to the device named before their colon, file descriptors 0 to 2 write to the
// standard streams devoptab_list[STD_IN] to devoptab_list[STD_ERR].

#include <dirent.h>
#include <errno.h>
#include <fcntl.h>
#include <stdlib.h>
#include <string.h>
#include <sys/iosupport.h>
#include <sys/uio.h>
#include <unistd.h>

#define MAX_FILES 32

static const devoptab_t null_device = {.name = "stdnull"};

const devoptab_t* devoptab_list[STD_MAX] = {
    &null_device, &null_device, &null_device, &null_device, &null_device, &null_device, &null_device, &null_device,
    &null_device, &null_device, &null_device, &null_device, &null_device, &null_device, &null_device, &null_device,
};

static int default_device = 0;
static char current_directory[PATH_MAX];

typedef struct {
    const devoptab_t* device;
    void* file;
} Handle;

static Handle handles[MAX_FILES];

// The device name of a path ends with a colon, a digit may come before it
static size_t device_name_length(const char* path) {
    const char* colon = strchr(path, ':');
    const char* slash = strchr(path, '/');
    return colon && (!slash || colon < slash) ? (size_t)(colon - path) : 0;
}

int FindDevice(const char* name) {
    size_t length = device_name_length(name);
    if (!length)
        return default_device;
    for (int i = 0; i < STD_MAX; i++) {
        size_t device_length = strlen(devoptab_list[i]->name);
        if (!strncmp(devoptab_list[i]->name, name, device_length) &&
            (device_length == length ||
             (device_length + 1 == length && name[device_length] >= '0' && name[device_length] <= '9')))
            return i;
    }
    return -1;
}

const devoptab_t* GetDeviceOpTab(const char* name) {
    int device = FindDevice(name);
    return device >= 0 ? devoptab_list[device] : NULL;
}

int AddDevice(const devoptab_t* device) {
    for (int i = STD_ERR + 1; i < STD_MAX; i++) {
        if (!strcmp(devoptab_list[i]->name, device->name) || devoptab_list[i] == &null_device) {
            devoptab_list[i] = device;
            return i;
        }
    }
    return -1;
}

int RemoveDevice(const char* name) {
    int device = FindDevice(name);
    if (device <= STD_ERR)
        return -1;
    devoptab_list[device] = &null_device;
    if (default_device == device) {
        default_device = 0;
        current_directory[0] = '\0';
    }
    return 0;
}

// Appends a path to an absolute path, resolving the . and .. parts
static bool append_path(char* buffer, size_t size, const char* path) {
    size_t length = strlen(buffer);
    size_t root = device_name_length(buffer) + 2;
    while (*path) {
        size_t part = strcspn(path, "/");
        if (part == 2 && path[0] == '.' && path[1] == '.') {
            if (length > root) {
                length--;
                while (length > root && buffer[length - 1] != '/')
                    length--;
            }
        } else if (part > 0 && !(part == 1 && path[0] == '.')) {
            if (length + part + 2 > size)
                return false;
            if (buffer[length - 1] != '/')
                buffer[length++] = '/';
            memcpy(buffer + length, path, part);
            length += part;
        }
        path += part;
        while (*path == '/')
            path++;
    }
    buffer[length] = '\0';
    return true;
}

// Makes a path absolute: "<device>:/..." with the current directory, NULL when it doesn't fit
static const char* absolute_path(const char* path, char* buffer, size_t size) {
    size_t length = device_name_length(path);
    if (length) {
        // device:path, the path is always from the root of the device
        if (length + 3 > size)
            return NULL;
        memcpy(buffer, path, length);
        memcpy(buffer + length, ":/", 3);
        path += length + 1;
    } else if (current_directory[0]) {
        if (*path == '/')
            length = device_name_length(current_directory) + 2;
        else
            length = strlen(current_directory);
        if (length + 1 > size)
            return NULL;
        memcpy(buffer, current_directory, length);
        buffer[length] = '\0';
    } else {
        // No current directory yet, the default device gets the path as it is
        return path;
    }
    return append_path(buffer, size, path) ? buffer : NULL;
}

// Finds the device of a path and makes it absolute in buffer
static const devoptab_t* path_device(const char** path, char* buffer, struct _reent* r) {
    *path = absolute_path(*path, buffer, PATH_MAX);
    if (!*path) {
        errno = ENAMETOOLONG;
        return NULL;
    }
    const devoptab_t* device = GetDeviceOpTab(*path);
    if (!device || device == &null_device) {
        errno = ENODEV;
        return NULL;
    }
    *r = (struct _reent){.deviceData = device->deviceData};
    return device;
}

// Returns the result of a device function, setting errno when it failed
static int result(int value, struct _reent* r) {
    if (value == -1)
        errno = r->_errno;
    return value;
}

static int not_supported(void) {
    errno = ENOSYS;
    return -1;
}

int open(const char* path, int flags, ...) {
    char buffer[PATH_MAX];
    struct _reent r;
    const devoptab_t* device = path_device(&path, buffer, &r);
    if (!device)
        return -1;
    if (!device->open_r)
        return not_supported();
    int fd = STD_ERR + 1;
    while (fd < MAX_FILES && handles[fd].device)
        fd++;
    if (fd == MAX_FILES) {
        errno = EMFILE;
        return -1;
    }
    void* file = calloc(1, device->structSize ? device->structSize : 1);
    if (!file) {
        errno = ENOMEM;
        return -1;
    }
    if (result(device->open_r(&r, file, path, flags, 0666), &r) == -1) {
        free(file);
        return -1;
    }
    handles[fd] = (Handle){device, file};
    return fd;
}

// Finds the handle of a file descriptor, the standard streams have no file struct
static const Handle* handle(int fd, struct _reent* r) {
    static Handle standard;
    if (fd >= 0 && fd <= STD_ERR) {
        standard.device = devoptab_list[fd];
        standard.file = NULL;
        *r = (struct _reent){.deviceData = standard.device->deviceData};
        return &standard;
    }
    if (fd < 0 || fd >= MAX_FILES || !handles[fd].device) {
        errno = EBADF;
        return NULL;
    }
    *r = (struct _reent){.deviceData = handles[fd].device->deviceData};
    return &handles[fd];
}

int close(int fd) {
    struct _reent r;
    const Handle* h = handle(fd, &r);
    if (!h)
        return -1;
    if (fd <= STD_ERR)
        return 0;
    int value = h->device->close_r ? result(h->device->close_r(&r, h->file), &r) : 0;
    free(h->file);
    handles[fd].device = NULL;
    return value;
}

ssize_t read(int fd, void* buffer, size_t size) {
    struct _reent r;
    const Handle* h = handle(fd, &r);
    if (!h)
        return -1;
    if (!h->device->read_r)
        return not_supported();
    return result(h->device->read_r(&r, h->file, buffer, size), &r);
}

ssize_t write(int fd, const void* buffer, size_t size) {
    struct _reent r;
    const Handle* h = handle(fd, &r);
    if (!h)
        return -1;
    if (!h->device->write_r)
        return fd <= STD_ERR ? (ssize_t)size : not_supported();
    return result(h->device->write_r(&r, h->file, buffer, size), &r);
}

off_t lseek(int fd, off_t offset, int whence) {
    struct _reent r;
    const Handle* h = handle(fd, &r);
    if (!h)
        return -1;
    if (!h->device->seek_r) {
        errno = ESPIPE;
        return -1;
    }
    off_t position = h->device->seek_r(&r, h->file, offset, whence);
    if (position == -1)
        errno = r._errno;
    return position;
}

ssize_t readv(int fd, const struct iovec* vector, int count) {
    ssize_t total = 0;
    for (int i = 0; i < count; i++) {
        ssize_t done = read(fd, vector[i].iov_base, vector[i].iov_len);
        if (done < 0)
            return total ? total : -1;
        total += done;
        if ((size_t)done < vector[i].iov_len)
            break;
    }
    return total;
}

ssize_t writev(int fd, const struct iovec* vector, int count) {
    ssize_t total = 0;
    for (int i = 0; i < count; i++) {
        ssize_t done = write(fd, vector[i].iov_base, vector[i].iov_len);
        if (done < 0)
            return total ? total : -1;
        total += done;
        if ((size_t)done < vector[i].iov_len)
            break;
    }
    return total;
}

int fstat(int fd, struct stat* st) {
    struct _reent r;
    const Handle* h = handle(fd, &r);
    if (!h)
        return -1;
    if (!h->device->fstat_r)
        return not_supported();
    return result(h->device->fstat_r(&r, h->file, st), &r);
}

int stat(const char* path, struct stat* st) {
    char buffer[PATH_MAX];
    struct _reent r;
    const devoptab_t* device = path_device(&path, buffer, &r);
    if (!device)
        return -1;
    if (!device->stat_r)
        return not_supported();
    return result(device->stat_r(&r, path, st), &r);
}

int access(const char* path, int mode) {
    struct stat st;
    (void)mode;
    return stat(path, &st);
}

int mkdir(const char* path, mode_t mode) {
    char buffer[PATH_MAX];
    struct _reent r;
    const devoptab_t* device = path_device(&path, buffer, &r);
    if (!device)
        return -1;
    if (!device->mkdir_r)
        return not_supported();
    return result(device->mkdir_r(&r, path, (int)mode), &r);
}

int unlink(const char* path) {
    char buffer[PATH_MAX];
    struct _reent r;
    const devoptab_t* device = path_device(&path, buffer, &r);
    if (!device)
        return -1;
    if (!device->unlink_r)
        return not_supported();
    return result(device->unlink_r(&r, path), &r);
}

int rmdir(const char* path) {
    char buffer[PATH_MAX];
    struct _reent r;
    const devoptab_t* device = path_device(&path, buffer, &r);
    if (!device)
        return -1;
    if (!device->rmdir_r)
        return not_supported();
    return result(device->rmdir_r(&r, path), &r);
}

int rename(const char* old_path, const char* new_path) {
    char old_buffer[PATH_MAX], new_buffer[PATH_MAX];
    struct _reent r;
    const devoptab_t* device = path_device(&old_path, old_buffer, &r);
    if (!device || !path_device(&new_path, new_buffer, &r))
        return -1;
    if (GetDeviceOpTab(new_path) != device) {
        errno = EXDEV;
        return -1;
    }
    if (!device->rename_r)
        return not_supported();
    return result(device->rename_r(&r, old_path, new_path), &r);
}

int chdir(const char* path) {
    char buffer[PATH_MAX];
    struct _reent r;
    const devoptab_t* device = path_device(&path, buffer, &r);
    if (!device)
        return -1;
    if (device->chdir_r && result(device->chdir_r(&r, path), &r) == -1)
        return -1;
    // Only absolute paths become the current directory
    if (!device_name_length(path))
        return 0;
    size_t length = strlen(path);
    if (length + 2 > sizeof(current_directory)) {
        errno = ENAMETOOLONG;
        return -1;
    }
    memcpy(current_directory, path, length + 1);
    if (current_directory[length - 1] != '/')
        memcpy(current_directory + length, "/", 2);
    default_device = FindDevice(path);
    return 0;
}

char* getcwd(char* buffer, size_t size) {
    size_t length = strlen(current_directory);
    if (length + 1 > size) {
        errno = ERANGE;
        return NULL;
    }
    // Without the trailing slash, except for the root of a device
    memcpy(buffer, current_directory, length + 1);
    if (length > 0 && buffer[length - 1] == '/' && buffer[length - 2] != ':')
        buffer[length - 1] = '\0';
    return buffer;
}

DIR* opendir(const char* path) {
    char buffer[PATH_MAX];
    struct _reent r;
    const devoptab_t* device = path_device(&path, buffer, &r);
    if (!device)
        return NULL;
    if (!device->diropen_r) {
        not_supported();
        return NULL;
    }
    DIR* dir = calloc(1, sizeof(DIR) + device->dirStateSize);
    if (!dir) {
        errno = ENOMEM;
        return NULL;
    }
    dir->device = device;
    dir->iterator.dirStruct = dir + 1;
    if (!device->diropen_r(&r, &dir->iterator, path)) {
        errno = r._errno;
        free(dir);
        return NULL;
    }
    return dir;
}

struct dirent* readdir(DIR* dir) {
    struct _reent r = {.deviceData = dir->device->deviceData};
    struct stat st;
    if (dir->device->dirnext_r(&r, &dir->iterator, dir->entry.d_name, &st) == -1)
        return NULL;
    dir->entry.d_ino = st.st_ino;
    dir->entry.d_type = S_ISDIR(st.st_mode) ? DT_DIR : S_ISREG(st.st_mode) ? DT_REG : DT_UNKNOWN;
    return &dir->entry;
}

int closedir(DIR* dir) {
    struct _reent r = {.deviceData = dir->device->deviceData};
    int value = result(dir->device->dirclose_r(&r, &dir->iterator), &r);
    free(dir);
    return value;
}
