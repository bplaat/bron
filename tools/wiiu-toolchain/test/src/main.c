// Tests the toolchain on the Wii U: the startup code, the C library, compiler-rt, wut, libgu and
// the tools. Results go through wutpicolibc and OSReport, where `build.sh test` reads them in
// cemu-runner.
#include <assert.h>
#include <coreinit/time.h>
#include <errno.h>
#include <gfd.h>
#include <gu.h>
#include <limits.h>
#include <malloc.h>
#include <math.h>
#include <setjmp.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/time.h>
#include <time.h>

#define PI 3.14159265358979f

static int failures;
static bool main_finished;
static void check(bool ok, int line, const char* text) {
    if (!ok) {
        failures++;
        printf("  line %d: %s\n", line, text);
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
    bool ok = isnan(expected) ? isnan(actual)
                              : actual == expected || (isfinite(actual) && isfinite(expected) &&
                                                       fabsf(actual - expected) <= tolerance * fabsf(expected));
    if (!ok) {
        failures++;
        printf("  line %d: %s = %08lx, expected %08lx\n", line, text, (unsigned long)float_bits(actual),
               (unsigned long)float_bits(expected));
    }
}

static void check_string(const char* actual, const char* expected, int line) {
    if (strcmp(actual, expected) != 0) {
        failures++;
        printf("  line %d: \"%s\", expected \"%s\"\n", line, actual, expected);
    }
}

#define CHECK(condition) check(condition, __LINE__, #condition)
#define CHECK_FLOAT(actual, expected) check_float(actual, expected, 2e-6f, __LINE__, #actual)
#define CHECK_EXACT(actual, expected) check_float(actual, expected, 0.0f, __LINE__, #actual)
#define CHECK_FORMAT(expected, ...) (snprintf(text, sizeof(text), __VA_ARGS__), check_string(text, expected, __LINE__))

static void run(const char* name, void (*test)(void)) {
    int before = failures;
    test();
    printf("%s %s\n", failures == before ? "ok  " : "FAIL", name);
    fflush(stdout);
}

static int data_value = 1234;
static int bss_values[256];
static int constructed;
__attribute__((constructor)) static void construct(void) {
    constructed = data_value + 1;
}
static void test_startup(void) {
    CHECK(data_value == 1234 && constructed == 1235);
    for (int i = 0; i < 256; i++)
        CHECK(bss_values[i] == 0);
}

// Exercise the patched word copies at every source and destination alignment and both overlapping
// move directions. Volatile function pointers prevent clang replacing libc with its builtins.
static void test_memory(void) {
    void* (*volatile copy)(void*, const void*, size_t) = memcpy;
    void* (*volatile move)(void*, const void*, size_t) = memmove;
    void* (*volatile fill)(void*, int, size_t) = memset;
    uint8_t source[192], buffer[192], expected[192];
    for (int i = 0; i < 192; i++)
        source[i] = (uint8_t)(i * 7 + 1);
    bool copied = true, moved = true, filled = true;
    for (int to = 0; to < 8; to++) {
        for (int from = 0; from < 8; from++) {
            for (int size = 0; size <= 100; size++) {
                for (int i = 0; i < 192; i++)
                    buffer[i] = expected[i] = 0xee;
                for (int i = 0; i < size; i++)
                    expected[to + i] = source[from + i];
                copy(buffer + to, source + from, size);
                copied &= memcmp(buffer, expected, sizeof(buffer)) == 0;
            }
        }
    }
    for (int shift = -9; shift <= 9; shift++) {
        for (int size = 0; size <= 100; size++) {
            copy(buffer, source, sizeof(buffer));
            copy(expected, source, sizeof(expected));
            for (int i = 0; i < size; i++)
                expected[40 + shift + i] = source[40 + i];
            move(buffer + 40 + shift, buffer + 40, size);
            moved &= memcmp(buffer, expected, sizeof(buffer)) == 0;
        }
    }
    for (int offset = 0; offset < 8; offset++) {
        for (int size = 0; size <= 180; size += 3) {
            for (int value = 0; value <= 0xa5; value += 0xa5) {
                for (int i = 0; i < 192; i++)
                    buffer[i] = expected[i] = 0xee;
                for (int i = 0; i < size; i++)
                    expected[offset + i] = (uint8_t)value;
                fill(buffer + offset, value, size);
                filled &= memcmp(buffer, expected, sizeof(buffer)) == 0;
            }
        }
    }
    CHECK(copied && moved && filled);
    CHECK(strlen("blocks") == 6 && strcmp("abc", "abd") < 0);
    CHECK(strtol("-0x1f", NULL, 16) == -31);
}

static void test_heap(void) {
    uint8_t* pointer = malloc(100);
    CHECK(pointer != NULL && ((uintptr_t)pointer & 63) == 0);
    if (pointer) {
        memset(pointer, 0x5a, 100);
        uint8_t* grown = realloc(pointer, 5000);
        CHECK(grown != NULL);
        if (grown) {
            for (int i = 0; i < 100; i++)
                CHECK(grown[i] == 0x5a);
            uint8_t* shrunk = realloc(grown, 16);
            CHECK(shrunk != NULL);
            if (shrunk) {
                for (int i = 0; i < 16; i++)
                    CHECK(shrunk[i] == 0x5a);
                free(shrunk);
            } else {
                free(grown);
            }
        } else {
            free(pointer);
        }
    }
    uint8_t* zero = calloc(10, 100);
    CHECK(zero != NULL);
    if (zero) {
        for (int i = 0; i < 1000; i++)
            CHECK(zero[i] == 0);
        free(zero);
    }
    volatile size_t huge = SIZE_MAX;
    CHECK(calloc(huge, 2) == NULL);
    for (size_t alignment = 64; alignment <= 4096; alignment *= 2) {
        void* aligned = memalign(alignment, 1000);
        CHECK(aligned != NULL && ((uintptr_t)aligned & (alignment - 1)) == 0);
        free(aligned);
    }
    free(NULL);
}

static void test_printf(void) {
    char text[64];
    CHECK_FORMAT("42", "%d", 42);
    CHECK_FORMAT("  -42|", "%5d|", -42);
    CHECK_FORMAT("42   |", "%-5d|", 42);
    CHECK_FORMAT("-0042", "%05d", -42);
    CHECK_FORMAT("007", "%.3d", 7);
    CHECK_FORMAT("-0000000000000000000000001", "%.25d", -1);
    CHECK_FORMAT("    -0000000000000000000000001", "%030.25d", -1);
    CHECK_FORMAT("", "%.0d", 0);
    CHECK(snprintf(text, 5, "%.25d", 1) == 25);
    check_string(text, "0000", __LINE__);
    CHECK(snprintf(NULL, 0, "%.25d", 1) == 25);
    CHECK_FORMAT("4000000000", "%u", 4000000000u);
    CHECK_FORMAT("beef BEEF 0000BEEF", "%x %X %08X", 0xbeef, 0xbeef, 0xbeef);
    CHECK_FORMAT("-1234567890123", "%lld", -1234567890123LL);
    CHECK_FORMAT("123456789abcdef", "%llx", 0x123456789abcdefULL);
    CHECK_FORMAT("7 x %", "%zu %c %%", (size_t)7, 'x');
    CHECK_FORMAT("0x10001234", "%p", (void*)0x10001234);
    CHECK_FORMAT("wiiu|wi|(null)", "%s|%.2s|%s", "wiiu", "wiiu", (char*)NULL);
    CHECK_FORMAT("[     1] [2     ] [xy]", "[%*d] [%-*d] [%.*s]", 6, 1, 6, 2, 2, "xyz");
    CHECK(snprintf(text, 5, "hello world") == 11);
    check_string(text, "hell", __LINE__);
    CHECK_FORMAT("3.14 -2.5e+10 0.001 1e+100", "%.2f %g %g %g", 3.14159, -2.5e10, 0.001, 1e100);
    CHECK_FORMAT("1.500000 2 -1.250 +2.0", "%f %.0f %.3f %+.1f", 1.5, 2.4, -1.25, 2.0);
    CHECK_FORMAT("10000000000.0 -0.000 inf nan", "%.1f %.3f %f %f", 1e10, -0.0, (double)INFINITY, (double)NAN);

    int a = 0, b = 0;
    char word[16], c = 0;
    CHECK(sscanf("[12;34m", "[%d;%dm", &a, &b) == 2 && a == 12 && b == 34);
    CHECK(sscanf("  -7 word x", "%d %s %c", &a, word, &c) == 3 && a == -7 && !strcmp(word, "word") && c == 'x');
    CHECK(sscanf("ff 0x10", "%x %i", &a, &b) == 2 && a == 255 && b == 16);
    CHECK(sscanf("abc", "%d", &a) == 0);
    CHECK(sscanf("abc123]]x", "%[a-c]%2[0-9]", word, text) == 2 && !strcmp(word, "abc") && !strcmp(text, "12"));
    float f = 0;
    double d = 0;
    int count = 0;
    CHECK(sscanf("1.5 -2.5e2 3x", "%f %lf%n", &f, &d, &count) == 2 && f == 1.5f && d == -250.0 && count == 10);
    CHECK(sscanf("-inf nan", "%f %lf", &f, &d) == 2 && isinf(f) && f < 0 && isnan(d));
    CHECK(sscanf("", "%d", &a) == EOF);
}

// stdout and stderr go to OSReport
static void test_streams(void) {
    CHECK(fputs("stdout through OSReport\n", stdout) >= 0);
    CHECK(fputs("stderr through OSReport\n", stderr) >= 0);
    CHECK(fflush(stdout) == 0 && fflush(stderr) == 0);
}

static int compare_ints(const void* a, const void* b) {
    return *(const int*)a - *(const int*)b;
}

static int compare_chars(const void* a, const void* b) {
    return *(const char*)a - *(const char*)b;
}

static void test_stdlib(void) {
    char* end;
    CHECK(strtol(" -123abc", &end, 10) == -123 && *end == 'a');
    CHECK(strtol("0x1F", &end, 0) == 31 && !*end);
    CHECK(strtol("017", NULL, 0) == 15);
    CHECK(strtoul("4294967295", NULL, 10) == 4294967295u);
    errno = 0;
    CHECK(strtol("99999999999", NULL, 10) == LONG_MAX && errno == ERANGE);
    CHECK(atoi("42") == 42 && atol("-123456789") == -123456789L && abs(-5) == 5);
    CHECK(strtof(" -1.5e3x", &end) == -1500.0f && *end == 'x');
    CHECK(strtof("0.1", NULL) == 0.1f && strtof("123456.7", NULL) == 123456.7f && atof(".25") == 0.25);
    CHECK(strtod("3.14159f", &end) == 3.14159 && *end == 'f');
    CHECK(isinf(strtof("-Infinity", &end)) && !*end && isnan(strtof("nan", NULL)));
    errno = 0;
    CHECK(isinf(strtof("1e39", NULL)) && errno == ERANGE);
    CHECK(BYTE_ORDER == BIG_ENDIAN);

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

static jmp_buf jump;
static void jump_back(int value) {
    longjmp(jump, value);
}

// A failed assertion comes here instead of the C library's, which would abort the program
static const char* assert_expression;
void __assert_func(const char* file, int line, const char* function, const char* expression) {
    (void)file, (void)line, (void)function;
    assert_expression = expression;
    longjmp(jump, 1);
}

static void test_runtime(void) {
    volatile int jumped = 0;
    int value = setjmp(jump);
    if (value == 0)
        jump_back(7);
    else
        jumped = value;
    CHECK(jumped == 7);
    // The floating point registers are callee saved too
    volatile double kept = 1.25;
    if (setjmp(jump) == 0)
        jump_back(1);
    CHECK(kept == 1.25);
    static volatile int zero = 0;
    if (setjmp(jump) == 0)
        assert(zero == 1);
    CHECK(assert_expression && !strcmp(assert_expression, "zero == 1"));
}

static void test_math(void) {
    // Volatile inputs exercise the libc calls rather than constant folding
    volatile float zero = 0.0f, negzero = -0.0f;
    volatile float inf = __builtin_inff(), nan = __builtin_nanf("");
    CHECK(isnan(sqrtf(-inf)));
    CHECK(float_bits(sqrtf(negzero)) == 0x80000000u);
    CHECK(isnan(sqrtf(nan)));
    CHECK(float_bits(fmodf(negzero, 2.0f)) == 0x80000000u);
    CHECK(float_bits(fmodf(-4.0f, 2.0f)) == 0x80000000u);
    CHECK_FLOAT(atanf(inf), PI / 2.0f);
    CHECK_FLOAT(atan2f(-inf, -inf), -3.0f * PI / 4.0f);
    CHECK(isnan(hypotf(zero, nan)));
    CHECK(hypotf(inf, nan) == inf);
    CHECK(exp2f(-126.0f) == 0x1p-126f);
    CHECK(exp2f(-inf) == 0.0f && exp2f(inf) == inf);
    CHECK(float_bits(powf(negzero, 3.0f)) == 0x80000000u);
    CHECK(float_bits(powf(negzero, -3.0f)) == 0xff800000u);
    CHECK(isnan(powf(zero, nan)));
    CHECK(powf(-1.0f, inf) == 1.0f && powf(0.5f, -inf) == inf && powf(-inf, 3.0f) == -inf);
    CHECK(isnan(powf(-2.0f, 0.5f)));
    CHECK(powf(nan, zero) == 1.0f && powf(1.0f, nan) == 1.0f);

    // Double precision, which Espresso does in hardware
    volatile double half_pi = 1.5707963267948966;
    CHECK(sin(half_pi) == 1.0 && fabs(cos(half_pi)) < 1e-16);
    CHECK(fabs(pow(2.0, 0.5) - 1.4142135623730951) < 1e-16);
    CHECK(fabs(log(exp(10.0)) - 10.0) < 1e-14);

    // sqrt rounds correctly, so squares come back exact
    bool squares = true;
    for (int i = 1; i <= 1000; i++)
        squares &= sqrtf((float)(i * i)) == i && sqrt((double)i * i) == i;
    CHECK(squares);
    volatile double two = 2.0, below_four = 0x1.fffffffffffffp+1;
    CHECK(float_bits(sqrtf((float)two)) == 0x3fb504f3u);
    CHECK(sqrt(two) == 0x1.6a09e667f3bcdp+0);
    CHECK(sqrt(below_four) == 0x1.fffffffffffffp+0);

    CHECK_FLOAT(sinf(PI / 6.0f), 0.5f);
    CHECK_FLOAT(cosf(PI / 3.0f), 0.5f);
    CHECK_FLOAT(sinf(1000.0f), 0.82687954f);
    CHECK_FLOAT(cosf(1000.0f), 0.56237907f);
    CHECK_FLOAT(tanf(PI / 4.0f), 1.0f);
    CHECK_EXACT(atan2f(-0.0f, -1.0f), -PI);
    CHECK_FLOAT(hypotf(3e30f, 4e30f), 5e30f);
    CHECK_EXACT(exp2f(10.0f), 1024.0f);
    CHECK_FLOAT(log2f(8.0f), 3.0f);
    CHECK_FLOAT(powf(2.0f, 0.5f), 1.41421356f);
    CHECK_EXACT(powf(-2.0f, 3.0f), -8.0f);
    CHECK_EXACT(fmodf(-7.5f, 2.0f), -1.5f);
    CHECK_EXACT(fmodf(1e8f, 3.0f), 1.0f);
    CHECK_EXACT(floorf(-0.5f), -1.0f);
}

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

static void test_matrices(void) {
    Mtx a, b, expected, actual, identity;
    c_guMtxRotRad(a, 'y', 0.7f);
    c_guMtxTrans(b, 1.0f, -2.0f, 3.0f);
    c_guMtxConcat(a, b, expected);
    ps_guMtxConcat(a, b, actual);
    ps_guMtxIdentity(identity);
    for (int row = 0; row < 3; row++) {
        for (int column = 0; column < 4; column++) {
            CHECK_FLOAT(actual[row][column], expected[row][column]);
            CHECK(identity[row][column] == (row == column ? 1.0f : 0.0f));
        }
    }
    guVector vector = {3.0f, -4.0f, 12.0f};
    ps_guVecNormalize(&vector);
    CHECK_FLOAT(vector.x, 3.0f / 13.0f);
    CHECK_FLOAT(vector.y, -4.0f / 13.0f);
    CHECK_FLOAT(vector.z, 12.0f / 13.0f);
}

_Alignas(4096) static const uint8_t textures[] = {
#embed "../../../../target/wiiu-toolchain/test/fixtures.gtx"
};
static void test_textures(void) {
    GX2Texture texture;
    CHECK(GFDGetTextureInPlace(&texture, 0, textures));
    CHECK(texture.surface.width == 32 && texture.surface.height == 32);
    CHECK(texture.surface.format == GX2_SURFACE_FORMAT_UNORM_BC1);
    CHECK(texture.surface.mipLevels == 6 && texture.surface.mipmapSize > 0);
    CHECK((const uint8_t*)texture.surface.image >= textures &&
          (const uint8_t*)texture.surface.image < textures + sizeof(textures));
    CHECK(((uintptr_t)texture.surface.image & (texture.surface.alignment - 1)) == 0);
    CHECK((const uint8_t*)texture.surface.mipmaps >= textures &&
          (const uint8_t*)texture.surface.mipmaps < textures + sizeof(textures));
    CHECK(GFDGetTextureInPlace(&texture, 1, textures));
    CHECK(texture.surface.format == GX2_SURFACE_FORMAT_UNORM_R8_G8_B8_A8);
    CHECK(texture.surface.mipLevels == 1 && texture.surface.mipmapSize == 0);
    CHECK(!GFDGetTextureInPlace(&texture, 2, textures));
    CHECK(!GFDGetTextureInPlace(NULL, 0, textures));
}

_Alignas(64) static const uint8_t shader[] = {
#embed "../../../../target/wiiu-toolchain/test/fixture.gsh"
};
static void test_shaders(void) {
    CHECK(GFDGetVertexShaderCount(shader) == 1 && GFDGetPixelShaderCount(shader) == 1);
    uint32_t header_size = GFDGetVertexShaderHeaderSize(0, shader);
    uint32_t program_size = GFDGetVertexShaderProgramSize(0, shader);
    GX2VertexShader* vertex = malloc(header_size);
    void* program = memalign(256, program_size);
    CHECK(vertex != NULL && program != NULL && program_size > 0);
    if (vertex && program) {
        CHECK(GFDGetVertexShader(vertex, program, 0, shader));
        CHECK(vertex->program == program && vertex->size == program_size);
        CHECK(vertex->attribVarCount == 2 && vertex->uniformVarCount == 2);
        CHECK(strcmp(vertex->attribVars[0].name, "aPosition") == 0);
        CHECK(strcmp(vertex->attribVars[1].name, "aTexCoord") == 0);
        CHECK(strcmp(vertex->uniformVars[0].name, "uProjection") == 0 && vertex->uniformVars[0].offset == 0);
        CHECK(strcmp(vertex->uniformVars[1].name, "uModelView") == 0 && vertex->uniformVars[1].offset == 16);
    }
    free(vertex);
    free(program);
    header_size = GFDGetPixelShaderHeaderSize(0, shader);
    program_size = GFDGetPixelShaderProgramSize(0, shader);
    GX2PixelShader* pixel = malloc(header_size);
    program = memalign(256, program_size);
    CHECK(pixel != NULL && program != NULL && program_size > 0);
    if (pixel && program) {
        CHECK(GFDGetPixelShader(pixel, program, 0, shader));
        CHECK(pixel->program == program && pixel->size == program_size);
        CHECK(pixel->samplerVarCount == 1);
        CHECK(strcmp(pixel->samplerVars[0].name, "uTexture") == 0);
    }
    free(pixel);
    free(program);
}

static void test_clock(void) {
    struct timeval before, after;
    CHECK(gettimeofday(&before, NULL) == 0);
    CHECK(before.tv_sec >= 1767225600 && before.tv_usec >= 0 && before.tv_usec < 1000000);
    OSTime start = OSGetTime();
    volatile unsigned sum = 0;
    for (unsigned i = 0; i < 10000; i++)
        sum += i;
    CHECK(OSGetTime() > start && sum == 49995000);
    CHECK(gettimeofday(&after, NULL) == 0);
    CHECK(after.tv_sec > before.tv_sec || (after.tv_sec == before.tv_sec && after.tv_usec > before.tv_usec));
    CHECK(time(NULL) >= before.tv_sec);
}

__attribute__((destructor)) static void finalize(void) {
    CHECK(main_finished);
    printf("DONE %d failed\n", failures);
    fflush(stdout);
}

int main(void) {
    run("startup and constructors", test_startup);
    run("memory and strings", test_memory);
    run("heap and alignment", test_heap);
    run("printf and scanf", test_printf);
    run("standard streams", test_streams);
    run("stdlib and strings", test_stdlib);
    run("setjmp and assert", test_runtime);
    run("math", test_math);
    run("compiler-rt", test_compiler_rt);
    run("paired single matrices", test_matrices);
    run("gtxconv and GFD textures", test_textures);
    run("Latte shaders and GFD relocation", test_shaders);
    run("clock", test_clock);
    main_finished = true;
    return failures ? 1 : 0;
}
