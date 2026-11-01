// A minimal program that draws a rotating triangle on the TV and the GamePad.

#include <coreinit/time.h>
#include <gu.h>
#include <gx2/draw.h>
#include <gx2/mem.h>
#include <gx2/registers.h>
#include <gx2/shaders.h>
#include <stdint.h>
#include <whb/gfx.h>
#include <whb/proc.h>

typedef struct {
    float position[3];
    float color[4];
} Vertex;

// The GPU reads the vertices straight from memory, which needs 64 byte alignment
_Alignas(GX2_VERTEX_BUFFER_ALIGNMENT) static Vertex vertices[3] = {
    {{0.0f, 1.0f, 0.0f}, {1.0f, 0.0f, 0.0f, 1.0f}},
    {{-1.0f, -1.0f, 0.0f}, {0.0f, 1.0f, 0.0f, 1.0f}},
    {{1.0f, -1.0f, 0.0f}, {0.0f, 0.0f, 1.0f, 1.0f}},
};

_Alignas(64) static const uint8_t shader_gsh[] = {
#embed "../target/shader.gsh"
};

static void draw(WHBGfxShaderGroup* group, Mtx44 projection, Mtx matrix) {
    WHBGfxClearColor(0.5f, 0.5f, 0.5f, 1.0f);
    GX2SetFetchShader(&group->fetchShader);
    GX2SetVertexShader(group->vertexShader);
    GX2SetPixelShader(group->pixelShader);
    GX2SetVertexUniformReg(GX2GetVertexUniformVar(group->vertexShader, "uProjection")->offset, 16, projection);
    GX2SetVertexUniformReg(GX2GetVertexUniformVar(group->vertexShader, "uModelView")->offset, 12, matrix);
    GX2SetAttribBuffer(0, sizeof(vertices), sizeof(Vertex), vertices);
    GX2DrawEx(GX2_PRIMITIVE_MODE_TRIANGLES, 3, 0, 1);
}

int main(void) {
    WHBProcInit();
    WHBGfxInit();

    WHBGfxShaderGroup group = {0};
    if (!WHBGfxLoadGFDShaderGroup(&group, 0, shader_gsh))
        return 1;
    WHBGfxInitShaderAttribute(&group, "aPosition", 0, 0, GX2_ATTRIB_FORMAT_FLOAT_32_32_32);
    WHBGfxInitShaderAttribute(&group, "aColor", 0, 12, GX2_ATTRIB_FORMAT_FLOAT_32_32_32_32);
    WHBGfxInitFetchShader(&group);
    GX2Invalidate(GX2_INVALIDATE_MODE_CPU_ATTRIBUTE_BUFFER, vertices, sizeof(vertices));

    // The TV renders at 1280x720 and the GamePad at 854x480, both about 16:9
    Mtx44 projection;
    guPerspective(projection, 45.0f, 16.0f / 9.0f, 0.1f, 100.0f);
    OSTime started = OSGetTime();
    while (WHBProcIsRunning()) {
        float seconds = (float)OSTicksToMilliseconds(OSGetTime() - started) / 1000.0f;
        Mtx matrix;
        ps_guMtxRotRad(matrix, 'y', seconds);
        ps_guMtxTransApply(matrix, matrix, 0, 0, -3);

        WHBGfxBeginRender();
        WHBGfxBeginRenderTV();
        draw(&group, projection, matrix);
        WHBGfxFinishRenderTV();
        WHBGfxBeginRenderDRC();
        draw(&group, projection, matrix);
        WHBGfxFinishRenderDRC();
        WHBGfxFinishRender();
    }

    WHBGfxFreeShaderGroup(&group);
    WHBGfxShutdown();
    WHBProcShutdown();
    return 0;
}
