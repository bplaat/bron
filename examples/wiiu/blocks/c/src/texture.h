#pragma once

#include <gx2/mem.h>
#include <gx2/texture.h>
#include <gx2/utils.h>
#include <limits.h>
#include <malloc.h>
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>
#include <string.h>

#define STBI_ONLY_PNG
#define STBI_NO_STDIO
#define STBI_NO_LINEAR
#define STBI_NO_THREAD_LOCALS
#define STB_IMAGE_IMPLEMENTATION
#pragma GCC diagnostic push
#pragma GCC diagnostic ignored "-Wunused-function"
#include "stb_image.h"
#pragma GCC diagnostic pop

// Decodes a PNG image to an RGBA8 texture. The rows stay linear, which the GPU reads without
// tiling, they only need the pitch GX2 calculates.
static bool texture_load(GX2Texture* texture, const uint8_t* data, size_t size) {
    if (size > INT_MAX)
        return false;
    int width, height, channels;
    uint8_t* pixels = stbi_load_from_memory(data, (int)size, &width, &height, &channels, 4);
    if (!pixels)
        return false;

    memset(texture, 0, sizeof(GX2Texture));
    texture->surface.dim = GX2_SURFACE_DIM_TEXTURE_2D;
    texture->surface.width = (uint32_t)width;
    texture->surface.height = (uint32_t)height;
    texture->surface.depth = 1;
    texture->surface.mipLevels = 1;
    texture->surface.format = GX2_SURFACE_FORMAT_UNORM_R8_G8_B8_A8;
    texture->surface.use = GX2_SURFACE_USE_TEXTURE;
    texture->surface.tileMode = GX2_TILE_MODE_LINEAR_ALIGNED;
    texture->viewNumMips = 1;
    texture->viewNumSlices = 1;
    texture->compMap = GX2_COMP_MAP(GX2_SQ_SEL_R, GX2_SQ_SEL_G, GX2_SQ_SEL_B, GX2_SQ_SEL_A);
    GX2CalcSurfaceSizeAndAlignment(&texture->surface);
    GX2InitTextureRegs(texture);

    texture->surface.image = memalign(texture->surface.alignment, texture->surface.imageSize);
    if (!texture->surface.image) {
        stbi_image_free(pixels);
        return false;
    }
    for (int y = 0; y < height; y++) {
        memcpy((uint8_t*)texture->surface.image + (size_t)y * texture->surface.pitch * 4,
               pixels + (size_t)y * (size_t)width * 4, (size_t)width * 4);
    }
    stbi_image_free(pixels);
    GX2Invalidate(GX2_INVALIDATE_MODE_CPU_TEXTURE, texture->surface.image, texture->surface.imageSize);
    return true;
}

static void texture_free(GX2Texture* texture) {
    free(texture->surface.image);
    texture->surface.image = NULL;
}
