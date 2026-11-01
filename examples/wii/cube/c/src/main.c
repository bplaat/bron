// The rotating textured cube scene from the WebGPU example.

#include <gccore.h>
#include <malloc.h>
#include <math.h>
#include <ogc/lwp_watchdog.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdlib.h>
#include <string.h>
#include <wiiuse/wpad.h>

#define FIFO_SIZE (256 * 1024)
#define CLEAR_COLOR ((GXColor){0, 0, 0, 255})

// The power button of the Wii and of the Wii Remotes, libogc calls it from an interrupt
static volatile bool power_pressed = false;

static void power(void) {
    power_pressed = true;
}

static bool home_pressed(void) {
    for (int channel = 0; channel < WPAD_MAX_WIIMOTES; channel++) {
        if (WPAD_ButtonsDown(channel) & WPAD_BUTTON_HOME)
            return true;
    }
    return false;
}

// libogc relocates the TPL headers in place.
_Alignas(32) static uint8_t texture_tpl[] = {
#embed "../target/cube.tpl"
};

// The same model and camera rotations as the WebGPU cube shader.
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

static const float corners[6][4][3] = {
    {{-1, -1, 1}, {1, -1, 1}, {1, 1, 1}, {-1, 1, 1}}, {{1, -1, -1}, {-1, -1, -1}, {-1, 1, -1}, {1, 1, -1}},
    {{1, -1, 1}, {1, -1, -1}, {1, 1, -1}, {1, 1, 1}}, {{-1, -1, -1}, {-1, -1, 1}, {-1, 1, 1}, {-1, 1, -1}},
    {{-1, 1, 1}, {1, 1, 1}, {1, 1, -1}, {-1, 1, -1}}, {{-1, -1, -1}, {1, -1, -1}, {1, -1, 1}, {-1, -1, 1}}};
static const float normals[6][3] = {{0, 0, 1}, {0, 0, -1}, {1, 0, 0}, {-1, 0, 0}, {0, 1, 0}, {0, -1, 0}};

static void draw_cube(float elapsed) {
    Mtx matrix;
    cube_matrix(matrix, elapsed);
    matrix[2][3] = -4.5f;
    GX_LoadPosMtxImm(matrix, GX_PNMTX0);
    GX_Begin(GX_QUADS, GX_VTXFMT0, 24);
    for (int face = 0; face < 6; face++) {
        float dot = 0;
        const float light[3] = {0.4f, 0.8f, 0.6f};
        for (int row = 0; row < 3; row++)
            for (int col = 0; col < 3; col++)
                dot += matrix[row][col] * normals[face][col] * light[row];
        uint8_t shade = (0.35f + 0.65f * fabsf(dot) / sqrtf(1.16f)) * 255;
        for (int vertex = 0; vertex < 4; vertex++) {
            GX_Position3f32(corners[face][vertex][0], corners[face][vertex][1], corners[face][vertex][2]);
            GX_Color4u8(shade, shade, shade, 255);
            GX_TexCoord2f32(vertex == 1 || vertex == 2, vertex < 2);
        }
    }
    GX_End();
}

int main(void) {
    TPLFile tpl;
    GXTexObj texture;
    if (TPL_OpenTPLFromMemory(&tpl, texture_tpl, sizeof(texture_tpl)) != 1 || TPL_GetTexture(&tpl, 0, &texture) != 0)
        return 1;

    VIDEO_Init();
    VIDEO_SetBlack(true);
    GXRModeObj* screenmode = VIDEO_GetPreferredMode(NULL);
    if (!screenmode)
        return 1;

    void* framebuffer0 = SYS_AllocateFramebuffer(screenmode);
    void* framebuffer1 = SYS_AllocateFramebuffer(screenmode);
    void* fifo = memalign(32, FIFO_SIZE);
    if (!framebuffer0 || !framebuffer1 || !fifo) {
        free(framebuffer0);
        free(framebuffer1);
        free(fifo);
        return 1;
    }
    void* framebuffers[2] = {MEM_K0_TO_K1(framebuffer0), MEM_K0_TO_K1(framebuffer1)};
    uint32_t framebuffer = 0;
    bool first_frame = true;
    float aspect = CONF_GetAspectRatio() == CONF_ASPECT_16_9 ? 16.0f / 9.0f : 4.0f / 3.0f;

    VIDEO_Configure(screenmode);
    VIDEO_SetNextFramebuffer(framebuffers[framebuffer]);
    VIDEO_Flush();
    VIDEO_WaitVSync();
    if (screenmode->viTVMode & VI_NON_INTERLACE)
        VIDEO_WaitVSync();

    memset(fifo, 0, FIFO_SIZE);
    GX_Init(fifo, FIFO_SIZE);
    GX_SetViewport(0, 0, screenmode->fbWidth, screenmode->efbHeight, 0, 1);
    GX_SetScissor(0, 0, screenmode->fbWidth, screenmode->efbHeight);
    float yscale = GX_GetYScaleFactor(screenmode->efbHeight, screenmode->xfbHeight);
    uint32_t xfb_height = GX_SetDispCopyYScale(yscale);
    GX_SetDispCopySrc(0, 0, screenmode->fbWidth, screenmode->efbHeight);
    GX_SetDispCopyDst(screenmode->fbWidth, xfb_height);
    GX_SetCopyFilter(screenmode->aa, screenmode->sample_pattern, GX_TRUE, screenmode->vfilter);
    GX_SetFieldMode(screenmode->field_rendering,
                    screenmode->viHeight == 2 * screenmode->xfbHeight ? GX_ENABLE : GX_DISABLE);
    GX_SetPixelFmt(screenmode->aa ? GX_PF_RGB565_Z16 : GX_PF_RGB8_Z24, GX_ZC_LINEAR);
    GX_SetCopyClear(CLEAR_COLOR, GX_MAX_Z24);
    GX_CopyDisp(framebuffers[framebuffer], GX_TRUE);

    WPAD_Init();
    SYS_SetPowerCallback(power);

    GX_ClearVtxDesc();
    GX_SetVtxDesc(GX_VA_POS, GX_DIRECT);
    GX_SetVtxDesc(GX_VA_CLR0, GX_DIRECT);
    GX_SetVtxAttrFmt(GX_VTXFMT0, GX_VA_POS, GX_POS_XYZ, GX_F32, 0);
    GX_SetVtxAttrFmt(GX_VTXFMT0, GX_VA_CLR0, GX_CLR_RGBA, GX_RGBA8, 0);
    GX_SetNumChans(1);
    GX_SetChanCtrl(GX_COLOR0A0, GX_DISABLE, GX_SRC_VTX, GX_SRC_VTX, 0, GX_DF_NONE, GX_AF_NONE);
    GX_SetVtxDesc(GX_VA_TEX0, GX_DIRECT);
    GX_SetVtxAttrFmt(GX_VTXFMT0, GX_VA_TEX0, GX_TEX_ST, GX_F32, 0);
    GX_SetNumTexGens(1);
    GX_SetTexCoordGen(GX_TEXCOORD0, GX_TG_MTX2x4, GX_TG_TEX0, GX_IDENTITY);
    GX_InitTexObjFilterMode(&texture, GX_LINEAR, GX_LINEAR);
    GX_LoadTexObj(&texture, GX_TEXMAP0);
    GX_SetZMode(GX_TRUE, GX_LEQUAL, GX_TRUE);
    GX_SetTevOrder(GX_TEVSTAGE0, GX_TEXCOORD0, GX_TEXMAP0, GX_COLOR0A0);
    GX_SetTevOp(GX_TEVSTAGE0, GX_MODULATE);
    GX_SetCullMode(GX_CULL_NONE);

    Mtx44 projection;
    guPerspective(projection, 45.0f, aspect, 0.1f, 100.0f);
    GX_LoadProjectionMtx(projection, GX_PERSPECTIVE);

    uint64_t started = gettime();
    while (!power_pressed) {
        WPAD_ScanPads();
        if (home_pressed())
            break;
        draw_cube((float)(gettime() - started) / 60750000.0f);
        framebuffer ^= 1;
        GX_CopyDisp(framebuffers[framebuffer], GX_TRUE);
        // The copy follows the drawing, so this waits for both before the frame is shown.
        GX_DrawDone();
        VIDEO_SetNextFramebuffer(framebuffers[framebuffer]);
        if (first_frame) {
            VIDEO_SetBlack(false);
            first_frame = false;
        }
        VIDEO_Flush();
        VIDEO_WaitVSync();
    }

    // HOME returns to the loader, a power button turns the Wii off
    if (power_pressed)
        SYS_ResetSystem(SYS_POWEROFF, 0, 0);
    free(framebuffer0);
    free(framebuffer1);
    free(fifo);
    return 0;
}
