#include <gdiplus.h>
#include <windows.h>

#define TURN 6.283185307179586

static void circle(GpGraphics* canvas, ARGB color, float x, float y, float radius) {
    GpSolidFill* brush;
    if (GdipCreateSolidFill(color, &brush) == Ok) {
        GdipFillEllipse(canvas, (GpBrush*)brush, x - radius, y - radius, 2 * radius, 2 * radius);
        GdipDeleteBrush((GpBrush*)brush);
    }
}

static void radial_line(GpGraphics* canvas, GpPen* pen, float x, float y, float radius, double turn, float inner,
                        float outer) {
    float dx = (float)sin(turn * TURN) * radius;
    float dy = -(float)cos(turn * TURN) * radius;
    GdipDrawLine(canvas, pen, x + inner * dx, y + inner * dy, x + outer * dx, y + outer * dy);
}

static void draw_face(GpGraphics* canvas, float x, float y, float radius) {
    GdipSetSmoothingMode(canvas, SmoothingModeAntiAlias);
    GdipGraphicsClear(canvas, 0xff101827);
    circle(canvas, 0xff344864, x, y, radius);
    circle(canvas, 0xff1b2a40, x, y, radius * 0.975f);

    GpPen* pen;
    if (GdipCreatePen1(0xff88a2c2, radius * 0.01f, UnitPixel, &pen) != Ok)
        return;

    for (int mark = 0; mark < 60; ++mark) {
        BOOL hour = mark % 5 == 0;
        GdipSetPenColor(pen, hour ? 0xffd7e7fa : 0xff6984a5);
        GdipSetPenWidth(pen, radius * (hour ? 0.021f : 0.009f));
        radial_line(canvas, pen, x, y, radius, mark / 60.0, hour ? 0.82f : 0.89f, 0.94f);
    }

    SYSTEMTIME time;
    GetLocalTime(&time);
    double minute = time.wMinute + time.wSecond / 60.0;
    GdipSetPenStartCap(pen, LineCapRound);
    GdipSetPenEndCap(pen, LineCapRound);
    GdipSetPenColor(pen, 0xfff2f7ff);
    GdipSetPenWidth(pen, radius * 0.062f);
    radial_line(canvas, pen, x, y, radius, (time.wHour + minute / 60.0) / 12.0, 0, 0.47f);
    GdipSetPenWidth(pen, radius * 0.038f);
    radial_line(canvas, pen, x, y, radius, minute / 60.0, 0, 0.66f);
    GdipSetPenColor(pen, 0xffffa15d);
    GdipSetPenWidth(pen, radius * 0.014f);
    radial_line(canvas, pen, x, y, radius, time.wSecond / 60.0, -0.15f, 0.80f);
    GdipDeletePen(pen);
    circle(canvas, 0xffffa15d, x, y, radius * 0.049f);
    circle(canvas, 0xff1b2a40, x, y, radius * 0.019f);
}

static void draw_numbers(HDC canvas, float x, float y, float radius) {
    static const WCHAR* const labels[] = {L"12", L"3", L"6", L"9"};
    HFONT font = CreateFontW(-(int)(radius * 0.16f), 0, 0, 0, FW_SEMIBOLD, FALSE, FALSE, FALSE, DEFAULT_CHARSET,
                             OUT_DEFAULT_PRECIS, CLIP_DEFAULT_PRECIS, ANTIALIASED_QUALITY, DEFAULT_PITCH, L"Tahoma");
    HGDIOBJ previous = font ? SelectObject(canvas, font) : NULL;

    SetTextColor(canvas, RGB(216, 230, 248));
    SetBkMode(canvas, TRANSPARENT);
    for (int index = 0; index < 4; ++index) {
        float angle = (float)(index * TURN / 4);
        int cx = (int)(x + radius * 0.72f * sin(angle));
        int cy = (int)(y - radius * 0.72f * cos(angle));
        int size = (int)(radius * 0.20f);
        RECT label = {cx - size, cy - size, cx + size, cy + size};
        DrawTextW(canvas, labels[index], -1, &label, DT_CENTER | DT_VCENTER | DT_SINGLELINE);
    }
    if (previous)
        SelectObject(canvas, previous);
    if (font)
        DeleteObject(font);
}

static void paint(HWND window) {
    PAINTSTRUCT paint;
    RECT client;
    HDC target = BeginPaint(window, &paint);
    GetClientRect(window, &client);
    int width = client.right;
    int height = client.bottom;
    HDC buffer = CreateCompatibleDC(target);
    HBITMAP bitmap = buffer ? CreateCompatibleBitmap(target, width, height) : NULL;
    HGDIOBJ previous = bitmap ? SelectObject(buffer, bitmap) : NULL;
    HDC canvas = previous ? buffer : target;
    float radius = (width < height ? width : height) * 0.46f;

    GpGraphics* graphics;
    if (radius >= 12 && GdipCreateFromHDC(canvas, &graphics) == Ok) {
        draw_face(graphics, width / 2.0f, height / 2.0f, radius);
        GdipDeleteGraphics(graphics);
        draw_numbers(canvas, width / 2.0f, height / 2.0f, radius);
    } else {
        FillRect(canvas, &client, GetSysColorBrush(COLOR_WINDOW));
    }
    if (previous) {
        BitBlt(target, 0, 0, width, height, buffer, 0, 0, SRCCOPY);
        SelectObject(buffer, previous);
    }
    if (bitmap)
        DeleteObject(bitmap);
    if (buffer)
        DeleteDC(buffer);
    EndPaint(window, &paint);
}

static LRESULT CALLBACK window_proc(HWND window, UINT message, WPARAM wparam, LPARAM lparam) {
    switch (message) {
        case WM_CREATE:
            return SetTimer(window, 1, 200, NULL) ? 0 : -1;
        case WM_TIMER:
            InvalidateRect(window, NULL, FALSE);
            return 0;
        case WM_PAINT:
            paint(window);
            return 0;
        case WM_DESTROY:
            KillTimer(window, 1);
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
    window_class.style = CS_HREDRAW | CS_VREDRAW;
    window_class.lpfnWndProc = window_proc;
    window_class.hInstance = instance;
    window_class.hCursor = LoadCursorW(NULL, IDC_ARROW);
    window_class.lpszClassName = L"AnalogClock";
    HWND window = NULL;
    if (RegisterClassW(&window_class)) {
        window = CreateWindowExW(0, window_class.lpszClassName, L"Analog Clock", WS_OVERLAPPEDWINDOW, CW_USEDEFAULT,
                                 CW_USEDEFAULT, 370, 395, NULL, NULL, instance, NULL);
    }

    int exit_code = 1;
    if (window) {
        ShowWindow(window, show);
        MSG message;
        int result;
        while ((result = GetMessageW(&message, NULL, 0, 0)) > 0) {
            TranslateMessage(&message);
            DispatchMessageW(&message);
        }
        if (result >= 0)
            exit_code = (int)message.wParam;
    }
    GdiplusShutdown(token);
    return exit_code;
}
