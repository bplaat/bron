// Generates a random 64x64x64 voxel world and renders its visible block faces
// with GX display lists.

#include <gccore.h>
#include <malloc.h>
#include <ogc/lwp_watchdog.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdlib.h>
#include <string.h>
#include <wiiuse/wpad.h>

#include "mesh.h"
#include "scene.h"

#define FIFO_SIZE (256 * 1024)
#define CLEAR_COLOR ((GXColor){46, 84, 158, 255})

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

    WPAD_Init();
    SYS_SetPowerCallback(power);

    TPLFile tpl;
    GXTexObj textures[MaterialLayerCount];
    if (!materials_load(&tpl, textures))
        return 1;
    mesh_configure_pipeline();

    WorldInstances world = world_build((uint32_t)gettime());
    Mesh mesh = {0};
    if (!world.instances || !mesh_build(&mesh, &world))
        return 1;
    free(world.instances);

    Mtx44 projection;
    guPerspective(projection, 50.0f, aspect, 1.0f, 500.0f);
    GX_LoadProjectionMtx(projection, GX_PERSPECTIVE);
    GX_SetFog(GX_FOG_LIN, 120.0f, 300.0f, 1.0f, 500.0f, CLEAR_COLOR);
    uint64_t started = gettime();
    while (!power_pressed) {
        WPAD_ScanPads();
        if (WPAD_ButtonsDown(0) & WPAD_BUTTON_HOME)
            break;
        double seconds = (double)(gettime() - started) / (double)secs_to_ticks(1);
        BlocksCamera camera = blocks_camera(seconds, aspect);
        guVector eye = {camera.eye[0], camera.eye[1], camera.eye[2]};
        guVector target = {camera.target[0], camera.target[1], camera.target[2]};
        guVector up = {0, 1, 0};
        Mtx view;
        guLookAt(view, &eye, &up, &target);
        GX_LoadPosMtxImm(view, GX_PNMTX0);

        mesh_draw(&mesh, textures);

        GX_DrawDone();
        framebuffer ^= 1;
        // GX_CopyDisp clears depth only when Z writes are enabled.
        GX_SetZMode(GX_ENABLE, GX_LEQUAL, GX_TRUE);
        GX_CopyDisp(framebuffers[framebuffer], GX_TRUE);
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
