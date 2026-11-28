// Generates a random 64x64x64 voxel world and renders its visible block faces with Direct3D 8.

#include <d3d8.h>
#include <gdiplus.h>
#include <math.h>
#include <stdio.h>
#include <string.h>
#include <windows.h>

#include "materials.h"
#include "world.h"

typedef struct {
    float position[3];
    D3DCOLOR color;
    float tex_coord[2];
} Vertex;

#define VERTEX_FVF (D3DFVF_XYZ | D3DFVF_DIFFUSE | D3DFVF_TEX1)
// 16 bit indices reach 65536 vertices, every batch of quads starts at its own base vertex
#define BATCH_QUADS 16384

// Face order matches faceOffsets in world.h, the corners are offsets from the voxel's minimum corner
static const BYTE corners[6][4][3] = {
    {{0, 0, 1}, {1, 0, 1}, {1, 1, 1}, {0, 1, 1}}, {{1, 0, 0}, {0, 0, 0}, {0, 1, 0}, {1, 1, 0}},
    {{1, 0, 1}, {1, 0, 0}, {1, 1, 0}, {1, 1, 1}}, {{0, 0, 0}, {0, 0, 1}, {0, 1, 1}, {0, 1, 0}},
    {{0, 1, 1}, {1, 1, 1}, {1, 1, 0}, {0, 1, 0}}, {{0, 0, 0}, {1, 0, 0}, {1, 0, 1}, {0, 0, 1}}};
static const float uv[4][2] = {{0, 1}, {1, 1}, {1, 0}, {0, 0}};

// max(dot(normal, normalize([0.45, 0.78, 0.30])), 0) for every face
static const float face_light[6] = {0.31606977f, 0, 0.47410466f, 0, 0.82178140f, 0};

typedef struct {
    IDirect3D8* d3d;
    IDirect3DDevice8* device;
    IDirect3DVertexBuffer8* vertices;
    IDirect3DIndexBuffer8* indices;
    Materials materials;
    // The quads of every material, sorted by material so each draws one range
    UINT first[MaterialLayerCount];
    UINT count[MaterialLayerCount];
    D3DPRESENT_PARAMETERS present;
    BOOL resized;
} Blocks;

static void emit_face(Vertex* vertices, const VoxelInstance* instance, BOOL water) {
    const BYTE(*face)[3] = corners[instance->face];
    BYTE shade = (BYTE)((0.42f + 0.58f * face_light[instance->face]) * (float)instance->occlusion);
    for (int corner = 0; corner < 4; corner++) {
        vertices[corner].position[0] = (float)(instance->x + face[corner][0]);
        vertices[corner].position[1] = (float)(instance->y + face[corner][1]);
        vertices[corner].position[2] = (float)(instance->z + face[corner][2]);
        vertices[corner].color = D3DCOLOR_ARGB(water ? 153 : 255, shade, shade, shade);
        vertices[corner].tex_coord[0] = uv[corner][0];
        vertices[corner].tex_coord[1] = uv[corner][1];
    }
}

static BOOL blocks_build_mesh(Blocks* blocks, const WorldInstances* world) {
    IDirect3DDevice8* device = blocks->device;
    size_t total = world->opaqueCount + world->translucentCount;
    UINT quads = 0, next[MaterialLayerCount];
    for (size_t i = 0; i < total; i++)
        blocks->count[world->instances[i].material]++;
    for (int material = 0; material < MaterialLayerCount; material++) {
        blocks->first[material] = next[material] = quads;
        quads += blocks->count[material];
    }

    Vertex* vertices;
    if (FAILED(IDirect3DDevice8_CreateVertexBuffer(device, quads * 4 * sizeof(Vertex), D3DUSAGE_WRITEONLY, VERTEX_FVF,
                                                   D3DPOOL_MANAGED, &blocks->vertices)) ||
        FAILED(IDirect3DVertexBuffer8_Lock(blocks->vertices, 0, 0, (BYTE**)&vertices, 0)))
        return FALSE;
    for (size_t i = 0; i < total; i++) {
        const VoxelInstance* instance = &world->instances[i];
        emit_face(&vertices[next[instance->material]++ * 4], instance, instance->material == MaterialLayerWater);
    }
    IDirect3DVertexBuffer8_Unlock(blocks->vertices);

    // Two triangles for every quad of a batch
    WORD* indices;
    if (FAILED(IDirect3DDevice8_CreateIndexBuffer(device, BATCH_QUADS * 6 * sizeof(WORD), D3DUSAGE_WRITEONLY,
                                                  D3DFMT_INDEX16, D3DPOOL_MANAGED, &blocks->indices)) ||
        FAILED(IDirect3DIndexBuffer8_Lock(blocks->indices, 0, 0, (BYTE**)&indices, 0)))
        return FALSE;
    for (UINT quad = 0; quad < BATCH_QUADS; quad++) {
        static const WORD pattern[6] = {0, 1, 2, 0, 2, 3};
        for (int i = 0; i < 6; i++)
            indices[quad * 6 + i] = (WORD)(quad * 4 + pattern[i]);
    }
    IDirect3DIndexBuffer8_Unlock(blocks->indices);
    return TRUE;
}

static BOOL blocks_init(Blocks* blocks, HWND window) {
    D3DDISPLAYMODE mode;
    blocks->d3d = Direct3DCreate8(D3D_SDK_VERSION);
    if (!blocks->d3d || FAILED(IDirect3D8_GetAdapterDisplayMode(blocks->d3d, D3DADAPTER_DEFAULT, &mode)))
        return FALSE;

    memset(&blocks->present, 0, sizeof(blocks->present));
    blocks->present.Windowed = TRUE;
    blocks->present.SwapEffect = D3DSWAPEFFECT_DISCARD;
    blocks->present.BackBufferFormat = mode.Format;
    blocks->present.EnableAutoDepthStencil = TRUE;
    blocks->present.AutoDepthStencilFormat = D3DFMT_D16;
    if (FAILED(IDirect3D8_CreateDevice(blocks->d3d, D3DADAPTER_DEFAULT, D3DDEVTYPE_HAL, window,
                                       D3DCREATE_HARDWARE_VERTEXPROCESSING | D3DCREATE_FPU_PRESERVE, &blocks->present,
                                       &blocks->device)) ||
        !materials_load(&blocks->materials, blocks->device))
        return FALSE;

    WorldInstances world = world_build(GetTickCount());
    BOOL built = world.instances && blocks_build_mesh(blocks, &world);
    free(world.instances);
    return built;
}

static void subtract(float out[3], const float a[3], const float b[3]) {
    for (int i = 0; i < 3; i++)
        out[i] = a[i] - b[i];
}

static void cross(float out[3], const float a[3], const float b[3]) {
    out[0] = a[1] * b[2] - a[2] * b[1];
    out[1] = a[2] * b[0] - a[0] * b[2];
    out[2] = a[0] * b[1] - a[1] * b[0];
}

static float dot(const float a[3], const float b[3]) {
    return a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
}

static void normalize(float v[3]) {
    float length = sqrtf(dot(v, v));
    for (int i = 0; i < 3; i++)
        v[i] /= length;
}

// A right handed view looking from the eye at the target, like the other examples use
static void look_at(D3DMATRIX* m, const float eye[3], const float target[3]) {
    static const float up[3] = {0, 1, 0};
    float x[3], y[3], z[3];
    subtract(z, eye, target);
    normalize(z);
    cross(x, up, z);
    normalize(x);
    cross(y, z, x);
    memset(m, 0, sizeof(*m));
    for (int i = 0; i < 3; i++) {
        m->m[i][0] = x[i];
        m->m[i][1] = y[i];
        m->m[i][2] = z[i];
    }
    m->_41 = -dot(x, eye);
    m->_42 = -dot(y, eye);
    m->_43 = -dot(z, eye);
    m->_44 = 1.0f;
}

// A right handed perspective projection to the depth range of Direct3D
static void perspective(D3DMATRIX* m, float fov, float aspect, float near_z, float far_z) {
    float y = 1.0f / tanf(fov / 2.0f);
    memset(m, 0, sizeof(*m));
    m->_11 = y / aspect;
    m->_22 = y;
    m->_33 = far_z / (near_z - far_z);
    m->_34 = -1.0f;
    m->_43 = near_z * far_z / (near_z - far_z);
}

static void blocks_draw_material(Blocks* blocks, int material) {
    IDirect3DDevice8* device = blocks->device;
    IDirect3DDevice8_SetTexture(device, 0, (IDirect3DBaseTexture8*)blocks->materials.textures[material]);
    for (UINT done = 0; done < blocks->count[material]; done += BATCH_QUADS) {
        UINT quads = blocks->count[material] - done < BATCH_QUADS ? blocks->count[material] - done : BATCH_QUADS;
        IDirect3DDevice8_SetIndices(device, blocks->indices, (blocks->first[material] + done) * 4);
        IDirect3DDevice8_DrawIndexedPrimitive(device, D3DPT_TRIANGLELIST, 0, quads * 4, 0, quads * 2);
    }
}

static void blocks_render(Blocks* blocks, HWND window, double seconds) {
    IDirect3DDevice8* device = blocks->device;
    RECT client;
    D3DMATRIX projection, view;
    HRESULT result = IDirect3DDevice8_TestCooperativeLevel(device);
    // Nothing to draw while minimized or while another program has the device
    if (result == D3DERR_DEVICELOST || IsIconic(window)) {
        Sleep(50);
        return;
    }
    // A new window size or a lost device needs a reset, managed resources survive it
    if (result == D3DERR_DEVICENOTRESET || blocks->resized) {
        blocks->present.BackBufferWidth = 0;
        blocks->present.BackBufferHeight = 0;
        if (FAILED(IDirect3DDevice8_Reset(device, &blocks->present)))
            return;
        blocks->resized = FALSE;
    }

    // Orbits the center of the diorama, pulled back in portrait so it stays in view
    GetClientRect(window, &client);
    float aspect = client.bottom > 0 ? (float)client.right / (float)client.bottom : 1.0f;
    float angle = (float)fmod(seconds * 0.16, 2.0 * M_PI);
    float scale = 1.0f / (aspect < 1.0f ? aspect : 1.0f);
    const float target[3] = {32.0f, 21.76f, 32.0f};
    const float eye[3] = {32.0f + sinf(angle) * 88.0f * scale, 21.76f + 58.0f * scale,
                          32.0f + cosf(angle) * 88.0f * scale};
    perspective(&projection, 50.0f * (float)M_PI / 180.0f, aspect, 1.0f, 500.0f);
    look_at(&view, eye, target);

    IDirect3DDevice8_Clear(device, 0, NULL, D3DCLEAR_TARGET | D3DCLEAR_ZBUFFER, D3DCOLOR_XRGB(46, 84, 158), 1.0f, 0);
    if (SUCCEEDED(IDirect3DDevice8_BeginScene(device))) {
        IDirect3DDevice8_SetTransform(device, D3DTS_PROJECTION, &projection);
        IDirect3DDevice8_SetTransform(device, D3DTS_VIEW, &view);
        IDirect3DDevice8_SetRenderState(device, D3DRS_LIGHTING, FALSE);
        IDirect3DDevice8_SetRenderState(device, D3DRS_ZENABLE, D3DZB_TRUE);
        IDirect3DDevice8_SetTextureStageState(device, 0, D3DTSS_COLOROP, D3DTOP_MODULATE);
        IDirect3DDevice8_SetTextureStageState(device, 0, D3DTSS_COLORARG1, D3DTA_TEXTURE);
        IDirect3DDevice8_SetTextureStageState(device, 0, D3DTSS_COLORARG2, D3DTA_DIFFUSE);
        IDirect3DDevice8_SetTextureStageState(device, 0, D3DTSS_ALPHAOP, D3DTOP_MODULATE);
        IDirect3DDevice8_SetTextureStageState(device, 0, D3DTSS_ALPHAARG1, D3DTA_TEXTURE);
        IDirect3DDevice8_SetTextureStageState(device, 0, D3DTSS_ALPHAARG2, D3DTA_DIFFUSE);
        // Sharp texels up close like the other block renderers, blended mipmaps in the distance
        IDirect3DDevice8_SetTextureStageState(device, 0, D3DTSS_MINFILTER, D3DTEXF_POINT);
        IDirect3DDevice8_SetTextureStageState(device, 0, D3DTSS_MAGFILTER, D3DTEXF_POINT);
        IDirect3DDevice8_SetTextureStageState(device, 0, D3DTSS_MIPFILTER, D3DTEXF_LINEAR);
        IDirect3DDevice8_SetVertexShader(device, VERTEX_FVF);
        IDirect3DDevice8_SetStreamSource(device, 0, blocks->vertices, sizeof(Vertex));

        // Faces wind counter-clockwise seen from outside, culling the clockwise ones skips those seen from behind
        IDirect3DDevice8_SetRenderState(device, D3DRS_CULLMODE, D3DCULL_CW);
        IDirect3DDevice8_SetRenderState(device, D3DRS_ZWRITEENABLE, TRUE);
        IDirect3DDevice8_SetRenderState(device, D3DRS_ALPHABLENDENABLE, FALSE);
        for (int material = 0; material < MaterialLayerCount; material++)
            if (material != MaterialLayerWater)
                blocks_draw_material(blocks, material);

        // The translucent water goes last, blended over the terrain without writing depth
        IDirect3DDevice8_SetRenderState(device, D3DRS_CULLMODE, D3DCULL_NONE);
        IDirect3DDevice8_SetRenderState(device, D3DRS_ZWRITEENABLE, FALSE);
        IDirect3DDevice8_SetRenderState(device, D3DRS_ALPHABLENDENABLE, TRUE);
        IDirect3DDevice8_SetRenderState(device, D3DRS_SRCBLEND, D3DBLEND_SRCALPHA);
        IDirect3DDevice8_SetRenderState(device, D3DRS_DESTBLEND, D3DBLEND_INVSRCALPHA);
        blocks_draw_material(blocks, MaterialLayerWater);
        IDirect3DDevice8_EndScene(device);
    }
    IDirect3DDevice8_Present(device, NULL, NULL, NULL, NULL);
}

static void blocks_destroy(Blocks* blocks) {
    materials_destroy(&blocks->materials);
    if (blocks->indices)
        IDirect3DIndexBuffer8_Release(blocks->indices);
    if (blocks->vertices)
        IDirect3DVertexBuffer8_Release(blocks->vertices);
    if (blocks->device)
        IDirect3DDevice8_Release(blocks->device);
    if (blocks->d3d)
        IDirect3D8_Release(blocks->d3d);
}

static Blocks blocks;

static LRESULT CALLBACK window_proc(HWND window, UINT message, WPARAM wparam, LPARAM lparam) {
    switch (message) {
        case WM_SIZE:
            blocks.resized = TRUE;
            return 0;
        case WM_KEYDOWN:
            if (wparam == VK_ESCAPE)
                DestroyWindow(window);
            return 0;
        case WM_DESTROY:
            PostQuitMessage(0);
            return 0;
        default:
            return DefWindowProcW(window, message, wparam, lparam);
    }
}

int WINAPI WinMain(HINSTANCE instance, HINSTANCE previous, LPSTR command_line, int show) {
    GdiplusStartupInput startup = {1, NULL, FALSE, FALSE};
    ULONG_PTR token;
    if (GdiplusStartup(&token, &startup, NULL) != Ok)
        return 1;

    WNDCLASSW window_class = {0};
    window_class.lpfnWndProc = window_proc;
    window_class.hInstance = instance;
    window_class.hCursor = LoadCursorW(NULL, (LPCWSTR)IDC_ARROW);
    window_class.lpszClassName = L"Blocks";
    RegisterClassW(&window_class);

    RECT rect = {0, 0, 960, 720};
    AdjustWindowRect(&rect, WS_OVERLAPPEDWINDOW, FALSE);
    HWND window = CreateWindowExW(0, L"Blocks", L"Direct3D 8 Blocks", WS_OVERLAPPEDWINDOW, CW_USEDEFAULT, CW_USEDEFAULT,
                                  rect.right - rect.left, rect.bottom - rect.top, NULL, NULL, instance, NULL);
    if (!blocks_init(&blocks, window)) {
        MessageBoxW(NULL, L"This program needs a Direct3D 8 graphics adapter with hardware acceleration.",
                    L"Direct3D 8 Blocks", MB_ICONERROR);
        blocks_destroy(&blocks);
        GdiplusShutdown(token);
        return 1;
    }
    ShowWindow(window, show);

    // The frame rate goes in the title, once a second
    LARGE_INTEGER frequency, started, now;
    QueryPerformanceFrequency(&frequency);
    QueryPerformanceCounter(&started);
    LONGLONG sample_start = started.QuadPart;
    int frames = 0;
    MSG message;
    for (;;) {
        if (PeekMessageW(&message, NULL, 0, 0, PM_REMOVE)) {
            if (message.message == WM_QUIT)
                break;
            TranslateMessage(&message);
            DispatchMessageW(&message);
            continue;
        }
        QueryPerformanceCounter(&now);
        blocks_render(&blocks, window, (double)(now.QuadPart - started.QuadPart) / (double)frequency.QuadPart);
        frames++;
        if (now.QuadPart - sample_start >= frequency.QuadPart) {
            WCHAR title[64];
            swprintf(title, L"Direct3D 8 Blocks - %d FPS",
                     (int)(frames * frequency.QuadPart / (now.QuadPart - sample_start)));
            SetWindowTextW(window, title);
            sample_start = now.QuadPart;
            frames = 0;
        }
    }

    blocks_destroy(&blocks);
    GdiplusShutdown(token);
    return (int)message.wParam;
}
