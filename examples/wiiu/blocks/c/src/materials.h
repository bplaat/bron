#pragma once

#include <gx2/sampler.h>
#include <gx2/texture.h>
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

#include "texture.h"

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

static const uint8_t brick_red_png[] = {
#embed "assets/brick_red.png"
};
static const uint8_t cactus_side_png[] = {
#embed "assets/cactus_side.png"
};
static const uint8_t cactus_top_png[] = {
#embed "assets/cactus_top.png"
};
static const uint8_t dirt_png[] = {
#embed "assets/dirt.png"
};
static const uint8_t dirt_grass_png[] = {
#embed "assets/dirt_grass.png"
};
static const uint8_t grass_top_png[] = {
#embed "assets/grass_top.png"
};
static const uint8_t greystone_png[] = {
#embed "assets/greystone.png"
};
static const uint8_t lava_png[] = {
#embed "assets/lava.png"
};
static const uint8_t leaves_png[] = {
#embed "assets/leaves.png"
};
static const uint8_t sand_png[] = {
#embed "assets/sand.png"
};
static const uint8_t stone_png[] = {
#embed "assets/stone.png"
};
static const uint8_t stone_coal_png[] = {
#embed "assets/stone_coal.png"
};
static const uint8_t stone_diamond_png[] = {
#embed "assets/stone_diamond.png"
};
static const uint8_t stone_gold_png[] = {
#embed "assets/stone_gold.png"
};
static const uint8_t stone_iron_png[] = {
#embed "assets/stone_iron.png"
};
static const uint8_t trunk_side_png[] = {
#embed "assets/trunk_side.png"
};
static const uint8_t trunk_top_png[] = {
#embed "assets/trunk_top.png"
};
static const uint8_t water_png[] = {
#embed "assets/water.png"
};
static const uint8_t wood_png[] = {
#embed "assets/wood.png"
};

static const struct {
    const uint8_t* data;
    size_t size;
} material_images[MaterialLayerCount] = {
    {brick_red_png, sizeof(brick_red_png)},
    {cactus_side_png, sizeof(cactus_side_png)},
    {cactus_top_png, sizeof(cactus_top_png)},
    {dirt_png, sizeof(dirt_png)},
    {dirt_grass_png, sizeof(dirt_grass_png)},
    {grass_top_png, sizeof(grass_top_png)},
    {greystone_png, sizeof(greystone_png)},
    {lava_png, sizeof(lava_png)},
    {leaves_png, sizeof(leaves_png)},
    {sand_png, sizeof(sand_png)},
    {stone_png, sizeof(stone_png)},
    {stone_coal_png, sizeof(stone_coal_png)},
    {stone_diamond_png, sizeof(stone_diamond_png)},
    {stone_gold_png, sizeof(stone_gold_png)},
    {stone_iron_png, sizeof(stone_iron_png)},
    {trunk_side_png, sizeof(trunk_side_png)},
    {trunk_top_png, sizeof(trunk_top_png)},
    {water_png, sizeof(water_png)},
    {wood_png, sizeof(wood_png)},
};

typedef struct {
    GX2Texture textures[MaterialLayerCount];
    GX2Sampler sampler;
} Materials;

static bool materials_load(Materials* materials) {
    for (int material = 0; material < MaterialLayerCount; material++) {
        if (!texture_load(&materials->textures[material], material_images[material].data,
                          material_images[material].size))
            return false;
    }
    // Sharp texels like the other block renderers, the textures have no mipmaps yet
    GX2InitSampler(&materials->sampler, GX2_TEX_CLAMP_MODE_WRAP, GX2_TEX_XY_FILTER_MODE_POINT);
    return true;
}
