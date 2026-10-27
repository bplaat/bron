// A minimal program that draws a few triangles using a display list.

#include <gccore.h>
#include <malloc.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdlib.h>
#include <string.h>
#include <wiiuse/wpad.h>

#define FIFO_SIZE (256 * 1024)
#define CLEAR_COLOR ((GXColor){128, 128, 128, 255})

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

// clang-format off
_Alignas(32) static uint8_t triangle_list[32] = {
    GX_TRIANGLES | GX_VTXFMT0,
    0, 3,
    0, 1,           255, 0, 0, 255,
    -1, -1,         0, 255, 0, 255,
    1, -1,          0, 0, 255, 255
};
// clang-format on

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

    WPAD_Init();
    SYS_SetPowerCallback(power);

    GX_ClearVtxDesc();
    GX_SetVtxDesc(GX_VA_POS, GX_DIRECT);
    GX_SetVtxDesc(GX_VA_CLR0, GX_DIRECT);
    GX_SetVtxAttrFmt(GX_VTXFMT0, GX_VA_POS, GX_POS_XY, GX_S8, 0);
    GX_SetVtxAttrFmt(GX_VTXFMT0, GX_VA_CLR0, GX_CLR_RGBA, GX_RGBA8, 0);
    GX_SetNumChans(1);
    GX_SetChanCtrl(GX_COLOR0A0, GX_DISABLE, GX_SRC_VTX, GX_SRC_VTX, 0, GX_DF_NONE, GX_AF_NONE);
    GX_SetNumTexGens(0);
    GX_SetTevOrder(GX_TEVSTAGE0, GX_TEXCOORDNULL, GX_TEXMAP_NULL, GX_COLOR0A0);
    GX_SetTevOp(GX_TEVSTAGE0, GX_PASSCLR);
    GX_SetCullMode(GX_CULL_NONE);
    DCFlushRange(triangle_list, sizeof(triangle_list));

    Mtx44 projection;
    guPerspective(projection, 45.0f, aspect, 0.1f, 1000.0f);
    GX_LoadProjectionMtx(projection, GX_PERSPECTIVE);

    float rotation = 0.0f;
    while (!power_pressed) {
        WPAD_ScanPads();
        if (home_pressed())
            break;
        rotation += 1.0f;

        for (int32_t y = -2; y < 2; y++) {
            for (int32_t x = -2; x < 2; x++) {
                Mtx matrix;
                Mtx temp;
                ps_guMtxRotRad(matrix, 'x', DegToRad(rotation));
                ps_guMtxRotRad(temp, 'y', DegToRad(rotation));
                ps_guMtxConcat(matrix, temp, matrix);
                ps_guMtxTransApply(matrix, matrix, x * 2 + 1, y * 2 + 1, -10);
                GX_LoadPosMtxImm(matrix, GX_PNMTX0);
                GX_CallDispList(triangle_list, sizeof(triangle_list));
            }
        }
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
    return 0;
}
