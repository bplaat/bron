// Draws a rotating textured crate with Direct3D 8, the texture is built in and decoded with GDI+.

#include <d3d8.h>
#include <gdiplus.h>
#include <math.h>
#include <objbase.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <windows.h>

typedef struct {
    float position[3];
    float tex_coord[2];
} Vertex;

#define VERTEX_FVF (D3DFVF_XYZ | D3DFVF_TEX1)

// Four corners for each face, so every face gets the whole texture
static const Vertex vertices[24] = {
    {{-1, -1, 1}, {0, 1}},  {{1, -1, 1}, {1, 1}},  {{1, 1, 1}, {1, 0}},    {{-1, 1, 1}, {0, 0}},  {{1, -1, -1}, {0, 1}},
    {{-1, -1, -1}, {1, 1}}, {{-1, 1, -1}, {1, 0}}, {{1, 1, -1}, {0, 0}},   {{1, -1, 1}, {0, 1}},  {{1, -1, -1}, {1, 1}},
    {{1, 1, -1}, {1, 0}},   {{1, 1, 1}, {0, 0}},   {{-1, -1, -1}, {0, 1}}, {{-1, -1, 1}, {1, 1}}, {{-1, 1, 1}, {1, 0}},
    {{-1, 1, -1}, {0, 0}},  {{-1, 1, 1}, {0, 1}},  {{1, 1, 1}, {1, 1}},    {{1, 1, -1}, {1, 0}},  {{-1, 1, -1}, {0, 0}},
    {{-1, -1, -1}, {0, 1}}, {{1, -1, -1}, {1, 1}}, {{1, -1, 1}, {1, 0}},   {{-1, -1, 1}, {0, 0}},
};
static const BYTE crate_jpg[] = {
#embed "crate.jpg"
};

static const float normals[6][3] = {{0, 0, 1}, {0, 0, -1}, {1, 0, 0}, {-1, 0, 0}, {0, 1, 0}, {0, -1, 0}};

typedef struct {
    IDirect3D8* d3d;
    IDirect3DDevice8* device;
    IDirect3DVertexBuffer8* vertices;
    IDirect3DTexture8* texture;
    D3DPRESENT_PARAMETERS present;
    BOOL resized;
} Cube;

static void multiply(D3DMATRIX* out, const D3DMATRIX* a, const D3DMATRIX* b) {
    D3DMATRIX result;
    for (int row = 0; row < 4; row++)
        for (int column = 0; column < 4; column++) {
            float sum = 0;
            for (int i = 0; i < 4; i++)
                sum += a->m[row][i] * b->m[i][column];
            result.m[row][column] = sum;
        }
    *out = result;
}

// A rotation for Direct3D's row vectors around axis 0 (x), 1 (y) or 2 (z)
static void rotation(D3DMATRIX* m, int axis, float angle) {
    float c = cosf(angle), s = sinf(angle);
    int a = (axis + 1) % 3, b = (axis + 2) % 3;
    memset(m, 0, sizeof(*m));
    m->m[0][0] = m->m[1][1] = m->m[2][2] = m->m[3][3] = 1.0f;
    m->m[a][a] = c;
    m->m[a][b] = s;
    m->m[b][a] = -s;
    m->m[b][b] = c;
}

// A right handed perspective projection to the depth range of Direct3D, like the other examples use
static void perspective(D3DMATRIX* m, float fov, float aspect, float near_z, float far_z) {
    float y = 1.0f / tanf(fov / 2.0f);
    memset(m, 0, sizeof(*m));
    m->_11 = y / aspect;
    m->_22 = y;
    m->_33 = far_z / (near_z - far_z);
    m->_34 = -1.0f;
    m->_43 = near_z * far_z / (near_z - far_z);
}

// Decodes an image with GDI+ into a texture with a full mipmap chain
static IDirect3DTexture8* load_texture(IDirect3DDevice8* device, const BYTE* image, UINT size) {
    // GDI+ decodes from a stream, over a copy of the image
    IStream* stream = NULL;
    GpBitmap* bitmap;
    HGLOBAL memory = GlobalAlloc(GMEM_MOVEABLE, size);
    if (!memory)
        return NULL;
    memcpy(GlobalLock(memory), image, size);
    GlobalUnlock(memory);
    if (FAILED(CreateStreamOnHGlobal(memory, TRUE, &stream))) {
        GlobalFree(memory);
        return NULL;
    }
    if (GdipCreateBitmapFromStream(stream, &bitmap) != Ok) {
        IUnknown_Release((IUnknown*)stream);
        return NULL;
    }
    UINT width, height;
    GdipGetImageWidth((GpImage*)bitmap, &width);
    GdipGetImageHeight((GpImage*)bitmap, &height);
    GpRect rect = {0, 0, (INT)width, (INT)height};
    BitmapData data;
    IDirect3DTexture8* texture = NULL;
    UINT32* pixels = malloc(width * height * 4);
    if (pixels && GdipBitmapLockBits(bitmap, &rect, ImageLockModeRead, PixelFormat32bppARGB, &data) == Ok) {
        for (UINT y = 0; y < height; y++)
            memcpy(pixels + y * width, (BYTE*)data.Scan0 + y * data.Stride, width * 4);
        GdipBitmapUnlockBits(bitmap, &data);
        if (FAILED(IDirect3DDevice8_CreateTexture(device, width, height, 0, 0, D3DFMT_A8R8G8B8, D3DPOOL_MANAGED,
                                                  &texture)))
            texture = NULL;
    }
    // Every level halves the one before, averaging each 2x2 block
    for (DWORD level = 0; texture && level < IDirect3DTexture8_GetLevelCount(texture); level++) {
        D3DLOCKED_RECT locked;
        if (level > 0) {
            UINT next_width = width > 1 ? width / 2 : 1, next_height = height > 1 ? height / 2 : 1;
            for (UINT y = 0; y < next_height; y++)
                for (UINT x = 0; x < next_width; x++) {
                    UINT x1 = x * 2 + (width > 1), y1 = y * 2 + (height > 1);
                    const UINT32 corners[4] = {pixels[y * 2 * width + x * 2], pixels[y * 2 * width + x1],
                                               pixels[y1 * width + x * 2], pixels[y1 * width + x1]};
                    UINT32 average = 0;
                    for (int shift = 0; shift < 32; shift += 8) {
                        UINT32 sum = 0;
                        for (int i = 0; i < 4; i++)
                            sum += (corners[i] >> shift) & 0xff;
                        average |= ((sum + 2) / 4) << shift;
                    }
                    pixels[y * next_width + x] = average;
                }
            width = next_width;
            height = next_height;
        }
        if (SUCCEEDED(IDirect3DTexture8_LockRect(texture, level, &locked, NULL, 0))) {
            for (UINT y = 0; y < height; y++)
                memcpy((BYTE*)locked.pBits + y * locked.Pitch, pixels + y * width, width * 4);
            IDirect3DTexture8_UnlockRect(texture, level);
        }
    }
    free(pixels);
    GdipDisposeImage((GpImage*)bitmap);
    IUnknown_Release((IUnknown*)stream);
    return texture;
}

static BOOL cube_init(Cube* cube, HWND window) {
    D3DDISPLAYMODE mode;
    cube->d3d = Direct3DCreate8(D3D_SDK_VERSION);
    if (!cube->d3d || FAILED(IDirect3D8_GetAdapterDisplayMode(cube->d3d, D3DADAPTER_DEFAULT, &mode)))
        return FALSE;

    memset(&cube->present, 0, sizeof(cube->present));
    cube->present.Windowed = TRUE;
    cube->present.SwapEffect = D3DSWAPEFFECT_DISCARD;
    cube->present.BackBufferFormat = mode.Format;
    cube->present.EnableAutoDepthStencil = TRUE;
    cube->present.AutoDepthStencilFormat = D3DFMT_D16;
    if (FAILED(IDirect3D8_CreateDevice(cube->d3d, D3DADAPTER_DEFAULT, D3DDEVTYPE_HAL, window,
                                       D3DCREATE_HARDWARE_VERTEXPROCESSING | D3DCREATE_FPU_PRESERVE, &cube->present,
                                       &cube->device)))
        return FALSE;

    void* data;
    if (FAILED(IDirect3DDevice8_CreateVertexBuffer(cube->device, sizeof(vertices), D3DUSAGE_WRITEONLY, VERTEX_FVF,
                                                   D3DPOOL_MANAGED, &cube->vertices)) ||
        FAILED(IDirect3DVertexBuffer8_Lock(cube->vertices, 0, 0, (BYTE**)&data, 0)))
        return FALSE;
    memcpy(data, vertices, sizeof(vertices));
    IDirect3DVertexBuffer8_Unlock(cube->vertices);
    cube->texture = load_texture(cube->device, crate_jpg, sizeof(crate_jpg));
    return cube->texture != NULL;
}

static void cube_render(Cube* cube, HWND window, float elapsed) {
    IDirect3DDevice8* device = cube->device;
    RECT client;
    D3DMATRIX projection, world, step;
    HRESULT result = IDirect3DDevice8_TestCooperativeLevel(device);
    // Nothing to draw while minimized or while another program has the device
    if (result == D3DERR_DEVICELOST || IsIconic(window)) {
        Sleep(50);
        return;
    }
    // A new window size or a lost device needs a reset, managed resources survive it
    if (result == D3DERR_DEVICENOTRESET || cube->resized) {
        cube->present.BackBufferWidth = 0;
        cube->present.BackBufferHeight = 0;
        if (FAILED(IDirect3DDevice8_Reset(device, &cube->present)))
            return;
        cube->resized = FALSE;
    }

    // The model and camera rotations of the WebGPU cube, then pulled back, further in portrait
    GetClientRect(window, &client);
    float aspect = client.bottom > 0 ? (float)client.right / (float)client.bottom : 1.0f;
    perspective(&projection, 45.0f * (float)M_PI / 180.0f, aspect, 0.1f, 100.0f);
    rotation(&world, 0, elapsed * 0.55f);
    rotation(&step, 2, elapsed * 0.35f);
    multiply(&world, &world, &step);
    rotation(&step, 1, 0.4f);
    multiply(&world, &world, &step);
    rotation(&step, 0, 0.25f);
    multiply(&world, &world, &step);
    world._43 = -4.5f / (aspect < 1.0f ? aspect : 1.0f);

    IDirect3DDevice8_Clear(device, 0, NULL, D3DCLEAR_TARGET | D3DCLEAR_ZBUFFER, D3DCOLOR_XRGB(0, 0, 0), 1.0f, 0);
    if (SUCCEEDED(IDirect3DDevice8_BeginScene(device))) {
        IDirect3DDevice8_SetTransform(device, D3DTS_PROJECTION, &projection);
        IDirect3DDevice8_SetTransform(device, D3DTS_WORLD, &world);
        IDirect3DDevice8_SetRenderState(device, D3DRS_LIGHTING, FALSE);
        IDirect3DDevice8_SetRenderState(device, D3DRS_ZENABLE, D3DZB_TRUE);
        // Counter-clockwise from outside like the other examples, the right handed projection keeps that on screen
        IDirect3DDevice8_SetRenderState(device, D3DRS_CULLMODE, D3DCULL_CW);
        IDirect3DDevice8_SetTexture(device, 0, (IDirect3DBaseTexture8*)cube->texture);
        IDirect3DDevice8_SetTextureStageState(device, 0, D3DTSS_COLOROP, D3DTOP_MODULATE);
        IDirect3DDevice8_SetTextureStageState(device, 0, D3DTSS_COLORARG1, D3DTA_TEXTURE);
        IDirect3DDevice8_SetTextureStageState(device, 0, D3DTSS_COLORARG2, D3DTA_TFACTOR);
        IDirect3DDevice8_SetTextureStageState(device, 0, D3DTSS_MINFILTER, D3DTEXF_LINEAR);
        IDirect3DDevice8_SetTextureStageState(device, 0, D3DTSS_MAGFILTER, D3DTEXF_LINEAR);
        IDirect3DDevice8_SetTextureStageState(device, 0, D3DTSS_MIPFILTER, D3DTEXF_LINEAR);
        IDirect3DDevice8_SetVertexShader(device, VERTEX_FVF);
        IDirect3DDevice8_SetStreamSource(device, 0, cube->vertices, sizeof(Vertex));
        for (int face = 0; face < 6; face++) {
            // Like the WebGPU shader: the rotated normal against a fixed light, lit from both sides
            const float light[3] = {0.4f, 0.8f, 0.6f};
            float dot = 0;
            for (int column = 0; column < 3; column++) {
                float rotated = 0;
                for (int row = 0; row < 3; row++)
                    rotated += normals[face][row] * world.m[row][column];
                dot += rotated * light[column];
            }
            BYTE shade = (BYTE)(255.0f * (0.35f + 0.65f * fabsf(dot) / sqrtf(1.16f)));
            IDirect3DDevice8_SetRenderState(device, D3DRS_TEXTUREFACTOR, D3DCOLOR_XRGB(shade, shade, shade));
            IDirect3DDevice8_DrawPrimitive(device, D3DPT_TRIANGLEFAN, (UINT)face * 4, 2);
        }
        IDirect3DDevice8_EndScene(device);
    }
    IDirect3DDevice8_Present(device, NULL, NULL, NULL, NULL);
}

static void cube_destroy(Cube* cube) {
    if (cube->texture)
        IDirect3DTexture8_Release(cube->texture);
    if (cube->vertices)
        IDirect3DVertexBuffer8_Release(cube->vertices);
    if (cube->device)
        IDirect3DDevice8_Release(cube->device);
    if (cube->d3d)
        IDirect3D8_Release(cube->d3d);
}

static Cube cube;

static LRESULT CALLBACK window_proc(HWND window, UINT message, WPARAM wparam, LPARAM lparam) {
    switch (message) {
        case WM_SIZE:
            cube.resized = TRUE;
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
    window_class.lpszClassName = L"Cube";
    RegisterClassW(&window_class);

    RECT rect = {0, 0, 960, 720};
    AdjustWindowRect(&rect, WS_OVERLAPPEDWINDOW, FALSE);
    HWND window = CreateWindowExW(0, L"Cube", L"Direct3D 8 Cube", WS_OVERLAPPEDWINDOW, CW_USEDEFAULT, CW_USEDEFAULT,
                                  rect.right - rect.left, rect.bottom - rect.top, NULL, NULL, instance, NULL);
    if (!cube_init(&cube, window)) {
        MessageBoxW(NULL, L"This program needs a Direct3D 8 graphics adapter with hardware acceleration.",
                    L"Direct3D 8 Cube", MB_ICONERROR);
        cube_destroy(&cube);
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
        cube_render(&cube, window, (float)(now.QuadPart - started.QuadPart) / (float)frequency.QuadPart);
        frames++;
        if (now.QuadPart - sample_start >= frequency.QuadPart) {
            WCHAR title[64];
            swprintf(title, L"Direct3D 8 Cube - %d FPS",
                     (int)(frames * frequency.QuadPart / (now.QuadPart - sample_start)));
            SetWindowTextW(window, title);
            sample_start = now.QuadPart;
            frames = 0;
        }
    }

    cube_destroy(&cube);
    GdiplusShutdown(token);
    return (int)message.wParam;
}
