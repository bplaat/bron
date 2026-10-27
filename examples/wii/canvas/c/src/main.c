// 2D rendering with PNG textures, bitmap font text and Wii Remote cursors.

#include <gccore.h>
#include <malloc.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <wiiuse/wpad.h>

#include "canvas.h"
#include "cursor.h"

#define FIFO_SIZE (256 * 1024)
#define CLEAR_COLOR ((GXColor){128, 128, 128, 255})

static const uint8_t dirt_grass_png[] = {
#embed "assets/dirt_grass.png"
};
static const uint8_t stone_coal_png[] = {
#embed "assets/stone_coal.png"
};

// The power button of the Wii and of the Wii Remotes, libogc calls it from an interrupt
static volatile bool power_pressed = false;

static void power(void) {
    power_pressed = true;
}

int main(void) {
    VIDEO_Init();
    VIDEO_SetBlack(true);
    GXRModeObj* screenmode = VIDEO_GetPreferredMode(NULL);

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
    uint32_t screen_height = screenmode->viHeight;
    uint32_t screen_width = (uint32_t)(screen_height * aspect);

    WPAD_Init();
    SYS_SetPowerCallback(power);
    WPAD_SetDataFormat(WPAD_CHAN_ALL, WPAD_FMT_BTNS_ACC_IR);
    WPAD_SetVRes(0, screen_width, screen_height);

    // Init stuff
    if (!canvas_init() || !cursor_init())
        return 1;

    GXTexObj dirt_grass_texture;
    if (!texture_load_png_rgba8(&dirt_grass_texture, dirt_grass_png, sizeof(dirt_grass_png)))
        return 1;
    GXTexObj stone_coal_texture;
    if (!texture_load_png_rgba8(&stone_coal_texture, stone_coal_png, sizeof(stone_coal_png)))
        return 1;

    // Game state
    float rotation = 0;
    Mtx44 perspective_matrix;
    guPerspective(perspective_matrix, 45.0f, aspect, 0.1f, 1000.0f);

    // Game loop
    while (!power_pressed) {
        // Update
        rotation += 1;

        // Read buttons
        if (cursor_update() & WPAD_BUTTON_HOME)
            break;

        // ### Draw cube ###
        {
            // Set projection matrix
            GX_LoadProjectionMtx(perspective_matrix, GX_PERSPECTIVE);

            // Enable depth test and disable culling
            GX_SetZMode(GX_ENABLE, GX_LEQUAL, GX_TRUE);
            GX_SetCullMode(GX_CULL_NONE);

            // Set vertex pipeline
            GX_ClearVtxDesc();
            GX_SetVtxDesc(GX_VA_POS, GX_DIRECT);
            GX_SetVtxDesc(GX_VA_TEX0, GX_DIRECT);
            GX_SetVtxAttrFmt(GX_VTXFMT0, GX_VA_POS, GX_POS_XYZ, GX_F32, 0);
            GX_SetVtxAttrFmt(GX_VTXFMT0, GX_VA_TEX0, GX_TEX_ST, GX_F32, 0);
            GX_SetNumChans(1);
            GX_SetNumTexGens(1);
            GX_SetTexCoordGen(GX_TEXCOORD0, GX_TG_MTX2x4, GX_TG_TEX0, GX_IDENTITY);
            GX_SetTevOrder(GX_TEVSTAGE0, GX_TEXCOORD0, GX_TEXMAP0, GX_COLOR0A0);
            GX_SetTevOp(GX_TEVSTAGE0, GX_REPLACE);

            // Load texture
            GX_LoadTexObj(&stone_coal_texture, GX_TEXMAP0);

            // Set cube matrix
            Mtx cube_matrix;
            ps_guMtxRotRad(cube_matrix, 'x', DegToRad(rotation));
            Mtx temp_matrix;
            ps_guMtxRotRad(temp_matrix, 'y', DegToRad(rotation));
            ps_guMtxConcat(cube_matrix, temp_matrix, cube_matrix);
            ps_guMtxTransApply(cube_matrix, cube_matrix, 0, 0, -8);
            GX_LoadPosMtxImm(cube_matrix, GX_PNMTX0);

            // Draw cube
            GX_Begin(GX_QUADS, GX_VTXFMT0, 24);
            GX_Position3f32(-1.0f, 1.0f, -1.0f);
            GX_TexCoord2f32(0.0f, 0.0f);
            GX_Position3f32(-1.0f, 1.0f, 1.0f);
            GX_TexCoord2f32(1.0f, 0.0f);
            GX_Position3f32(-1.0f, -1.0f, 1.0f);
            GX_TexCoord2f32(1.0f, 1.0f);
            GX_Position3f32(-1.0f, -1.0f, -1.0f);
            GX_TexCoord2f32(0.0f, 1.0f);

            GX_Position3f32(1.0f, 1.0f, -1.0f);
            GX_TexCoord2f32(0.0f, 0.0f);
            GX_Position3f32(1.0f, -1.0f, -1.0f);
            GX_TexCoord2f32(1.0f, 0.0f);
            GX_Position3f32(1.0f, -1.0f, 1.0f);
            GX_TexCoord2f32(1.0f, 1.0f);
            GX_Position3f32(1.0f, 1.0f, 1.0f);
            GX_TexCoord2f32(0.0f, 1.0f);

            GX_Position3f32(-1.0f, -1.0f, 1.0f);
            GX_TexCoord2f32(0.0f, 0.0f);
            GX_Position3f32(1.0f, -1.0f, 1.0f);
            GX_TexCoord2f32(1.0f, 0.0f);
            GX_Position3f32(1.0f, -1.0f, -1.0f);
            GX_TexCoord2f32(1.0f, 1.0f);
            GX_Position3f32(-1.0f, -1.0f, -1.0f);
            GX_TexCoord2f32(0.0f, 1.0f);

            GX_Position3f32(-1.0f, 1.0f, 1.0f);
            GX_TexCoord2f32(0.0f, 0.0f);
            GX_Position3f32(-1.0f, 1.0f, -1.0f);
            GX_TexCoord2f32(1.0f, 0.0f);
            GX_Position3f32(1.0f, 1.0f, -1.0f);
            GX_TexCoord2f32(1.0f, 1.0f);
            GX_Position3f32(1.0f, 1.0f, 1.0f);
            GX_TexCoord2f32(0.0f, 1.0f);

            GX_Position3f32(1.0f, -1.0f, -1.0f);
            GX_TexCoord2f32(0.0f, 0.0f);
            GX_Position3f32(1.0f, 1.0f, -1.0f);
            GX_TexCoord2f32(1.0f, 0.0f);
            GX_Position3f32(-1.0f, 1.0f, -1.0f);
            GX_TexCoord2f32(1.0f, 1.0f);
            GX_Position3f32(-1.0f, -1.0f, -1.0f);
            GX_TexCoord2f32(0.0f, 1.0f);

            GX_Position3f32(1.0f, -1.0f, 1.0f);
            GX_TexCoord2f32(0.0f, 0.0f);
            GX_Position3f32(-1.0f, -1.0f, 1.0f);
            GX_TexCoord2f32(1.0f, 0.0f);
            GX_Position3f32(-1.0f, 1.0f, 1.0f);
            GX_TexCoord2f32(1.0f, 1.0f);
            GX_Position3f32(1.0f, 1.0f, 1.0f);
            GX_TexCoord2f32(0.0f, 1.0f);
            GX_End();
        }

        // ### Draw HUD ###
        canvas_begin(screen_width, screen_height);

        ps_guMtxRotRad(canvas.transform_matrix, 'z', DegToRad(rotation));
        canvas_draw_image(&dirt_grass_texture, 50, 100, 100, 100, 0xffffffff);
        canvas_draw_image(&dirt_grass_texture, 100, 150, 100, 100, 0xff0000ff);
        canvas_draw_image(&dirt_grass_texture, 150, 200, 100, 100, 0x00ff00ff);
        canvas_draw_image(&dirt_grass_texture, 200, 250, 100, 100, 0x0000ffff);
        ps_guMtxIdentity(canvas.transform_matrix);

        float y = 8;
        canvas_fill_text("Hello Wii 🏠!", 8, y, 64, 0xffffffff);
        y += 64 + 8;
        canvas_fill_text("The quick brown fox jumps over the lazy dog.", 8, y, 24, 0xff0000ff);
        y += 24 + 8;

        char debug_string[255];
        sprintf(debug_string, "framebuffer=%dx%d viewport=%dx%d", screenmode->fbWidth, screenmode->xfbHeight,
                screen_width, screen_height);
        canvas_fill_text(debug_string, 8, y, 24, 0xffffffff);

        cursor_render();
        canvas_end();

        framebuffer ^= 1;
        // GX_CopyDisp clears depth only when Z writes are enabled.
        GX_SetZMode(GX_ENABLE, GX_LEQUAL, GX_TRUE);
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
    return 0;
}
