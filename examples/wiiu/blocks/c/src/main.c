// Generates a random 64x64x64 voxel world and renders its visible block faces on the TV and the
// GamePad.

#include <coreinit/time.h>
#include <gu.h>
#include <gx2/registers.h>
#include <gx2/shaders.h>
#include <stddef.h>
#include <stdint.h>
#include <stdlib.h>
#include <whb/gfx.h>
#include <whb/proc.h>

#include "mesh.h"
#include "scene.h"

_Alignas(64) static const uint8_t shader_gsh[] = {
#embed "../target/shader.gsh"
};

typedef struct {
    WHBGfxShaderGroup group;
    uint32_t projection_offset;
    uint32_t model_view_offset;
    Materials materials;
    Mesh mesh;
} Blocks;

static void blocks_draw(Blocks* blocks, Mtx44 projection, Mtx view) {
    WHBGfxClearColor(46 / 255.0f, 84 / 255.0f, 158 / 255.0f, 1.0f);
    GX2SetFetchShader(&blocks->group.fetchShader);
    GX2SetVertexShader(blocks->group.vertexShader);
    GX2SetPixelShader(blocks->group.pixelShader);
    GX2SetVertexUniformReg(blocks->projection_offset, 16, projection);
    GX2SetVertexUniformReg(blocks->model_view_offset, 12, view);
    mesh_draw(&blocks->mesh, &blocks->materials);
}

int main(void) {
    WHBProcInit();
    WHBGfxInit();

    static Blocks blocks;
    if (!WHBGfxLoadGFDShaderGroup(&blocks.group, 0, shader_gsh))
        return 1;
    WHBGfxInitShaderAttribute(&blocks.group, "aPosition", 0, offsetof(Vertex, position),
                              GX2_ATTRIB_FORMAT_FLOAT_32_32_32);
    WHBGfxInitShaderAttribute(&blocks.group, "aTexCoord", 0, offsetof(Vertex, tex_coord), GX2_ATTRIB_FORMAT_UNORM_8_8);
    WHBGfxInitShaderAttribute(&blocks.group, "aColor", 0, offsetof(Vertex, color), GX2_ATTRIB_FORMAT_UNORM_8_8_8_8);
    WHBGfxInitFetchShader(&blocks.group);
    blocks.projection_offset = GX2GetVertexUniformVar(blocks.group.vertexShader, "uProjection")->offset;
    blocks.model_view_offset = GX2GetVertexUniformVar(blocks.group.vertexShader, "uModelView")->offset;
    if (!materials_load(&blocks.materials))
        return 1;

    WorldInstances world = world_build((uint32_t)OSGetTime());
    if (!world.instances || !mesh_build(&blocks.mesh, &world))
        return 1;
    free(world.instances);

    // The TV renders at 1280x720 and the GamePad at 854x480, both about 16:9
    float aspect = 16.0f / 9.0f;
    Mtx44 projection;
    guPerspective(projection, 50.0f, aspect, 1.0f, 500.0f);
    OSTime started = OSGetTime();
    while (WHBProcIsRunning()) {
        double seconds = (double)OSTicksToMilliseconds(OSGetTime() - started) / 1000.0;
        BlocksCamera camera = blocks_camera(seconds, aspect);
        guVector eye = {camera.eye[0], camera.eye[1], camera.eye[2]};
        guVector target = {camera.target[0], camera.target[1], camera.target[2]};
        guVector up = {0, 1, 0};
        Mtx view;
        guLookAt(view, &eye, &up, &target);

        WHBGfxBeginRender();
        WHBGfxBeginRenderTV();
        blocks_draw(&blocks, projection, view);
        WHBGfxFinishRenderTV();
        WHBGfxBeginRenderDRC();
        blocks_draw(&blocks, projection, view);
        WHBGfxFinishRenderDRC();
        WHBGfxFinishRender();
    }

    free(blocks.mesh.vertices);
    WHBGfxFreeShaderGroup(&blocks.group);
    WHBGfxShutdown();
    WHBProcShutdown();
    return 0;
}
