// Small float math functions, accurate enough for games. The DS has no floating point unit, so
// rounding and square roots work on the bits as integers and the rest uses few float operations.

#include <math.h>
#include <stdint.h>

#define PI 3.14159265358979f

typedef union {
    float f;
    uint32_t i;
} Bits;

float fabsf(float x) {
    Bits bits = {x};
    bits.i &= 0x7fffffffu;
    return bits.f;
}

// Clears the fraction bits of the mantissa, rounding away from zero when up is set
static float round_bits(float x, bool up) {
    Bits bits = {x};
    int32_t exponent = (int32_t)((bits.i >> 23) & 255) - 127;
    if (exponent >= 23)  // Already an integer, infinite or NaN
        return x;
    if (exponent < 0) {
        // |x| < 1 becomes 0 or 1 with the sign of x
        bits.i &= 0x80000000u;
        if (up && (Bits){x}.i << 1)
            bits.i |= 0x3f800000u;
        return bits.f;
    }
    uint32_t fraction = 0x007fffffu >> exponent;
    if ((bits.i & fraction) == 0)
        return x;
    if (up)
        bits.i += fraction;
    bits.i &= ~fraction;
    return bits.f;
}

static float truncf(float x) {
    return round_bits(x, false);
}

float floorf(float x) {
    return round_bits(x, x < 0.0f);
}

float ceilf(float x) {
    return round_bits(x, x > 0.0f);
}

// Rounds halves away from zero. x - truncf(x) is exact, adding 0.5 first would round.
static float roundf(float x) {
    float t = truncf(x);
    if (fabsf(x - t) >= 0.5f)
        t += x < 0.0f ? -1.0f : 1.0f;
    return t;
}

// Rounds halves to even, like the default rounding mode
long lrintf(float x) {
    float t = truncf(x);
    if (fabsf(x - t) == 0.5f)
        return (long)(2.0f * roundf(x * 0.5f));
    return (long)roundf(x);
}

// Exact like the standard asks: subtracts y doubled as often as fits, from big to small. Each
// subtraction is exact because the remainder is always less than twice what is subtracted.
float fmodf(float x, float y) {
    if (isnan(x) || isnan(y) || isinf(x) || y == 0.0f)
        return __builtin_nanf("");
    float remainder = fabsf(x), step = fabsf(y);
    if (remainder < step)
        return x;
    while (step + step <= remainder)
        step += step;
    for (;;) {
        if (remainder >= step)
            remainder -= step;
        if (step == fabsf(y))
            break;
        step *= 0.5f;
    }
    return x < 0.0f ? -remainder : remainder;
}

// The square root of the mantissa bit by bit, like newlib's, so it is exact
float sqrtf(float x) {
    Bits bits = {x};
    if ((bits.i << 1) == 0)  // +0 or -0
        return x;
    if (bits.i >= 0x7f800000u)  // Negative, infinite or NaN
        return bits.i == 0x7f800000u ? x : __builtin_nanf("");
    uint32_t mantissa = bits.i;
    int32_t exponent = (int32_t)(mantissa >> 23);
    if (exponent == 0) {
        // Normalize a denormal number
        for (; !(mantissa & 0x00800000u); exponent--)
            mantissa <<= 1;
        exponent++;
    }
    exponent -= 127;
    mantissa = (mantissa & 0x007fffffu) | 0x00800000u;
    if (exponent & 1)
        mantissa <<= 1;
    exponent >>= 1;

    mantissa <<= 1;
    uint32_t root = 0, sum = 0;
    for (uint32_t bit = 0x01000000u; bit; bit >>= 1) {
        uint32_t trial = sum + bit;
        if (trial <= mantissa) {
            sum = trial + bit;
            mantissa -= trial;
            root += bit;
        }
        mantissa <<= 1;
    }
    // Round to nearest, ties to even
    if (mantissa)
        root += root & 1;
    bits.i = (root >> 1) + 0x3f000000u + ((uint32_t)exponent << 23);
    return bits.f;
}

// Reduces x to [-pi, pi]. 2 pi is split in a part with few bits, so turns * 2 pi is exact
// without double precision, this keeps the result accurate up to about |x| < 400000.
static float reduce_angle(float x) {
    float turns = floorf(x * (0.5f / PI) + 0.5f);
    return (x - turns * 6.28125f) - turns * 1.9353071795864769e-3f;
}

// sin(x) for x in [-3 pi / 2, 3 pi / 2]
static float sin_reduced(float x) {
    // Fold into [-pi/2, pi/2]
    if (x > PI / 2.0f)
        x = PI - x;
    else if (x < -PI / 2.0f)
        x = -PI - x;
    // Taylor series up to x^11
    float x2 = x * x;
    return x * (1.0f + x2 * (-1.0f / 6.0f +
                             x2 * (1.0f / 120.0f +
                                   x2 * (-1.0f / 5040.0f + x2 * (1.0f / 362880.0f + x2 * (-1.0f / 39916800.0f))))));
}

float sinf(float x) {
    return sin_reduced(reduce_angle(x));
}

float cosf(float x) {
    return sin_reduced(reduce_angle(x) + PI / 2.0f);
}

float tanf(float x) {
    return sinf(x) / cosf(x);
}

float atanf(float x) {
    // Abramowitz and Stegun 4.4.49 for |x| <= 1
    bool invert = fabsf(x) > 1.0f;
    if (invert)
        x = 1.0f / x;
    float x2 = x * x;
    float y =
        x *
        (0.9999993329f +
         x2 * (-0.3332985605f +
               x2 * (0.1994653599f + x2 * (-0.1390853351f +
                                           x2 * (0.0964200441f + x2 * (-0.0559098861f +
                                                                       x2 * (0.0218612288f + x2 * -0.0040540580f)))))));
    if (invert)
        y = (x > 0.0f ? PI / 2.0f : -PI / 2.0f) - y;
    return y;
}

float atan2f(float y, float x) {
    if (isnan(x) || isnan(y))
        return x + y;
    // The sign of y picks the half, also for -0
    float pi = __builtin_signbit(y) ? -PI : PI;
    if (x > 0.0f)
        return atanf(y / x);
    if (x < 0.0f)
        return atanf(y / x) + pi;
    if (y != 0.0f)
        return pi / 2.0f;
    return __builtin_signbit(x) ? pi : y;
}

// The angles of the sides of a right triangle with hypotenuse 1, nan outside [-1, 1]
float asinf(float x) {
    return fabsf(x) > 1.0f ? NAN : atan2f(x, sqrtf((1.0f - x) * (1.0f + x)));
}

float acosf(float x) {
    return fabsf(x) > 1.0f ? NAN : atan2f(sqrtf((1.0f - x) * (1.0f + x)), x);
}

// 2^x as 2^i * e^(f * ln 2), the fraction is done with a Taylor series of e^x
float exp2f(float x) {
    if (isnan(x))
        return x;
    if (x >= 128.0f)
        return __builtin_inff();
    if (x < -126.0f)
        return 0.0f;
    float i = floorf(x + 0.5f);
    float f = (x - i) * 0.69314718f;
    float y =
        1.0f + f * (1.0f + f * (1.0f / 2.0f +
                                f * (1.0f / 6.0f + f * (1.0f / 24.0f + f * (1.0f / 120.0f + f * (1.0f / 720.0f))))));
    // x rounds up to 128 above 127.5, 2^128 doesn't fit in a float so y gets that last factor 2
    int32_t exponent = (int32_t)i;
    if (exponent > 127) {
        exponent = 127;
        y *= 2.0f;
    }
    Bits scale = {.i = (uint32_t)(exponent + 127) << 23};
    return y * scale.f;
}

float expf(float x) {
    return exp2f(x * 1.44269504f);
}

// log2(x), split x into 2^e * m and use the atanh series for log(m)
float log2f(float x) {
    if (!(x > 0.0f))
        return x == 0.0f ? -__builtin_inff() : __builtin_nanf("");
    if (isinf(x))
        return x;
    // Scale subnormals up by 2^23 so they get an exponent
    int32_t e = -127;
    if (x < 1.17549435e-38f) {
        x *= 8388608.0f;
        e -= 23;
    }
    Bits bits = {x};
    e += (int32_t)((bits.i >> 23) & 255);
    bits.i = (bits.i & 0x007fffff) | 0x3f800000;
    float m = bits.f;
    if (m > 1.41421356f) {
        m *= 0.5f;
        e++;
    }
    float s = (m - 1.0f) / (m + 1.0f);
    float s2 = s * s;
    float ln = 2.0f * s * (1.0f + s2 * (1.0f / 3.0f + s2 * (1.0f / 5.0f + s2 * (1.0f / 7.0f + s2 * (1.0f / 9.0f)))));
    return (float)e + ln * 1.44269504f;
}

float powf(float x, float y) {
    if (y == 0.0f || x == 1.0f)
        return 1.0f;
    if (x == 0.0f)
        return y > 0.0f ? 0.0f : __builtin_inff();
    if (x < 0.0f) {
        // Only integer powers of negative numbers are real
        if (truncf(y) != y)
            return __builtin_nanf("");
        float result = powf(-x, y);
        return fabsf(fmodf(y, 2.0f)) == 1.0f ? -result : result;
    }
    return exp2f(y * log2f(x));
}

float tanhf(float x) {
    if (fabsf(x) > 9.0f)
        return x > 0.0f ? 1.0f : -1.0f;
    float e = expf(2.0f * x);
    return (e - 1.0f) / (e + 1.0f);
}
