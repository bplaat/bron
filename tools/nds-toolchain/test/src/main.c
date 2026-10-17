// Tests the toolchain on the DS: the startup code, the C library, compiler-rt, calico, libdvm
// and ndstool. `build.sh test` reads the results from RAM, they also go to test.txt on the SD card.

#include <assert.h>
#include <dirent.h>
#include <errno.h>
#include <fat.h>
#include <fcntl.h>
#include <filesystem.h>
#include <inttypes.h>
#include <limits.h>
#include <malloc.h>
#include <math.h>
#include <nds.h>
#include <setjmp.h>
#include <stdarg.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <sys/uio.h>
#include <time.h>
#include <unistd.h>

// Read from RAM by the `test` command of desmume-runner, the status is 0 while running, 1 when
// all checks passed and 2 when one failed
struct {
    volatile uint32_t status;
    char text[16 * 1024];
} test_results;
static int results_length;
static char results_path[PATH_MAX];
static int failures;

static void report(const char* format, ...) {
    va_list args;
    va_start(args, format);
    int length =
        vsnprintf(test_results.text + results_length, sizeof(test_results.text) - results_length, format, args);
    va_end(args);
    if (length > 0)
        results_length += length;
    if (results_length > (int)sizeof(test_results.text) - 1)
        results_length = (int)sizeof(test_results.text) - 1;
}

// Writes all results again, so a crash still leaves the results so far
static void save_results(void) {
    FILE* file = fopen(results_path, "w");
    if (!file)
        return;
    fwrite(test_results.text, 1, results_length, file);
    fclose(file);
}

static void check(bool ok, int line, const char* text) {
    if (!ok) {
        failures++;
        report("  line %d: %s\n", line, text);
    }
}

static uint32_t float_bits(float value) {
    union {
        float f;
        uint32_t i;
    } bits = {value};
    return bits.i;
}

static void check_float(float actual, float expected, float tolerance, int line, const char* text) {
    bool ok = isnan(expected) ? isnan(actual) : fabsf(actual - expected) <= tolerance * fabsf(expected) + 1e-30f;
    if (!ok) {
        failures++;
        report("  line %d: %s = %08" PRIx32 ", expected %08" PRIx32 "\n", line, text, float_bits(actual),
               float_bits(expected));
    }
}

static void check_string(const char* actual, const char* expected, int line) {
    if (strcmp(actual, expected) != 0) {
        failures++;
        report("  line %d: \"%s\", expected \"%s\"\n", line, actual, expected);
    }
}

#define CHECK(condition) check(condition, __LINE__, #condition)
#define CHECK_FLOAT(actual, expected) check_float(actual, expected, 2e-6f, __LINE__, #actual)
#define CHECK_EXACT(actual, expected) check_float(actual, expected, 0.0f, __LINE__, #actual)
#define CHECK_FORMAT(expected, ...) (snprintf(text, sizeof(text), __VA_ARGS__), check_string(text, expected, __LINE__))

// Runs a group of checks and saves the results after it
static void run(const char* name, void (*test)(void)) {
    int before = failures;
    report("%s\n", name);
    test();
    if (failures == before)
        report("  ok\n");
    save_results();
}

// Volatile, so the compiler can't work the results out itself
static volatile float vf[] = {1.5f, 2.25f, -3.0f, 0.1f, 1e30f};
static volatile double vd[] = {1.5, 0.25, -7.0};
static volatile int64_t vl[] = {123456789012345LL, 1000, -7};
static volatile uint32_t vu[] = {4000000000u, 7};

static void test_format(void) {
    char text[128];
    CHECK_FORMAT("-42 7 3000000000 beef BEEF 10", "%d %i %u %x %X %o", -42, 7, 3000000000u, 0xbeef, 0xbeef, 8);
    CHECK_FORMAT("[   42] [42   ] [-0042] [+5] [ 5]", "[%5d] [%-5d] [%05d] [%+d] [% d]", 42, 42, -42, 5, 5);
    CHECK_FORMAT("[007] [    -007] [0xff] [010]", "[%.3d] [%8.3d] [%#x] [%#o]", 7, -7, 255, 8);
    CHECK_FORMAT("[abc] [       abc] [abc       ] [ab] [z] [%]", "[%s] [%10s] [%-10s] [%.2s] [%c] [%%]", "abc", "abc",
                 "abc", "abc", 'z');
    CHECK_FORMAT("[     1] [2     ] [xy]", "[%*d] [%-*d] [%.*s]", 6, 1, 6, 2, 2, "xyz");
    CHECK_FORMAT("-1234567890123 18446744073709551615", "%lld %llu", -1234567890123LL, 18446744073709551615ULL);
    CHECK_FORMAT("44 4464 12 (null)", "%hhd %hd %zu %s", 300, 70000, (size_t)12, (char*)NULL);
    CHECK_FORMAT("1.500000 3.14 2 -1.250 +2.0", "%f %.2f %.0f %.3f %+.1f", 1.5, 3.14159, 2.4, -1.25, 2.0);
    CHECK_FORMAT("10000000000.0 -0.000 inf nan", "%.1f %.3f %f %f", 1e10, -0.0, (double)INFINITY, (double)NAN);
    char small[5];
    CHECK(snprintf(small, sizeof(small), "%d", 1234567) == 7 && !strcmp(small, "1234"));

    int a = 0, b = 0;
    char word[16], c = 0;
    CHECK(sscanf("[12;34m", "[%d;%dm", &a, &b) == 2 && a == 12 && b == 34);
    CHECK(sscanf("  -7 word x", "%d %s %c", &a, word, &c) == 3 && a == -7 && !strcmp(word, "word") && c == 'x');
    CHECK(sscanf("ff 0x10", "%x %i", &a, &b) == 2 && a == 255 && b == 16);
    CHECK(sscanf("abc", "%d", &a) == 0);
    // Scansets, a quoted string read into the buffer it comes from
    char quoted[16] = "\"two words\"x";
    CHECK(sscanf(quoted, "\"%[^\"]\"", quoted) == 1 && !strcmp(quoted, "two words"));
    CHECK(sscanf("abc123-]x", "%[a-c]%2[0-9]%*[0-9]%[]-]", word, text, quoted) == 3 && !strcmp(word, "abc") &&
          !strcmp(text, "12") && !strcmp(quoted, "-]"));
    CHECK(sscanf("xyz", "%[a-c]", word) == 0);
    float f = 0;
    double d = 0;
    int count = 0;
    CHECK(sscanf("1.5 -2.5e2 3x", "%f %lf%n", &f, &d, &count) == 2 && f == 1.5f && d == -250.0 && count == 10);
    CHECK(sscanf("0x1f -017", "%i %i", &a, &b) == 2 && a == 31 && b == -15);
    // Floats end where a number can't go on, letters only spell inf and nan
    CHECK(sscanf("3.5f rest", "%f%s", &f, word) == 2 && f == 3.5f && !strcmp(word, "f"));
    CHECK(sscanf("-inf 1e5e", "%f %lf%s", &f, &d, word) == 3 && isinf(f) && f < 0 && d == 1e5 && !strcmp(word, "e"));
    float g = 0;
    CHECK(sscanf("infinity! nanx", "%f%s %f", &f, word, &g) == 3 && isinf(f) && !strcmp(word, "!") && isnan(g));
    CHECK(sscanf("", "%d", &a) == EOF && sscanf("  ", "%s", word) == EOF);
}

static int compare_chars(const void* a, const void* b) {
    return *(const char*)a - *(const char*)b;
}

static int compare_ints(const void* a, const void* b) {
    return *(const int*)a - *(const int*)b;
}

static void test_stdlib(void) {
    char* end;
    CHECK(strtol(" -123abc", &end, 10) == -123 && *end == 'a');
    CHECK(strtol("0x1F", &end, 0) == 31 && !*end);
    CHECK(strtol("017", NULL, 0) == 15);
    CHECK(strtol("zz", &end, 10) == 0 && !strcmp(end, "zz"));
    CHECK(strtoul("4294967295", NULL, 10) == 4294967295u);
    errno = 0;
    CHECK(strtol("99999999999", NULL, 10) == LONG_MAX && errno == ERANGE);
    CHECK(atoi("42") == 42 && atol("-123456789") == -123456789L && abs(-5) == 5);
    // Floats, strtod and atof with float precision too
    CHECK(strtof(" -1.5e3x", &end) == -1500.0f && *end == 'x');
    CHECK(strtof("0.1", NULL) == 0.1f && strtof("123456.7", NULL) == 123456.7f && atof(".25") == 0.25);
    CHECK(strtod("3.14159f", &end) == (double)3.14159f && *end == 'f');
    // Up to 7 digits and a power up to 10^10 are rounded exactly
    CHECK(strtof("0.3", NULL) == 0.3f && strtof("1234.567", NULL) == 1234.567f && strtof("2.5e-3", NULL) == 2.5e-3f &&
          strtof("9999999e10", NULL) == 9999999e10f && strtof("-0.0000001", NULL) == -0.0000001f);
    check_float(strtof("1e-30", NULL), 1e-30f, 1e-6f, __LINE__, "strtof(\"1e-30\")");
    check_float(strtof("6.02214076e23", NULL), 6.02214076e23f, 1e-6f, __LINE__, "strtof(\"6.02214076e23\")");
    CHECK(isinf(strtof("-Infinity", &end)) && !*end && isnan(strtof("nan", NULL)));
    CHECK(strtof("0e999", NULL) == 0 && strtof("x1", &end) == 0 && !strcmp(end, "x1"));
    errno = 0;
    CHECK(isinf(strtof("1e39", NULL)) && errno == ERANGE);
    // alloca from stdlib.h like glibc's, and the byte order from sys/types.h
    char* stack = alloca(64);
    stack[63] = 1;
    CHECK(stack[63] == 1 && BYTE_ORDER == LITTLE_ENDIAN);
    // There is no input
    CHECK(getchar() == EOF);
    // clock() runs before anything slept or started a tick task
    clock_t first = clock();
    for (int spins = 0; clock() == first && spins < 10000000; spins++)
        ;
    CHECK(clock() != first);

    int numbers[100];
    for (int i = 0; i < 100; i++)
        numbers[i] = (i * 7919) % 101;
    qsort(numbers, 100, sizeof(int), compare_ints);
    bool sorted = true;
    for (int i = 1; i < 100; i++)
        sorted &= numbers[i - 1] <= numbers[i];
    CHECK(sorted);
    int key = 50;
    CHECK(bsearch(&key, numbers, 100, sizeof(int), compare_ints) != NULL);
    // elements that aren't word sized are swapped a byte at a time
    char triples[] = "zzzyyyxxxaaa";
    qsort(triples, 4, 3, compare_chars);
    CHECK(!strcmp(triples, "aaaxxxyyyzzz"));

    char text[32] = "a,b,,c";
    CHECK(!strcmp(strtok(text, ","), "a") && !strcmp(strtok(NULL, ","), "b") && !strcmp(strtok(NULL, ","), "c") &&
          !strtok(NULL, ","));
    CHECK(strstr("hello world", "o w") && !strstr("abc", "abd"));
    CHECK(strcasecmp("HeLLo", "hello") == 0 && strncmp("abc", "abd", 2) == 0);
    CHECK(strrchr("a/b/c", '/')[1] == 'c' && strspn("aab", "a") == 2 && strcspn("aab", "b") == 2);
    char* copy = strdup("copy");
    CHECK(copy && !strcmp(copy, "copy"));
    free(copy);
}

static void test_memory(void) {
    static uint8_t source[300], destination[300];
    for (int i = 0; i < 300; i++)
        source[i] = (uint8_t)i;
    bool ok = true;
    for (int offset1 = 0; offset1 < 4; offset1++) {
        for (int offset2 = 0; offset2 < 4; offset2++) {
            for (int size = 0; size < 100; size += 7) {
                memset(destination, 0xee, sizeof(destination));
                memcpy(destination + offset1, source + offset2, size);
                ok &= !memcmp(destination + offset1, source + offset2, size) && destination[offset1 + size] == 0xee;
                memset(destination + offset1, 0x5a, size);
                for (int i = 0; i < size; i++)
                    ok &= destination[offset1 + i] == 0x5a;
                ok &= destination[offset1 + size] == 0xee;
            }
        }
    }
    CHECK(ok);
    // memcmp skips equal words, the first different byte decides
    memcpy(destination, source, 300);
    destination[9] = 200;
    CHECK(memcmp(destination, source, 300) > 0 && memcmp(source, destination, 300) < 0 &&
          memcmp(destination, source, 9) == 0 && memcmp(destination + 1, source + 1, 20) > 0);
    for (int i = 0; i < 300; i++)
        destination[i] = (uint8_t)i;
    memmove(destination + 3, destination, 200);
    CHECK(destination[3] == 0 && destination[202] == 199);
    for (int i = 0; i < 300; i++)
        destination[i] = (uint8_t)i;
    memmove(destination, destination + 5, 200);
    CHECK(destination[0] == 5 && destination[199] == 204);

    // VRAM ignores byte writes, so copies and fills of halfword aligned sizes must use halfwords
    vramSetBankA(VRAM_A_LCD);
    uint8_t* vram = (uint8_t*)VRAM_A;
    memset(vram, 0, 64);
    memcpy(vram + 2, source + 2, 30);
    CHECK(vram[2] == 2 && vram[3] == 3 && vram[30] == 30 && vram[31] == 31);
    memcpy(vram + 34, source + 1, 6);
    CHECK(vram[34] == 1 && vram[39] == 6);
    memmove(vram + 36, vram + 35, 4);
    CHECK(vram[36] == 2 && vram[39] == 5);
    memset(vram + 2, 0x77, 30);
    CHECK(vram[2] == 0x77 && vram[31] == 0x77 && vram[32] == 0);
    // the bytes at odd ends go with their halfword, the other byte stays
    memcpy(vram + 41, source + 1, 4);
    CHECK(vram[40] == 0 && vram[41] == 1 && vram[44] == 4 && vram[45] == 0);
    memset(vram + 49, 0x55, 3);
    CHECK(vram[48] == 0 && vram[49] == 0x55 && vram[51] == 0x55 && vram[52] == 0);
    memmove(vram + 53, vram + 41, 3);
    CHECK(vram[52] == 0 && vram[53] == 1 && vram[55] == 3 && vram[56] == 0);
}

static void test_math(void) {
    CHECK_EXACT(sqrtf(2.25f), 1.5f);
    CHECK_EXACT(sqrtf(2.0f), 1.41421356f);
    CHECK_EXACT(sqrtf(-0.0f), -0.0f);
    CHECK(isnan(sqrtf(-1.0f)));
    CHECK_EXACT(floorf(-2.5f), -3.0f);
    CHECK_EXACT(ceilf(-2.5f), -2.0f);
    CHECK(lrintf(-2.6f) == -3 && lrintf(2.5f) == 2 && lrintf(3.5f) == 4 && lrintf(-2.5f) == -2);
    CHECK(lrintf(0.49999997f) == 0 && lrintf(8388609.0f) == 8388609 && lrintf(-0.5f) == 0);
    CHECK_EXACT(fmodf(7.5f, 2.0f), 1.5f);
    CHECK_EXACT(fmodf(-7.5f, 2.0f), -1.5f);
    CHECK_EXACT(fmodf(1e10f, 3.0f), 1.0f);
    CHECK_FLOAT(sinf(1.0f), 0.84147098f);
    CHECK_FLOAT(cosf(1.0f), 0.54030231f);
    CHECK_FLOAT(sinf(1000.0f), 0.82687954f);
    CHECK_FLOAT(tanf(0.5f), 0.54630249f);
    CHECK_FLOAT(atanf(2.0f), 1.10714872f);
    CHECK_FLOAT(atan2f(1.0f, -1.0f), 2.35619449f);
    CHECK_EXACT(atan2f(-0.0f, -1.0f), -3.14159265f);
    CHECK_FLOAT(asinf(0.5f), 0.52359878f);
    CHECK_FLOAT(acosf(-0.5f), 2.09439510f);
    CHECK(isnan(acosf(1.5f)));
    CHECK_FLOAT(expf(1.0f), 2.71828183f);
    CHECK_FLOAT(exp2f(10.5f), 1448.15468f);
    CHECK_FLOAT(log2f(10.0f), 3.32192809f);
    CHECK_FLOAT(log2f(1e-40f), -132.877124f);
    CHECK_FLOAT(powf(2.0f, 0.5f), 1.41421356f);
    CHECK_EXACT(powf(-2.0f, 3.0f), -8.0f);
    CHECK_FLOAT(tanhf(0.5f), 0.46211716f);
}

// compiler-rt: software floating point and 64-bit integer math
static void test_compiler_rt(void) {
    CHECK_EXACT(vf[0] + vf[1], 3.75f);
    CHECK_EXACT(vf[0] * vf[2], -4.5f);
    CHECK_EXACT(vf[1] / vf[0], 1.5f);
    CHECK_EXACT(vf[3] * 3.0f, 0.3f);
    CHECK(vf[2] < vf[0] && vf[0] >= vf[0] && !(vf[0] > vf[1]) && vf[0] != vf[1]);
    CHECK(isinf(vf[4] * vf[4]));
    CHECK((int)vf[2] == -3 && (unsigned)vf[1] == 2 && (float)vu[0] == 4e9f && (float)vl[0] == 1.23456789e14f);
    CHECK(vd[0] * vd[1] == 0.375 && vd[0] / vd[1] == 6.0 && vd[0] + vd[2] == -5.5);
    CHECK((float)vd[1] == 0.25f && (double)vf[0] == 1.5 && (int)vd[2] == -7 && vd[1] < vd[0]);
    CHECK(vl[0] / vl[1] == 123456789012LL && vl[0] % vl[1] == 345 && vl[0] * vl[2] == -864197523086415LL);
    CHECK(vl[0] >> 20 == 117737568LL && (uint64_t)vl[0] << 10 == 126419751948641280ULL);
    CHECK(vu[0] / vu[1] == 571428571u && vu[0] % vu[1] == 3u && (int)vu[1] / -2 == -3);
}

int thumb_add(int a, int b) {
    return a + b;
}
int itcm_call(int a, int b);

static jmp_buf jump;
static void jump_back(int value) {
    longjmp(jump, value);
}

// A failed assertion comes here instead of the C library's, like with newlib
static const char* assert_expression;
void __assert_func(const char* file, int line, const char* function, const char* expression) {
    (void)file, (void)line, (void)function;
    assert_expression = expression;
    longjmp(jump, 1);
}

static void test_runtime(void) {
    // ARM code in ITCM calling Thumb code in main RAM and back
    CHECK(itcm_call(20, 1) == 42);
    // setjmp and longjmp
    volatile int jumped = 0;
    int value = setjmp(jump);
    if (value == 0)
        jump_back(7);
    else
        jumped = value;
    CHECK(jumped == 7);
    static volatile int zero = 0;
    if (setjmp(jump) == 0)
        assert(zero == 1);
    CHECK(assert_expression && !strcmp(assert_expression, "zero == 1"));
    // Constructors ran before main()
    extern int constructed;
    CHECK(constructed == 1);
}

int constructed;
__attribute__((constructor)) static void construct(void) {
    constructed++;
}

static void test_heap(void) {
    void* blocks[64];
    bool ok = true;
    for (int round = 0; round < 4; round++) {
        for (int i = 0; i < 64; i++) {
            size_t size = (size_t)(rand() % 2000) + 1;
            blocks[i] = malloc(size);
            ok &= blocks[i] && ((uintptr_t)blocks[i] & 7) == 0;
            if (blocks[i]) {
                memset(blocks[i], i, size);
                ((uint8_t*)blocks[i])[size - 1] = (uint8_t)i;
            }
        }
        for (int i = 0; i < 64; i += 2)
            free(blocks[i]);
        for (int i = 1; i < 64; i += 2) {
            ok &= blocks[i] && ((uint8_t*)blocks[i])[0] == i;
            free(blocks[i]);
        }
    }
    CHECK(ok);
    void* aligned = memalign(32, 100);
    CHECK(aligned && ((uintptr_t)aligned & 31) == 0);
    free(aligned);
    static volatile size_t bad_alignment = 24;
    CHECK(memalign(bad_alignment, 100) == NULL);
    char* grown = malloc(10);
    strcpy(grown, "abc");
    grown = realloc(grown, 5000);
    CHECK(grown && !strcmp(grown, "abc"));
    free(grown);
    int* zeros = calloc(100, sizeof(int));
    CHECK(zeros && zeros[0] == 0 && zeros[99] == 0);
    free(zeros);
    // A game takes all memory that is left after sbrk(0)
    extern char* fake_heap_end;
    size_t left = (size_t)(fake_heap_end - (char*)sbrk(0));
    CHECK(left > 3000000);
    void* rest = malloc(left - 64 * 1024);
    CHECK(rest != NULL);
    free(rest);
    // More than the DSi has
    CHECK(malloc(32 * 1024 * 1024) == NULL);
    // The free chunk at the end of the heap counts when it grows
    rest = malloc(left - 64 * 1024);
    free(rest);
    rest = malloc(left - 32 * 1024);
    CHECK(rest != NULL);
    free(rest);
    // A far heap is used first by far_malloc(), which falls back to the main RAM when it is full,
    // and realloc() gives back the end of a block it shrinks
    static char far_memory[64 * 1024];
    far_heap_init(far_memory, sizeof(far_memory));
    char* far = far_malloc(60000);
    CHECK(far >= far_memory && far < far_memory + sizeof(far_memory));
    char* near = far_malloc(30000);
    CHECK(near && (near < far_memory || near >= far_memory + sizeof(far_memory)));
    CHECK(realloc(far, 100) == far);
    char* tail = far_malloc(50000);
    CHECK(tail >= far_memory && tail < far_memory + sizeof(far_memory));
    free(tail);
    free(near);
    free(far);
    near = malloc(100);
    CHECK(near < far_memory || near >= far_memory + sizeof(far_memory));
    free(near);
}

// calico threads, each with its own thread local storage
static _Thread_local int tls_value = 5;
static volatile int thread_result;

static int worker(void* arg) {
    tls_value += (int)arg;
    thread_result = tls_value;
    return tls_value * 2;
}

static void test_threads(void) {
    static Thread thread;
    alignas(8) static uint8_t stack[4096];
    threadPrepare(&thread, worker, (void*)10, &stack[sizeof(stack)], MAIN_THREAD_PRIO - 1);
    threadAttachLocalStorage(&thread, NULL);
    threadStart(&thread);
    CHECK(threadJoin(&thread) == 30);
    CHECK(thread_result == 15 && tls_value == 5);
    // The tick counter and sleeping
    u32 start = tickGetCount();
    usleep(20000);
    u32 elapsed = tickGetCount() - start;
    CHECK(elapsed >= ticksFromUsec(19000) && elapsed < ticksFromUsec(100000));
    CHECK(time(NULL) > 1700000000);
    clock_t clock_start = clock();
    usleep(20000);
    CHECK(clock() - clock_start >= 19 * CLOCKS_PER_SEC / 1000 && clock() - clock_start < CLOCKS_PER_SEC / 10);
    // Dates, 2024-02-29 was a Thursday
    time_t leap = 1709251199;
    struct tm date;
    gmtime_r(&leap, &date);
    CHECK(date.tm_year == 124 && date.tm_mon == 1 && date.tm_mday == 29 && date.tm_hour == 23 && date.tm_min == 59 &&
          date.tm_sec == 59 && date.tm_wday == 4 && date.tm_yday == 59);
    CHECK(localtime(&leap)->tm_mday == 29 && gmtime(&leap)->tm_hour == 23);
    date.tm_mon = 13;
    CHECK(mktime(&date) == 1740873599 && date.tm_year == 125 && date.tm_mon == 2 && date.tm_mday == 1);
}

// libdvm and FatFs on the SD card through the C library
static void test_files(void) {
    char cwd[PATH_MAX];
    mkdir("/toolchain-test", 0777);
    struct stat st;
    CHECK(stat("/toolchain-test", &st) == 0 && S_ISDIR(st.st_mode));
    CHECK(chdir("/toolchain-test") == 0);
    CHECK(getcwd(cwd, sizeof(cwd)) && !strcmp(strchr(cwd, ':'), ":/toolchain-test"));

    FILE* file = fopen("data.txt", "w");
    CHECK(file != NULL);
    if (!file)
        return;
    for (int i = 0; i < 1000; i++)
        fprintf(file, "line %d\n", i);
    long size = ftell(file);
    CHECK(size == 8890);
    fclose(file);
    CHECK(stat("data.txt", &st) == 0 && st.st_size == size);
    CHECK(access("data.txt", F_OK) == 0 && access("missing.txt", R_OK) == -1 && errno == ENOENT);

    file = fopen("data.txt", "r");
    char line[64];
    int lines = 0, last = -1;
    while (fgets(line, sizeof(line), file)) {
        lines++;
        sscanf(line, "line %d", &last);
    }
    CHECK(lines == 1000 && last == 999 && feof(file));
    // fscanf reads on where it stopped, ungetc puts a character back
    fseek(file, 0, SEEK_SET);
    char name[8];
    int number = -1, next_number = -1;
    CHECK(fscanf(file, "%4s %d\n%*s %d", name, &number, &next_number) == 3 && !strcmp(name, "line") && number == 0 &&
          next_number == 1);
    CHECK(fgetc(file) == '\n' && ungetc('L', file) == 'L' && fgetc(file) == 'L' && fgetc(file) == 'l');
    fseek(file, 5, SEEK_SET);
    CHECK(fgetc(file) == '0' && ftell(file) == 6);
    CHECK(fgetc(file) == '\n');
    fseek(file, -2, SEEK_CUR);
    CHECK(fgetc(file) == '0');
    static char buffer[10000];
    fseek(file, 0, SEEK_SET);
    CHECK(fread(buffer, 1, sizeof(buffer), file) == (size_t)size && !memcmp(buffer + size - 9, "line 999\n", 9));
    fclose(file);

    file = fopen("data.txt", "a");
    fputs("appended\n", file);
    fclose(file);
    // Append mode writes at the end, also after a seek
    file = fopen("data.txt", "a+");
    fseek(file, 0, SEEK_SET);
    fputs("end\n", file);
    fseek(file, -4, SEEK_END);
    CHECK(fgets(line, sizeof(line), file) && !strcmp(line, "end\n"));
    fclose(file);
    file = fopen("data.txt", "r+");
    fseek(file, -13, SEEK_END);
    CHECK(fgets(line, sizeof(line), file) && !strcmp(line, "appended\n"));
    fseek(file, 0, SEEK_SET);
    fputs("LINE", file);
    fseek(file, 0, SEEK_SET);
    CHECK(fgets(line, sizeof(line), file) && !strcmp(line, "LINE 0\n"));
    fclose(file);

    CHECK(rename("data.txt", "moved.txt") == 0);
    CHECK(stat("data.txt", &st) == -1 && errno == ENOENT);
    file = fopen("../toolchain-test/moved.txt", "r");
    CHECK(file != NULL);
    if (file)
        fclose(file);
    DIR* dir = opendir(".");
    int found = 0;
    for (struct dirent* entry; dir && (entry = readdir(dir));)
        found += !strcmp(entry->d_name, "moved.txt") && entry->d_type == DT_REG;
    if (dir)
        closedir(dir);
    CHECK(found == 1);
    CHECK(remove("moved.txt") == 0 && chdir("..") == 0 && remove("toolchain-test") == 0);
    CHECK(fopen("/missing/file.txt", "r") == NULL && errno == ENOENT);
    CHECK(fopen("nodevice:/file.txt", "r") == NULL && errno == ENODEV);

    int fd = open("raw.bin", O_WRONLY | O_CREAT | O_TRUNC, S_IREAD | S_IWRITE);
    CHECK(fd >= 0 && write(fd, "raw", 3) == 3);
    struct iovec parts[2] = {{"ab", 2}, {"cde", 3}};
    CHECK(writev(fd, parts, 2) == 5 && fstat(fd, &st) == 0 && st.st_size == 8 && close(fd) == 0);
    char first[4] = {0}, second[5] = {0};
    struct iovec into[2] = {{first, 3}, {second, 4}};
    fd = open("raw.bin", O_RDONLY);
    CHECK(fd >= 0 && readv(fd, into, 2) == 7 && !strcmp(first, "raw") && !strcmp(second, "abcd") && close(fd) == 0);
    CHECK(unlink("raw.bin") == 0);
}

// NitroFS: the files ndstool embedded in the ROM
static void test_nitrofs(void) {
    FILE* file = fopen("nitro:/hello.txt", "rb");
    CHECK(file != NULL);
    if (file) {
        char line[64];
        CHECK(fgets(line, sizeof(line), file) && !strcmp(line, "hello nitro\n"));
        CHECK(fgets(line, sizeof(line), file) && !strcmp(line, "second line\n"));
        fclose(file);
    }
    char cwd[PATH_MAX];
    getcwd(cwd, sizeof(cwd));
    struct stat st;
    CHECK(chdir("nitro:/dir") == 0 && stat("a.bin", &st) == 0 && st.st_size == 1);
    CHECK(fopen("nitro:/new.txt", "w") == NULL);
    chdir(cwd);
}

int main(int argc, char** argv) {
    consoleDemoInit();
    printf("Toolchain test\n");
    bool fat = fatInitDefault();
    bool nitro = nitroFSInit(NULL);
    // The results go next to the program, where fatInitDefault() changed to
    if (!fat || !getcwd(results_path, sizeof(results_path) - 16)) {
        report("No SD card\n");
        test_results.status = 2;
    } else {
        strcat(results_path, results_path[strlen(results_path) - 1] == '/' ? "test.txt" : "/test.txt");
        report("argc %d, argv[0] %s\n", argc, argc > 0 ? argv[0] : "-");
        CHECK(nitro);
        save_results();
        run("format", test_format);
        run("stdlib", test_stdlib);
        run("memory", test_memory);
        run("math", test_math);
        run("compiler-rt", test_compiler_rt);
        run("runtime", test_runtime);
        run("heap", test_heap);
        run("threads", test_threads);
        run("files", test_files);
        if (nitro)
            run("nitrofs", test_nitrofs);
        report("DONE %d failed\n", failures);
        save_results();
        printf("DONE %d failed\n", failures);
        test_results.status = failures == 0 ? 1 : 2;
    }
    DC_FlushRange(&test_results, sizeof(test_results));
    while (pmMainLoop())
        threadWaitForVBlank();
    return 0;
}
