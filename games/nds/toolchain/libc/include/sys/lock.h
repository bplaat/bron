#pragma once

#include <calico/system/mutex.h>

// The newlib locks libdvm uses, on calico mutexes
typedef Mutex _LOCK_T;
#define __lock_init(lock) ((lock) = (Mutex){0})
#define __lock_acquire(lock) mutexLock(&(lock))
#define __lock_release(lock) mutexUnlock(&(lock))
#define __lock_close(lock) ((void)0)
