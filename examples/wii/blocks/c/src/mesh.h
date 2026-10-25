#pragma once

#include <gccore.h>
#include <malloc.h>
#include <stdbool.h>
#include <stdint.h>

#include "scene.h"
#include "world.h"

typedef struct {
    void* lists[MaterialLayerCount];
    uint32_t list_sizes[MaterialLayerCount];
} Mesh;

// Face order matches faceOffsets in world.h, the corners are offsets from the voxel's minimum corner.
static const uint8_t corners[6][4][3] = {
    {{0, 0, 1}, {1, 0, 1}, {1, 1, 1}, {0, 1, 1}}, {{1, 0, 0}, {0, 0, 0}, {0, 1, 0}, {1, 1, 0}},
    {{1, 0, 1}, {1, 0, 0}, {1, 1, 0}, {1, 1, 1}}, {{0, 0, 0}, {0, 0, 1}, {0, 1, 1}, {0, 1, 0}},
    {{0, 1, 1}, {1, 1, 1}, {1, 1, 0}, {0, 1, 0}}, {{0, 0, 0}, {1, 0, 0}, {1, 0, 1}, {0, 0, 1}}};
static const uint8_t uv[4][2] = {{0, 1}, {1, 1}, {1, 0}, {0, 0}};

static void emit_face(const VoxelInstance* instance, bool water) {
    const uint8_t (*face)[3] = corners[instance->face];
    uint8_t shade = (uint8_t)((0.42f + 0.58f * blocks_face_light(instance->face)) * (float)instance->occlusion);
    for (int corner = 0; corner < 4; corner++) {
        // The world fits in 0..64, so 8-bit integer positions are exact and a quarter of the size of floats
        GX_Position3u8((uint8_t)(instance->x + face[corner][0]), (uint8_t)(instance->y + face[corner][1]),
                       (uint8_t)(instance->z + face[corner][2]));
        GX_Color4u8(shade, shade, shade, water ? 153 : 255);
        GX_TexCoord2u8(uv[corner][0], uv[corner][1]);
    }
}

static bool mesh_build(Mesh* mesh, const WorldInstances* world) {
    size_t total = world->opaqueCount + world->translucentCount;
    for (int material = 0; material < MaterialLayerCount; material++) {
        size_t count = 0;
        for (size_t i = 0; i < total; i++) {
            if (world->instances[i].material == material)
                count++;
        }
        if (count == 0)
            continue;
        size_t capacity = (count * 40 + ((count / 16000) + 1) * 64 + 159) & ~(size_t)31;
        mesh->lists[material] = memalign(32, capacity);
        if (!mesh->lists[material])
            return false;
        DCInvalidateRange(mesh->lists[material], capacity);
        GX_BeginDispList(mesh->lists[material], capacity);
        size_t emitted = 0;
        while (emitted < count) {
            size_t batch = count - emitted;
            if (batch > 16000)
                batch = 16000;
            GX_Begin(GX_QUADS, GX_VTXFMT0, batch * 4);
            size_t seen = 0;
            for (size_t i = 0; i < total && seen < emitted + batch; i++) {
                if (world->instances[i].material != material)
                    continue;
                if (seen++ < emitted)
                    continue;
                emit_face(&world->instances[i], material == MaterialLayerWater);
            }
            GX_End();
            emitted += batch;
        }
        mesh->list_sizes[material] = GX_EndDispList();
        if (mesh->list_sizes[material] == 0)
            return false;
    }
    return true;
}

static void mesh_configure_pipeline(void) {
    GX_ClearVtxDesc();
    GX_SetVtxDesc(GX_VA_POS, GX_DIRECT);
    GX_SetVtxDesc(GX_VA_CLR0, GX_DIRECT);
    GX_SetVtxDesc(GX_VA_TEX0, GX_DIRECT);
    GX_SetVtxAttrFmt(GX_VTXFMT0, GX_VA_POS, GX_POS_XYZ, GX_U8, 0);
    GX_SetVtxAttrFmt(GX_VTXFMT0, GX_VA_CLR0, GX_CLR_RGBA, GX_RGBA8, 0);
    GX_SetVtxAttrFmt(GX_VTXFMT0, GX_VA_TEX0, GX_TEX_ST, GX_U8, 0);
    GX_SetNumChans(1);
    GX_SetChanCtrl(GX_COLOR0A0, GX_DISABLE, GX_SRC_VTX, GX_SRC_VTX, 0, GX_DF_NONE, GX_AF_NONE);
    GX_SetNumTexGens(1);
    GX_SetTexCoordGen(GX_TEXCOORD0, GX_TG_MTX2x4, GX_TG_TEX0, GX_IDENTITY);
    GX_SetTevOrder(GX_TEVSTAGE0, GX_TEXCOORD0, GX_TEXMAP0, GX_COLOR0A0);
    GX_SetTevOp(GX_TEVSTAGE0, GX_MODULATE);
    GX_SetCullMode(GX_CULL_NONE);
}

static void mesh_draw(const Mesh* mesh, GXTexObj textures[MaterialLayerCount]) {
    GX_SetZMode(GX_ENABLE, GX_LEQUAL, GX_TRUE);
    GX_SetBlendMode(GX_BM_NONE, GX_BL_ONE, GX_BL_ZERO, GX_LO_CLEAR);
    for (int material = 0; material < MaterialLayerCount; material++) {
        if (material == MaterialLayerWater || !mesh->list_sizes[material])
            continue;
        GX_LoadTexObj(&textures[material], GX_TEXMAP0);
        GX_CallDispList(mesh->lists[material], mesh->list_sizes[material]);
    }
    if (mesh->list_sizes[MaterialLayerWater]) {
        GX_SetZMode(GX_ENABLE, GX_LEQUAL, GX_FALSE);
        GX_SetBlendMode(GX_BM_BLEND, GX_BL_SRCALPHA, GX_BL_INVSRCALPHA, GX_LO_CLEAR);
        GX_LoadTexObj(&textures[MaterialLayerWater], GX_TEXMAP0);
        GX_CallDispList(mesh->lists[MaterialLayerWater], mesh->list_sizes[MaterialLayerWater]);
    }
}
