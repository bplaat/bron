#pragma once

#include <d3d8.h>
#include <gdiplus.h>
#include <objbase.h>
#include <stdlib.h>
#include <string.h>
#include <windows.h>

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

// The textures of assets/, built in, in the order of MaterialLayer
static const BYTE brick_red_png[] = {
#embed "assets/brick_red.png"
};
static const BYTE cactus_side_png[] = {
#embed "assets/cactus_side.png"
};
static const BYTE cactus_top_png[] = {
#embed "assets/cactus_top.png"
};
static const BYTE dirt_png[] = {
#embed "assets/dirt.png"
};
static const BYTE dirt_grass_png[] = {
#embed "assets/dirt_grass.png"
};
static const BYTE grass_top_png[] = {
#embed "assets/grass_top.png"
};
static const BYTE greystone_png[] = {
#embed "assets/greystone.png"
};
static const BYTE lava_png[] = {
#embed "assets/lava.png"
};
static const BYTE leaves_png[] = {
#embed "assets/leaves.png"
};
static const BYTE sand_png[] = {
#embed "assets/sand.png"
};
static const BYTE stone_png[] = {
#embed "assets/stone.png"
};
static const BYTE stone_coal_png[] = {
#embed "assets/stone_coal.png"
};
static const BYTE stone_diamond_png[] = {
#embed "assets/stone_diamond.png"
};
static const BYTE stone_gold_png[] = {
#embed "assets/stone_gold.png"
};
static const BYTE stone_iron_png[] = {
#embed "assets/stone_iron.png"
};
static const BYTE trunk_side_png[] = {
#embed "assets/trunk_side.png"
};
static const BYTE trunk_top_png[] = {
#embed "assets/trunk_top.png"
};
static const BYTE water_png[] = {
#embed "assets/water.png"
};
static const BYTE wood_png[] = {
#embed "assets/wood.png"
};

static const struct {
    const BYTE* data;
    UINT size;
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
    IDirect3DTexture8* textures[MaterialLayerCount];
} Materials;

// Decodes an image with GDI+ into a texture with a full mipmap chain, each level averages the one before
static IDirect3DTexture8* material_load(IDirect3DDevice8* device, const BYTE* image, UINT size) {
    // GDI+ decodes from a stream, over a copy of the image
    IStream* stream = NULL;
    GpBitmap* bitmap;
    HGLOBAL memory = GlobalAlloc(GMEM_MOVEABLE, size);
    if (!memory)
        return NULL;
    memcpy(GlobalLock(memory), image, size);
    GlobalUnlock(memory);
    if (FAILED(CreateStreamOnHGlobal(memory, TRUE, &stream))) {
        GlobalFree(memory);
        return NULL;
    }
    if (GdipCreateBitmapFromStream(stream, &bitmap) != Ok) {
        IUnknown_Release((IUnknown*)stream);
        return NULL;
    }
    UINT width, height;
    GdipGetImageWidth((GpImage*)bitmap, &width);
    GdipGetImageHeight((GpImage*)bitmap, &height);
    GpRect rect = {0, 0, (INT)width, (INT)height};
    BitmapData data;
    IDirect3DTexture8* texture = NULL;
    UINT32* pixels = malloc(width * height * 4);
    if (pixels && GdipBitmapLockBits(bitmap, &rect, ImageLockModeRead, PixelFormat32bppARGB, &data) == Ok) {
        for (UINT y = 0; y < height; y++)
            memcpy(pixels + y * width, (BYTE*)data.Scan0 + y * data.Stride, width * 4);
        GdipBitmapUnlockBits(bitmap, &data);
        if (FAILED(IDirect3DDevice8_CreateTexture(device, width, height, 0, 0, D3DFMT_A8R8G8B8, D3DPOOL_MANAGED,
                                                  &texture)))
            texture = NULL;
    }
    for (DWORD level = 0; texture && level < IDirect3DTexture8_GetLevelCount(texture); level++) {
        D3DLOCKED_RECT locked;
        if (level > 0) {
            UINT next_width = width > 1 ? width / 2 : 1, next_height = height > 1 ? height / 2 : 1;
            for (UINT y = 0; y < next_height; y++)
                for (UINT x = 0; x < next_width; x++) {
                    UINT x1 = x * 2 + (width > 1), y1 = y * 2 + (height > 1);
                    const UINT32 corners[4] = {pixels[y * 2 * width + x * 2], pixels[y * 2 * width + x1],
                                               pixels[y1 * width + x * 2], pixels[y1 * width + x1]};
                    UINT32 average = 0;
                    for (int shift = 0; shift < 32; shift += 8) {
                        UINT32 sum = 0;
                        for (int i = 0; i < 4; i++)
                            sum += (corners[i] >> shift) & 0xff;
                        average |= ((sum + 2) / 4) << shift;
                    }
                    pixels[y * next_width + x] = average;
                }
            width = next_width;
            height = next_height;
        }
        if (SUCCEEDED(IDirect3DTexture8_LockRect(texture, level, &locked, NULL, 0))) {
            for (UINT y = 0; y < height; y++)
                memcpy((BYTE*)locked.pBits + y * locked.Pitch, pixels + y * width, width * 4);
            IDirect3DTexture8_UnlockRect(texture, level);
        }
    }
    free(pixels);
    GdipDisposeImage((GpImage*)bitmap);
    IUnknown_Release((IUnknown*)stream);
    return texture;
}

static BOOL materials_load(Materials* materials, IDirect3DDevice8* device) {
    for (int material = 0; material < MaterialLayerCount; material++) {
        materials->textures[material] =
            material_load(device, material_images[material].data, material_images[material].size);
        if (!materials->textures[material])
            return FALSE;
    }
    return TRUE;
}

static void materials_destroy(Materials* materials) {
    for (int material = 0; material < MaterialLayerCount; material++)
        if (materials->textures[material])
            IDirect3DTexture8_Release(materials->textures[material]);
}
