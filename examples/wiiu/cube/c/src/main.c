// The rotating textured cube scene from the WebGPU example, on the TV and the GamePad.

#include <coreinit/time.h>
#include <gu.h>
#include <gx2/draw.h>
#include <gx2/mem.h>
#include <gx2/registers.h>
#include <gx2/sampler.h>
#include <gx2/shaders.h>
#include <math.h>
#include <stdint.h>
#include <whb/gfx.h>
#include <whb/proc.h>

#include "texture.h"

typedef struct {
    float position[3];
    float tex_coord[2];
} Vertex;

// Four corners for each face, so every face gets the whole texture
_Alignas(GX2_VERTEX_BUFFER_ALIGNMENT) static Vertex vertices[24] = {
    {{-1, -1, 1}, {0, 1}},  {{1, -1, 1}, {1, 1}},  {{1, 1, 1}, {1, 0}},    {{-1, 1, 1}, {0, 0}},  {{1, -1, -1}, {0, 1}},
    {{-1, -1, -1}, {1, 1}}, {{-1, 1, -1}, {1, 0}}, {{1, 1, -1}, {0, 0}},   {{1, -1, 1}, {0, 1}},  {{1, -1, -1}, {1, 1}},
    {{1, 1, -1}, {1, 0}},   {{1, 1, 1}, {0, 0}},   {{-1, -1, -1}, {0, 1}}, {{-1, -1, 1}, {1, 1}}, {{-1, 1, 1}, {1, 0}},
    {{-1, 1, -1}, {0, 0}},  {{-1, 1, 1}, {0, 1}},  {{1, 1, 1}, {1, 1}},    {{1, 1, -1}, {1, 0}},  {{-1, 1, -1}, {0, 0}},
    {{-1, -1, -1}, {0, 1}}, {{1, -1, -1}, {1, 1}}, {{1, -1, 1}, {1, 0}},   {{-1, -1, 1}, {0, 0}},
};
static const float normals[6][3] = {{0, 0, 1}, {0, 0, -1}, {1, 0, 0}, {-1, 0, 0}, {0, 1, 0}, {0, -1, 0}};

_Alignas(64) static const uint8_t shader_gsh[] = {
#embed "../target/shader.gsh"
};

static const uint8_t crate_jpg[] = {
#embed "assets/crate.jpg"
};

typedef struct {
    WHBGfxShaderGroup group;
    uint32_t projection_offset;
    uint32_t model_view_offset;
    uint32_t light_offset;
    GX2Texture texture;
    GX2Sampler sampler;
} Cube;

// The same model and camera rotations as the WebGPU cube shader
static void cube_matrix(Mtx matrix, float elapsed) {
    Mtx temp;
    ps_guMtxRotRad(matrix, 'x', elapsed * 0.55f);
    ps_guMtxRotRad(temp, 'z', elapsed * 0.35f);
    ps_guMtxConcat(temp, matrix, matrix);
    ps_guMtxRotRad(temp, 'y', 0.4f);
    ps_guMtxConcat(temp, matrix, matrix);
    ps_guMtxRotRad(temp, 'x', 0.25f);
    ps_guMtxConcat(temp, matrix, matrix);
}

static void cube_draw(Cube* cube, Mtx44 projection, Mtx matrix) {
    WHBGfxClearColor(0.0f, 0.0f, 0.0f, 1.0f);
    GX2SetFetchShader(&cube->group.fetchShader);
    GX2SetVertexShader(cube->group.vertexShader);
    GX2SetPixelShader(cube->group.pixelShader);
    GX2SetDepthOnlyControl(TRUE, TRUE, GX2_COMPARE_FUNC_LEQUAL);
    GX2SetVertexUniformReg(cube->projection_offset, 16, projection);
    GX2SetVertexUniformReg(cube->model_view_offset, 12, matrix);
    GX2SetPixelTexture(&cube->texture, 0);
    GX2SetPixelSampler(&cube->sampler, 0);
    GX2SetAttribBuffer(0, sizeof(vertices), sizeof(Vertex), vertices);
    for (int face = 0; face < 6; face++) {
        // Like the WebGPU shader: the rotated normal against a fixed light, lit from both sides
        float dot = 0;
        const float light[3] = {0.4f, 0.8f, 0.6f};
        for (int row = 0; row < 3; row++)
            for (int column = 0; column < 3; column++)
                dot += matrix[row][column] * normals[face][column] * light[row];
        float shade = 0.35f + 0.65f * fabsf(dot) / sqrtf(1.16f);
        float color[4] = {shade, shade, shade, 1.0f};
        GX2SetPixelUniformReg(cube->light_offset, 4, color);
        GX2DrawEx(GX2_PRIMITIVE_MODE_QUADS, 4, (uint32_t)face * 4, 1);
    }
}

int main(void) {
    WHBProcInit();
    WHBGfxInit();

    Cube cube = {0};
    if (!WHBGfxLoadGFDShaderGroup(&cube.group, 0, shader_gsh))
        return 1;
    WHBGfxInitShaderAttribute(&cube.group, "aPosition", 0, 0, GX2_ATTRIB_FORMAT_FLOAT_32_32_32);
    WHBGfxInitShaderAttribute(&cube.group, "aTexCoord", 0, 12, GX2_ATTRIB_FORMAT_FLOAT_32_32);
    WHBGfxInitFetchShader(&cube.group);
    cube.projection_offset = GX2GetVertexUniformVar(cube.group.vertexShader, "uProjection")->offset;
    cube.model_view_offset = GX2GetVertexUniformVar(cube.group.vertexShader, "uModelView")->offset;
    cube.light_offset = GX2GetPixelUniformVar(cube.group.pixelShader, "uLight")->offset;
    GX2Invalidate(GX2_INVALIDATE_MODE_CPU_ATTRIBUTE_BUFFER, vertices, sizeof(vertices));
    if (!texture_load(&cube.texture, crate_jpg, sizeof(crate_jpg)))
        return 1;
    GX2InitSampler(&cube.sampler, GX2_TEX_CLAMP_MODE_WRAP, GX2_TEX_XY_FILTER_MODE_LINEAR);

    // The TV renders at 1280x720 and the GamePad at 854x480, both about 16:9
    Mtx44 projection;
    guPerspective(projection, 45.0f, 16.0f / 9.0f, 0.1f, 100.0f);
    OSTime started = OSGetTime();
    while (WHBProcIsRunning()) {
        Mtx matrix;
        cube_matrix(matrix, (float)OSTicksToMilliseconds(OSGetTime() - started) / 1000.0f);
        matrix[2][3] = -4.5f;

        WHBGfxBeginRender();
        WHBGfxBeginRenderTV();
        cube_draw(&cube, projection, matrix);
        WHBGfxFinishRenderTV();
        WHBGfxBeginRenderDRC();
        cube_draw(&cube, projection, matrix);
        WHBGfxFinishRenderDRC();
        WHBGfxFinishRender();
    }

    texture_free(&cube.texture);
    WHBGfxFreeShaderGroup(&cube.group);
    WHBGfxShutdown();
    WHBProcShutdown();
    return 0;
}
