#pragma once

#include <stddef.h>
#include <stdint.h>

typedef int64_t time_t;
typedef long clock_t;

// clock() counts milliseconds, only the time between two calls means something
#define CLOCKS_PER_SEC 1000

struct timespec {
    time_t tv_sec;
    long tv_nsec;
};

struct tm {
    int tm_sec;
    int tm_min;
    int tm_hour;
    int tm_mday;
    int tm_mon;
    int tm_year;
    int tm_wday;
    int tm_yday;
    int tm_isdst;
};

// The DS clock has no time zone, local time is UTC
time_t time(time_t* result);
clock_t clock(void);
time_t mktime(struct tm* time);
struct tm* gmtime_r(const time_t* restrict time, struct tm* restrict result);
struct tm* gmtime(const time_t* time);
struct tm* localtime_r(const time_t* restrict time, struct tm* restrict result);
struct tm* localtime(const time_t* time);
