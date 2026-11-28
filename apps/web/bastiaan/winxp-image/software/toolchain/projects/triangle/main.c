// Draws a rotating colored triangle with Direct3D 8.

#include <d3d8.h>
#include <math.h>
#include <string.h>
#include <windows.h>

typedef struct {
    float position[3];
    D3DCOLOR color;
} Vertex;

#define VERTEX_FVF (D3DFVF_XYZ | D3DFVF_DIFFUSE)

static const Vertex vertices[3] = {
    {{0.0f, 1.0f, 0.0f}, 0xffff1a1a},
    {{-1.0f, -1.0f, 0.0f}, 0xff1aff33},
    {{1.0f, -1.0f, 0.0f}, 0xff1a4dff},
};

typedef struct {
    IDirect3D8* d3d;
    IDirect3DDevice8* device;
    IDirect3DVertexBuffer8* vertices;
    D3DPRESENT_PARAMETERS present;
    BOOL resized;
} Triangle;

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

static BOOL triangle_create_resources(Triangle* triangle) {
    IDirect3DDevice8* device = triangle->device;
    void* data;
    if (FAILED(IDirect3DDevice8_CreateVertexBuffer(device, sizeof(vertices), D3DUSAGE_WRITEONLY, VERTEX_FVF,
                                                   D3DPOOL_MANAGED, &triangle->vertices)) ||
        FAILED(IDirect3DVertexBuffer8_Lock(triangle->vertices, 0, 0, (BYTE**)&data, 0)))
        return FALSE;
    memcpy(data, vertices, sizeof(vertices));
    IDirect3DVertexBuffer8_Unlock(triangle->vertices);
    return TRUE;
}

static BOOL triangle_init(Triangle* triangle, HWND window) {
    D3DDISPLAYMODE mode;
    triangle->d3d = Direct3DCreate8(D3D_SDK_VERSION);
    if (!triangle->d3d || FAILED(IDirect3D8_GetAdapterDisplayMode(triangle->d3d, D3DADAPTER_DEFAULT, &mode)))
        return FALSE;

    memset(&triangle->present, 0, sizeof(triangle->present));
    triangle->present.Windowed = TRUE;
    triangle->present.SwapEffect = D3DSWAPEFFECT_DISCARD;
    triangle->present.BackBufferFormat = mode.Format;
    return SUCCEEDED(IDirect3D8_CreateDevice(triangle->d3d, D3DADAPTER_DEFAULT, D3DDEVTYPE_HAL, window,
                                             D3DCREATE_HARDWARE_VERTEXPROCESSING | D3DCREATE_FPU_PRESERVE,
                                             &triangle->present, &triangle->device)) &&
           triangle_create_resources(triangle);
}

static void triangle_render(Triangle* triangle, HWND window, float seconds) {
    IDirect3DDevice8* device = triangle->device;
    RECT client;
    D3DMATRIX projection, world;
    HRESULT result = IDirect3DDevice8_TestCooperativeLevel(device);
    // Nothing to draw while minimized or while another program has the device
    if (result == D3DERR_DEVICELOST || IsIconic(window)) {
        Sleep(50);
        return;
    }
    // A new window size or a lost device needs a reset, managed resources survive it
    if (result == D3DERR_DEVICENOTRESET || triangle->resized) {
        triangle->present.BackBufferWidth = 0;
        triangle->present.BackBufferHeight = 0;
        if (FAILED(IDirect3DDevice8_Reset(device, &triangle->present)))
            return;
        triangle->resized = FALSE;
    }

    GetClientRect(window, &client);
    float aspect = client.bottom > 0 ? (float)client.right / (float)client.bottom : 1.0f;
    perspective(&projection, 45.0f * (float)M_PI / 180.0f, aspect, 0.1f, 100.0f);
    // Around the y axis, three units in front of the camera
    memset(&world, 0, sizeof(world));
    world._11 = cosf(seconds);
    world._13 = -sinf(seconds);
    world._22 = 1.0f;
    world._31 = sinf(seconds);
    world._33 = cosf(seconds);
    world._43 = -3.0f;
    world._44 = 1.0f;

    IDirect3DDevice8_Clear(device, 0, NULL, D3DCLEAR_TARGET, D3DCOLOR_XRGB(0, 0, 0), 1.0f, 0);
    if (SUCCEEDED(IDirect3DDevice8_BeginScene(device))) {
        IDirect3DDevice8_SetTransform(device, D3DTS_PROJECTION, &projection);
        IDirect3DDevice8_SetTransform(device, D3DTS_WORLD, &world);
        IDirect3DDevice8_SetRenderState(device, D3DRS_LIGHTING, FALSE);
        IDirect3DDevice8_SetRenderState(device, D3DRS_CULLMODE, D3DCULL_NONE);
        IDirect3DDevice8_SetVertexShader(device, VERTEX_FVF);
        IDirect3DDevice8_SetStreamSource(device, 0, triangle->vertices, sizeof(Vertex));
        IDirect3DDevice8_DrawPrimitive(device, D3DPT_TRIANGLELIST, 0, 1);
        IDirect3DDevice8_EndScene(device);
    }
    IDirect3DDevice8_Present(device, NULL, NULL, NULL, NULL);
}

static void triangle_destroy(Triangle* triangle) {
    if (triangle->vertices)
        IDirect3DVertexBuffer8_Release(triangle->vertices);
    if (triangle->device)
        IDirect3DDevice8_Release(triangle->device);
    if (triangle->d3d)
        IDirect3D8_Release(triangle->d3d);
}

static Triangle triangle;

static LRESULT CALLBACK window_proc(HWND window, UINT message, WPARAM wparam, LPARAM lparam) {
    switch (message) {
        case WM_SIZE:
            triangle.resized = TRUE;
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
    WNDCLASSW window_class = {0};
    window_class.lpfnWndProc = window_proc;
    window_class.hInstance = instance;
    window_class.hCursor = LoadCursorW(NULL, (LPCWSTR)IDC_ARROW);
    window_class.lpszClassName = L"Triangle";
    RegisterClassW(&window_class);

    RECT rect = {0, 0, 960, 720};
    AdjustWindowRect(&rect, WS_OVERLAPPEDWINDOW, FALSE);
    HWND window =
        CreateWindowExW(0, L"Triangle", L"Direct3D 8 Triangle", WS_OVERLAPPEDWINDOW, CW_USEDEFAULT, CW_USEDEFAULT,
                        rect.right - rect.left, rect.bottom - rect.top, NULL, NULL, instance, NULL);
    if (!triangle_init(&triangle, window)) {
        MessageBoxW(NULL, L"This program needs a Direct3D 8 graphics adapter with hardware acceleration.",
                    L"Direct3D 8 Triangle", MB_ICONERROR);
        triangle_destroy(&triangle);
        return 1;
    }
    ShowWindow(window, show);

    LARGE_INTEGER frequency, started, now;
    QueryPerformanceFrequency(&frequency);
    QueryPerformanceCounter(&started);
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
        triangle_render(&triangle, window, (float)(now.QuadPart - started.QuadPart) / (float)frequency.QuadPart);
    }

    triangle_destroy(&triangle);
    return (int)message.wParam;
}
