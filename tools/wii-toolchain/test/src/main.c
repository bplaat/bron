// Tests the toolchain on the Wii: the startup code, the C library, compiler-rt, libogc and the
// tools. The results go to /tmp/test.txt on the NAND, where `build.sh test` reads them.

#include <gccore.h>
#include <malloc.h>
#include <math.h>
#include <ogc/cast.h>
#include <ogc/lwp_watchdog.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

#define RESULTS_PATH "/tmp/test.txt"
#define PI 3.14159265358979f

_Alignas(32) static const uint8_t test_tpl[] = {
#embed "../../../../target/wii-toolchain/test/test.tpl"
};

_Alignas(32) static char results[16 * 1024];
static int results_length;
static int failures;

static void report(const char* format, ...) {
    va_list args;
    va_start(args, format);
    int length = vsnprintf(results + results_length, sizeof(results) - results_length, format, args);
    va_end(args);
    if (length > 0)
        results_length += length;
    if (results_length > (int)sizeof(results) - 1)
        results_length = (int)sizeof(results) - 1;
}

// Writes all results again, so a crash still leaves the results so far
static void save_results(void) {
    s32 file = ISFS_Open(RESULTS_PATH, ISFS_OPEN_WRITE);
    if (file < 0)
        return;
    ISFS_Write(file, results, results_length);
    ISFS_Close(file);
}

static uint32_t float_bits(float value) {
    union {
        float f;
        uint32_t i;
    } bits = {value};
    return bits.i;
}

static void check(bool ok, int line, const char* text) {
    if (!ok) {
        failures++;
        report("  line %d: %s\n", line, text);
    }
}

// There is no %f, so floats are shown as their bits
static void check_float(float actual, float expected, float tolerance, int line, const char* text) {
    bool ok = isnan(expected) ? isnan(actual)
                              : actual == expected || (isfinite(actual) && isfinite(expected) &&
                                                       fabsf(actual - expected) <= tolerance * fabsf(expected));
    if (!ok) {
        failures++;
        report("  line %d: %s = %08x, expected %08x\n", line, text, float_bits(actual), float_bits(expected));
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

static void run(const char* name, void (*test)(void)) {
    int before = failures;
    test();
    report("%s %s\n", failures == before ? "ok  " : "FAIL", name);
    save_results();
}

// Startup: crt0 cleared .bss and the loader copied .data
static int data_value = 1234;
static int bss_values[256];

static void test_startup(void) {
    extern void build_argv(struct __argv * args);
    _Alignas(4) char storage[64];
    memset(storage, 0xa5, sizeof(storage));
    memcpy(storage, "app\0arg", 8);
    struct __argv args = {.commandLine = storage, .length = 8};
    build_argv(&args);
    CHECK(args.argc == 2);
    CHECK(strcmp(args.argv[0], "app") == 0);
    CHECK(strcmp(args.argv[1], "arg") == 0);
    CHECK(args.argv[args.argc] == NULL);
    CHECK(args.endARGV == &args.argv[args.argc + 1]);
    CHECK((uintptr_t)args.endARGV <= (uintptr_t)(storage + sizeof(storage)));

    memset(storage, 0xa5, sizeof(storage));
    args.length = 0;
    build_argv(&args);
    CHECK(args.argc == 0 && args.argv[0] == NULL);
    CHECK(args.endARGV == &args.argv[1]);

    CHECK(data_value == 1234);
    bool zero = true;
    for (int i = 0; i < 256; i++)
        zero &= bss_values[i] == 0;
    CHECK(zero);
}

// Byte loops as the reference for the word sized memory functions
static bool same(const uint8_t* a, const uint8_t* b, size_t size) {
    for (size_t i = 0; i < size; i++) {
        if (a[i] != b[i])
            return false;
    }
    return true;
}

static void test_memory(void) {
    static uint8_t source[192], buffer[192], expected[192];
    for (int i = 0; i < 192; i++)
        source[i] = (uint8_t)(i * 7 + 1);

    bool copy_ok = true, move_ok = true, set_ok = true;
    for (int to = 0; to < 8; to++) {
        for (int from = 0; from < 8; from++) {
            for (int size = 0; size <= 100; size++) {
                for (int i = 0; i < 192; i++)
                    buffer[i] = expected[i] = 0xee;
                for (int i = 0; i < size; i++)
                    expected[to + i] = source[from + i];
                memcpy(buffer + to, source + from, size);
                copy_ok &= same(buffer, expected, 192);
            }
        }
    }
    // Overlapping moves in both directions
    for (int shift = -9; shift <= 9; shift++) {
        for (int size = 0; size <= 100; size++) {
            for (int i = 0; i < 192; i++)
                buffer[i] = expected[i] = source[i];
            for (int i = 0; i < size; i++)
                expected[40 + shift + i] = source[40 + i];
            memmove(buffer + 40 + shift, buffer + 40, size);
            move_ok &= same(buffer, expected, 192);
        }
    }
    // Zero fills of 64 bytes and more use dcbz on cached memory
    for (int offset = 0; offset < 8; offset++) {
        for (int size = 0; size <= 180; size += 3) {
            for (int value = 0; value <= 0xa5; value += 0xa5) {
                for (int i = 0; i < 192; i++)
                    buffer[i] = expected[i] = 0xee;
                for (int i = 0; i < size; i++)
                    expected[offset + i] = (uint8_t)value;
                memset(buffer + offset, value, size);
                set_ok &= same(buffer, expected, 192);
            }
        }
    }
    CHECK(copy_ok);
    CHECK(move_ok);
    CHECK(set_ok);

    CHECK(memcmp("abc", "abd", 3) < 0);
    CHECK(memcmp("abc", "abc", 3) == 0);
    CHECK(strlen("hello") == 5);
    CHECK(strnlen("hello", 3) == 3);
    CHECK(strcmp("abc", "abd") < 0 && strcmp("b", "a") > 0 && strcmp("same", "same") == 0);
    char copy[8];
    CHECK(strcpy(copy, "wii") == copy && strcmp(copy, "wii") == 0);
    const char* hello = "hello";
    CHECK(strchr(hello, 'l') == hello + 2);
    CHECK(strchr(hello, 'x') == NULL);
}

static void test_malloc(void) {
    uint8_t* a = malloc(100);
    uint8_t* b = malloc(100);
    CHECK(a != NULL && b != NULL);
    CHECK(((uintptr_t)a & 15) == 0 && ((uintptr_t)b & 15) == 0);
    CHECK(a + 100 <= b || b + 100 <= a);
    free(a);
    CHECK(malloc(100) == a);
    free(a);
    free(b);

    bool aligned = true;
    for (size_t alignment = 32; alignment <= 4096; alignment *= 2) {
        uint8_t* pointer = memalign(alignment, 1000);
        aligned &= pointer != NULL && ((uintptr_t)pointer & (alignment - 1)) == 0;
        memset(pointer, 0x5a, 1000);
        free(pointer);
    }
    CHECK(aligned);
    CHECK(memalign(48, 100) == NULL);
    CHECK(malloc(0x20000000) == NULL);

    // calloc clears memory that was used before
    uint8_t* dirty = malloc(1000);
    memset(dirty, 0xff, 1000);
    free(dirty);
    uint8_t* clean = calloc(10, 100);
    bool zero = clean != NULL;
    for (int i = 0; zero && i < 1000; i++)
        zero = clean[i] == 0;
    CHECK(zero);
    free(clean);

    uint8_t* grown = malloc(16);
    for (int i = 0; i < 16; i++)
        grown[i] = (uint8_t)i;
    grown = realloc(grown, 5000);
    bool kept = grown != NULL;
    for (int i = 0; kept && i < 16; i++)
        kept = grown[i] == i;
    CHECK(kept);
    free(grown);

    // Too big for what is left of MEM1, so it comes from MEM2
    uint8_t* big = malloc(32 * 1024 * 1024);
    CHECK(big != NULL && (uintptr_t)big >> 28 == 0x9);
    if (big) {
        big[0] = 1;
        big[32 * 1024 * 1024 - 1] = 2;
        CHECK(big[0] == 1 && big[32 * 1024 * 1024 - 1] == 2);
    }
    free(big);
}

static void test_printf(void) {
    char text[64];
    CHECK_FORMAT("42", "%d", 42);
    CHECK_FORMAT("  -42|", "%5d|", -42);
    CHECK_FORMAT("42   |", "%-5d|", 42);
    CHECK_FORMAT("-0042", "%05d", -42);
    CHECK_FORMAT("007", "%.3d", 7);
    CHECK_FORMAT("0000000000000000000000001", "%.25d", 1);
    CHECK_FORMAT("-0000000000000000000000001", "%.25d", -1);
    CHECK_FORMAT("    -0000000000000000000000001", "%030.25d", -1);
    CHECK_FORMAT("-0000000000000000000000001    ", "%-30.25d", -1);
    CHECK_FORMAT("-42  ", "%-05d", -42);
    CHECK_FORMAT("", "%.0d", 0);
    CHECK_FORMAT("   ", "%3.0d", 0);
    CHECK(snprintf(text, 5, "%.25d", 1) == 25);
    check_string(text, "0000", __LINE__);
    CHECK(snprintf(NULL, 0, "%.25d", 1) == 25);
    CHECK_FORMAT("4000000000", "%u", 4000000000u);
    CHECK_FORMAT("beef BEEF 0000BEEF", "%x %X %08X", 0xbeef, 0xbeef, 0xbeef);
    CHECK_FORMAT("-1234567890123", "%lld", -1234567890123LL);
    CHECK_FORMAT("123456789abcdef", "%llx", 0x123456789abcdefULL);
    CHECK_FORMAT("7 x %", "%zu %c %%", (size_t)7, 'x');
    CHECK_FORMAT("0x80001234", "%p", (void*)0x80001234);
    CHECK_FORMAT("wii|wi|(null)", "%s|%.2s|%s", "wii", "wii", (char*)NULL);
    CHECK(snprintf(text, 5, "hello world") == 11);
    check_string(text, "hell", __LINE__);
}

static void test_math(void) {
    // Volatile inputs exercise the libc calls rather than constant folding.
    volatile float zero = 0.0f, negzero = -0.0f;
    volatile float inf = __builtin_inff(), nan = __builtin_nanf("");
    volatile float subnormal = 0x1p-149f;
    CHECK(isnan(sqrtf(-inf)));
    CHECK_FLOAT(sqrtf(subnormal), 3.7433921e-23f);
    CHECK_FLOAT(sqrtf(0x1p-126f), 0x1p-63f);
    CHECK(float_bits(sqrtf(negzero)) == 0x80000000u);
    CHECK(isnan(sqrtf(nan)));
    CHECK(float_bits(fmodf(negzero, 2.0f)) == 0x80000000u);
    CHECK(float_bits(fmodf(-4.0f, 2.0f)) == 0x80000000u);
    CHECK_FLOAT(atanf(inf), PI / 2.0f);
    CHECK_FLOAT(atanf(-inf), -PI / 2.0f);
    CHECK_FLOAT(atan2f(inf, inf), PI / 4.0f);
    CHECK_FLOAT(atan2f(inf, -inf), 3.0f * PI / 4.0f);
    CHECK_FLOAT(atan2f(-inf, inf), -PI / 4.0f);
    CHECK_FLOAT(atan2f(-inf, -inf), -3.0f * PI / 4.0f);
    CHECK(isnan(hypotf(zero, nan)));
    CHECK(isnan(hypotf(nan, zero)));
    CHECK(hypotf(inf, nan) == inf);
    CHECK(exp2f(-127.0f) == 0x1p-127f);
    CHECK(exp2f(-149.0f) == 0x1p-149f);
    CHECK(exp2f(-150.0f) == 0.0f);
    CHECK(exp2f(-149.5f) == 0x1p-149f);
    CHECK(exp2f(-inf) == 0.0f);
    CHECK(exp2f(inf) == inf);
    CHECK(float_bits(powf(negzero, 3.0f)) == 0x80000000u);
    CHECK(float_bits(powf(negzero, -3.0f)) == 0xff800000u);
    CHECK(float_bits(powf(negzero, 2.0f)) == 0);
    CHECK(isnan(powf(zero, nan)));
    CHECK(powf(-1.0f, inf) == 1.0f);
    CHECK(powf(2.0f, inf) == inf);
    CHECK(powf(0.5f, inf) == 0.0f);
    CHECK(powf(2.0f, -inf) == 0.0f);
    CHECK(powf(0.5f, -inf) == inf);
    CHECK(powf(-inf, 0.5f) == inf);
    CHECK(powf(-inf, 3.0f) == -inf);
    CHECK(float_bits(powf(-inf, -3.0f)) == 0x80000000u);
    CHECK(isnan(powf(-2.0f, 0.5f)));
    CHECK(powf(nan, zero) == 1.0f);
    CHECK(powf(1.0f, nan) == 1.0f);

    CHECK_FLOAT(sqrtf(2.0f), 1.41421356f);
    CHECK_FLOAT(sqrtf(1e-30f), 1e-15f);
    CHECK_EXACT(sqrtf(0.0f), 0.0f);
    CHECK(isnan(sqrtf(-1.0f)));
    CHECK(isinf(sqrtf(__builtin_inff())));
    bool squares = true;
    for (int i = 1; i <= 1000; i++)
        squares &= fabsf(sqrtf((float)(i * i)) - i) <= i * 2e-7f;
    CHECK(squares);

    CHECK_FLOAT(sinf(PI / 6.0f), 0.5f);
    CHECK_FLOAT(cosf(PI / 3.0f), 0.5f);
    CHECK_FLOAT(sinf(1000.0f), 0.82687954f);
    CHECK_FLOAT(cosf(1000.0f), 0.56237907f);
    CHECK_FLOAT(tanf(PI / 4.0f), 1.0f);
    CHECK_FLOAT(atan2f(1.0f, 1.0f), PI / 4.0f);
    CHECK_EXACT(atan2f(-0.0f, -1.0f), -PI);
    CHECK_FLOAT(hypotf(3e30f, 4e30f), 5e30f);

    CHECK_EXACT(exp2f(10.0f), 1024.0f);
    CHECK_FLOAT(exp2f(127.7f), 2.7639458e38f);
    CHECK_FLOAT(log2f(8.0f), 3.0f);
    CHECK_FLOAT(log2f(1e-40f), -132.8771f);
    CHECK_FLOAT(powf(2.0f, 0.5f), 1.41421356f);
    CHECK_EXACT(powf(-2.0f, 3.0f), -8.0f);

    CHECK_EXACT(fmodf(10.0f, 3.0f), 1.0f);
    CHECK_EXACT(fmodf(-7.5f, 2.0f), -1.5f);
    CHECK_EXACT(fmodf(1e8f, 3.0f), 1.0f);
    CHECK_EXACT(floorf(-1.5f), -2.0f);
    CHECK_EXACT(floorf(-0.5f), -1.0f);
    CHECK_EXACT(floorf(2.0f), 2.0f);
}

// The PowerPC 750 has no 64-bit integer instructions, compiler-rt does this math
static void test_compiler_rt(void) {
    volatile int64_t a = -1234567890123456789LL, b = 987654321;
    volatile uint64_t c = 0xfedcba9876543210ULL, d = 1000000007, e = 0x123456789ULL, f = 0x9876543;
    volatile int shift = 13;
    CHECK(a / b == -1249999988LL);
    CHECK(a % b == -725308641LL);
    CHECK(c / d == 18364758415ULL);
    CHECK(c % d == 939755815ULL);
    CHECK(e * f == 781230997663524315ULL);
    CHECK((0x0123456789abcdefULL << shift) == 7542668687916785664ULL);
    CHECK((c >> shift) == 2241791814513313ULL);
    CHECK((-0x0123456789abcdefLL >> shift) == -10007999171935LL);

    volatile int64_t big = 1LL << 40;
    volatile float big_float = 1e12f;
    volatile double huge = 1e19;
    CHECK((float)big == 1099511627776.0f);
    CHECK((int64_t)big_float == 999999995904LL);
    CHECK((uint64_t)huge == 10000000000000000000ULL);
}

static void* count_up(void* argument) {
    mutex_t* mutex = argument;
    static int dummy;
    for (int i = 0; i < 1000; i++) {
        LWP_MutexLock(*mutex);
        dummy++;
        LWP_MutexUnlock(*mutex);
    }
    return &dummy;
}

static void test_libogc(void) {
    // The paired single matrix functions must match the plain C ones
    Mtx a, b, c_result, ps_result, c_identity, ps_identity;
    c_guMtxRotRad(a, 'x', 0.7f);
    c_guMtxRotRad(b, 'y', -1.3f);
    c_guMtxConcat(a, b, c_result);
    ps_guMtxConcat(a, b, ps_result);
    c_guMtxIdentity(c_identity);
    ps_guMtxIdentity(ps_identity);
    bool concat_ok = true, identity_ok = true;
    for (int row = 0; row < 3; row++) {
        for (int column = 0; column < 4; column++) {
            concat_ok &= fabsf(c_result[row][column] - ps_result[row][column]) < 1e-6f;
            identity_ok &= c_identity[row][column] == ps_identity[row][column];
        }
    }
    CHECK(concat_ok);
    CHECK(identity_ok);
    guVector c_vector = {3.0f, -4.0f, 12.0f}, ps_vector = c_vector;
    c_guVecNormalize(&c_vector);
    ps_guVecNormalize(&ps_vector);
    CHECK_FLOAT(ps_vector.x, c_vector.x);
    CHECK_FLOAT(ps_vector.z, c_vector.z);

    // Quantized loads and stores convert integers with the GQRs
    CAST_Init();
    u8 byte = 200;
    s16 word = -1234;
    CHECK_EXACT(__castu8f32(&byte), 200.0f);
    CHECK_EXACT(__casts16f32(&word), -1234.0f);
    CHECK(__castf32u8(77.0f) == 77);

    u64 start = gettime();
    usleep(20000);
    u32 slept = diff_usec(start, gettime());
    CHECK(slept >= 20000 && slept < 1000000);

    // Two threads counting under one mutex
    mutex_t mutex;
    lwp_t thread;
    LWP_MutexInit(&mutex, false);
    CHECK(LWP_CreateThread(&thread, count_up, &mutex, NULL, 16 * 1024, 50) == 0);
    int* counter = count_up(&mutex);
    void* result;
    LWP_JoinThread(thread, &result);
    CHECK(result == counter && *counter == 2000);
    LWP_MutexDestroy(mutex);
}

// gxtexconv converted src/assets/test.png, its pixel (x, y) is (x * 32, y * 32, 128, 255)
static void test_texture(void) {
    TPLFile tpl;
    CHECK(TPL_OpenTPLFromMemory(&tpl, (void*)test_tpl, sizeof(test_tpl)) == 1);
    u32 format;
    u16 width, height;
    CHECK(TPL_GetTextureInfo(&tpl, 0, &format, &width, &height) == 0);
    CHECK(format == GX_TF_RGBA8 && width == 8 && height == 8);

    // RGBA8 stores 4x4 tiles as 16 alpha red pairs and then 16 green blue pairs
    const uint32_t* words = (const uint32_t*)test_tpl;
    const uint8_t* image = test_tpl + ((const uint32_t*)(test_tpl + words[3]))[2];
    int pixel = 2 * 4 + 1;
    CHECK(image[pixel * 2] == 255 && image[pixel * 2 + 1] == 32);
    CHECK(image[32 + pixel * 2] == 64 && image[32 + pixel * 2 + 1] == 128);
    TPL_CloseTPLFile(&tpl);
}

int main(void) {
    ISFS_Initialize();
    ISFS_Delete(RESULTS_PATH);
    ISFS_CreateFile(RESULTS_PATH, 0, ISFS_OPEN_RW, ISFS_OPEN_RW, ISFS_OPEN_RW);

    run("startup", test_startup);
    run("memory and strings", test_memory);
    run("malloc", test_malloc);
    run("printf", test_printf);
    run("math", test_math);
    run("compiler-rt", test_compiler_rt);
    run("libogc", test_libogc);
    run("gxtexconv texture", test_texture);

    report("DONE %d failed\n", failures);
    save_results();
    // Returning reloads IOS, which clears /tmp, so wait until `build.sh test` stops Dolphin
    while (true)
        LWP_YieldThread();
}
