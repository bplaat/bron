#pragma once

#include <gfd.h>
#include <gx2/sampler.h>
#include <gx2/texture.h>
#include <stdbool.h>
#include <stdint.h>

typedef enum MaterialLayer {
    MaterialLayerBrickRed,
    MaterialLayerCactusSide,
    MaterialLayerCactusTop,
    MaterialLayerDirt,
    MaterialLayerDirtGrass,
    MaterialLayerGrassTop,
    MaterialLayerGreystone,
    MaterialLayerLava,
    MaterialLayerLeaves,
    MaterialLayerSand,
    MaterialLayerStone,
    MaterialLayerStoneCoal,
    MaterialLayerStoneDiamond,
    MaterialLayerStoneGold,
    MaterialLayerStoneIron,
    MaterialLayerTrunkSide,
    MaterialLayerTrunkTop,
    MaterialLayerWater,
    MaterialLayerWood,
    MaterialLayerCount,
} MaterialLayer;

// The GPU reads the BC1 textures in place, which needs the 4 KiB alignment of their tiles
_Alignas(4096) static const uint8_t textures_gtx[] = {
#embed "../target/textures.gtx"
};

typedef struct {
    GX2Texture textures[MaterialLayerCount];
    GX2Sampler sampler;
} Materials;

static bool materials_load(Materials* materials) {
    for (int material = 0; material < MaterialLayerCount; material++) {
        if (!GFDGetTextureInPlace(&materials->textures[material], (uint32_t)material, textures_gtx))
            return false;
    }
    // Sharp texels up close like the other block renderers, blended mipmaps in the distance
    GX2InitSampler(&materials->sampler, GX2_TEX_CLAMP_MODE_WRAP, GX2_TEX_XY_FILTER_MODE_POINT);
    GX2InitSamplerZMFilter(&materials->sampler, GX2_TEX_Z_FILTER_MODE_POINT, GX2_TEX_MIP_FILTER_MODE_LINEAR);
    return true;
}
