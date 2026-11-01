#pragma once

#include <gx2/draw.h>
#include <gx2/mem.h>
#include <gx2/registers.h>
#include <gx2/shaders.h>
#include <malloc.h>
#include <stdbool.h>
#include <stdint.h>

#include "materials.h"
#include "scene.h"
#include "world.h"

typedef struct {
    float position[3];
    uint8_t tex_coord[2];
    uint8_t color[4];
    uint8_t padding[2];
} Vertex;

// The vertices of all faces, sorted by material, so each material draws one range
typedef struct {
    Vertex* vertices;
    uint32_t vertex_count;
    uint32_t first[MaterialLayerCount];
    uint32_t count[MaterialLayerCount];
} Mesh;

// Face order matches faceOffsets in world.h, the corners are offsets from the voxel's minimum corner.
static const uint8_t corners[6][4][3] = {
    {{0, 0, 1}, {1, 0, 1}, {1, 1, 1}, {0, 1, 1}}, {{1, 0, 0}, {0, 0, 0}, {0, 1, 0}, {1, 1, 0}},
    {{1, 0, 1}, {1, 0, 0}, {1, 1, 0}, {1, 1, 1}}, {{0, 0, 0}, {0, 0, 1}, {0, 1, 1}, {0, 1, 0}},
    {{0, 1, 1}, {1, 1, 1}, {1, 1, 0}, {0, 1, 0}}, {{0, 0, 0}, {1, 0, 0}, {1, 0, 1}, {0, 0, 1}}};
// The texture coordinates are normalized bytes, 255 is 1.0
static const uint8_t uv[4][2] = {{0, 255}, {255, 255}, {255, 0}, {0, 0}};

static void emit_face(Vertex* vertices, const VoxelInstance* instance, bool water) {
    const uint8_t (*face)[3] = corners[instance->face];
    uint8_t shade = (uint8_t)((0.42f + 0.58f * blocks_face_light(instance->face)) * (float)instance->occlusion);
    for (int corner = 0; corner < 4; corner++) {
        vertices[corner] = (Vertex){
            .position = {(float)(instance->x + face[corner][0]), (float)(instance->y + face[corner][1]),
                         (float)(instance->z + face[corner][2])},
            .tex_coord = {uv[corner][0], uv[corner][1]},
            .color = {shade, shade, shade, water ? 153 : 255},
        };
    }
}

static bool mesh_build(Mesh* mesh, const WorldInstances* world) {
    size_t total = world->opaqueCount + world->translucentCount;
    for (size_t i = 0; i < total; i++)
        mesh->count[world->instances[i].material] += 4;
    for (int material = 0; material < MaterialLayerCount; material++) {
        mesh->first[material] = mesh->vertex_count;
        mesh->vertex_count += mesh->count[material];
    }

    // The GPU reads the vertices straight from memory, which needs 64 byte alignment
    mesh->vertices = memalign(GX2_VERTEX_BUFFER_ALIGNMENT, mesh->vertex_count * sizeof(Vertex));
    if (!mesh->vertices)
        return false;
    uint32_t next[MaterialLayerCount];
    for (int material = 0; material < MaterialLayerCount; material++)
        next[material] = mesh->first[material];
    for (size_t i = 0; i < total; i++) {
        const VoxelInstance* instance = &world->instances[i];
        emit_face(&mesh->vertices[next[instance->material]], instance, instance->material == MaterialLayerWater);
        next[instance->material] += 4;
    }
    GX2Invalidate(GX2_INVALIDATE_MODE_CPU_ATTRIBUTE_BUFFER, mesh->vertices, mesh->vertex_count * sizeof(Vertex));
    return true;
}

static void mesh_draw(const Mesh* mesh, Materials* materials) {
    GX2SetAttribBuffer(0, mesh->vertex_count * sizeof(Vertex), sizeof(Vertex), mesh->vertices);
    GX2SetPixelSampler(&materials->sampler, 0);

    // Faces wind counter-clockwise seen from outside, so culling the back faces skips the faces
    // seen from behind
    GX2SetCullOnlyControl(GX2_FRONT_FACE_CCW, FALSE, TRUE);
    GX2SetDepthOnlyControl(TRUE, TRUE, GX2_COMPARE_FUNC_LEQUAL);
    GX2SetColorControl(GX2_LOGIC_OP_COPY, 0, FALSE, TRUE);
    for (int material = 0; material < MaterialLayerCount; material++) {
        if (material == MaterialLayerWater || !mesh->count[material])
            continue;
        GX2SetPixelTexture(&materials->textures[material], 0);
        GX2DrawEx(GX2_PRIMITIVE_MODE_QUADS, mesh->count[material], mesh->first[material], 1);
    }

    // The translucent water goes last, blended over the terrain without writing depth
    if (mesh->count[MaterialLayerWater]) {
        GX2SetCullOnlyControl(GX2_FRONT_FACE_CCW, FALSE, FALSE);
        GX2SetDepthOnlyControl(TRUE, FALSE, GX2_COMPARE_FUNC_LEQUAL);
        GX2SetColorControl(GX2_LOGIC_OP_COPY, 1, FALSE, TRUE);
        GX2SetBlendControl(GX2_RENDER_TARGET_0, GX2_BLEND_MODE_SRC_ALPHA, GX2_BLEND_MODE_INV_SRC_ALPHA,
                           GX2_BLEND_COMBINE_MODE_ADD, TRUE, GX2_BLEND_MODE_ONE, GX2_BLEND_MODE_INV_SRC_ALPHA,
                           GX2_BLEND_COMBINE_MODE_ADD);
        GX2SetPixelTexture(&materials->textures[MaterialLayerWater], 0);
        GX2DrawEx(GX2_PRIMITIVE_MODE_QUADS, mesh->count[MaterialLayerWater], mesh->first[MaterialLayerWater], 1);
    }
}
