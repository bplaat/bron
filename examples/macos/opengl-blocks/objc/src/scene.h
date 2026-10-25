#pragma once

#include <math.h>

// Orbit and portrait framing for the 64-cube diorama.
typedef struct {
    float target[3];
    float eye[3];
    float scale;
} BlocksCamera;

static inline BlocksCamera blocks_camera(double seconds, float aspect) {
    float angle = (float)fmod(seconds * 0.16, 2.0 * M_PI);
    float scale = 1.0f / fminf(aspect, 1.0f);
    return (BlocksCamera){
        .target = {32.0f, 21.76f, 32.0f},
        .eye = {32.0f + sinf(angle) * 88.0f * scale, 21.76f + 58.0f * scale, 32.0f + cosf(angle) * 88.0f * scale},
        .scale = scale,
    };
}
