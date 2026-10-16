// Time and sleeping on the DS clock and calico's threads

#include <stdint.h>
#include <time.h>
#include <unistd.h>

#include "calico.h"

int usleep(unsigned microseconds) {
    struct timespec request = {microseconds / 1000000, (long)(microseconds % 1000000) * 1000};
    return __syscall_nanosleep(&request, NULL);
}

// calico starts its tick counter with the first tick task, which may not have run yet
clock_t clock(void) {
    tickInit();
    return (clock_t)(tickGetCount() * CLOCKS_PER_SEC / TICK_FREQ);
}

time_t time(time_t* result) {
    struct timeval now;
    __syscall_gettod_r(NULL, &now, NULL);
    if (result)
        *result = now.tv_sec;
    return now.tv_sec;
}

// The days since 1970-01-01 of a date, Howard Hinnant's days_from_civil. Days and years fit in
// 32 bits, the DS has no 64-bit division, so only the seconds are 64-bit.
static int32_t days_from_civil(int32_t year, int32_t month, int32_t day) {
    year -= month <= 2;
    int32_t era = (year >= 0 ? year : year - 399) / 400;
    int32_t year_of_era = year - era * 400;
    int32_t day_of_year = (153 * (month + (month > 2 ? -3 : 9)) + 2) / 5 + day - 1;
    int32_t day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    return era * 146097 + day_of_era - 719468;
}

time_t mktime(struct tm* time) {
    // Months out of range move the year, the rest overflows into the days
    int32_t year = time->tm_year + 1900 + time->tm_mon / 12;
    int32_t month = time->tm_mon % 12;
    if (month < 0) {
        month += 12;
        year--;
    }
    int32_t days = days_from_civil(year, month + 1, 1) + time->tm_mday - 1;
    time_t seconds = (time_t)days * 86400 + time->tm_hour * 3600 + time->tm_min * 60 + time->tm_sec;
    gmtime_r(&seconds, time);
    return seconds;
}

// The date of a time, Howard Hinnant's civil_from_days
struct tm* gmtime_r(const time_t* restrict time, struct tm* restrict result) {
    // The one 64-bit division, for the days
    int32_t days = (int32_t)(*time / 86400);
    int32_t seconds = (int32_t)(*time - (time_t)days * 86400);
    if (seconds < 0) {
        seconds += 86400;
        days--;
    }
    result->tm_hour = seconds / 3600;
    result->tm_min = seconds / 60 % 60;
    result->tm_sec = seconds % 60;
    result->tm_wday = (days % 7 + 11) % 7;

    int32_t z = days + 719468;
    int32_t era = (z >= 0 ? z : z - 146096) / 146097;
    int32_t day_of_era = z - era * 146097;
    int32_t year_of_era = (day_of_era - day_of_era / 1460 + day_of_era / 36524 - day_of_era / 146096) / 365;
    int32_t day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    int32_t month_index = (5 * day_of_year + 2) / 153;
    int32_t month = month_index < 10 ? month_index + 3 : month_index - 9;
    int32_t year = year_of_era + era * 400 + (month <= 2);
    result->tm_mday = day_of_year - (153 * month_index + 2) / 5 + 1;
    result->tm_mon = month - 1;
    result->tm_year = year - 1900;
    result->tm_yday = days - days_from_civil(year, 1, 1);
    result->tm_isdst = 0;
    return result;
}

struct tm* localtime_r(const time_t* restrict time, struct tm* restrict result) {
    return gmtime_r(time, result);
}

// The one result of gmtime and localtime, which the next call overwrites
static struct tm shared_tm;

struct tm* gmtime(const time_t* time) {
    return gmtime_r(time, &shared_tm);
}

struct tm* localtime(const time_t* time) {
    return localtime_r(time, &shared_tm);
}
