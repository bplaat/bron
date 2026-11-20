#include <windows.h>

static void paint(HWND window) {
    PAINTSTRUCT paint;
    RECT client;
    HDC target = BeginPaint(window, &paint);
    GetClientRect(window, &client);

    HDC buffer = CreateCompatibleDC(target);
    HBITMAP bitmap = buffer ? CreateCompatibleBitmap(target, client.right, client.bottom) : NULL;
    HGDIOBJ previous = bitmap ? SelectObject(buffer, bitmap) : NULL;
    HDC canvas = previous ? buffer : target;

    FillRect(canvas, &client, GetSysColorBrush(COLOR_WINDOW));
    SelectObject(canvas, GetStockObject(DEFAULT_GUI_FONT));
    SetTextColor(canvas, GetSysColor(COLOR_WINDOWTEXT));
    SetBkMode(canvas, TRANSPARENT);
    DrawTextW(canvas, L"Hello, world!", -1, &client, DT_CENTER | DT_VCENTER | DT_SINGLELINE);

    if (previous) {
        BitBlt(target, 0, 0, client.right, client.bottom, buffer, 0, 0, SRCCOPY);
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
        case WM_PAINT:
            paint(window);
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
    window_class.style = CS_HREDRAW | CS_VREDRAW;
    window_class.lpfnWndProc = window_proc;
    window_class.hInstance = instance;
    window_class.hCursor = LoadCursorW(NULL, IDC_ARROW);
    window_class.lpszClassName = L"HelloWorld";
    if (!RegisterClassW(&window_class))
        return 1;

    HWND window = CreateWindowExW(0, window_class.lpszClassName, L"Hello World", WS_OVERLAPPEDWINDOW, CW_USEDEFAULT,
                                  CW_USEDEFAULT, 360, 230, NULL, NULL, instance, NULL);
    if (!window)
        return 1;
    ShowWindow(window, show);

    MSG message;
    int result;
    while ((result = GetMessageW(&message, NULL, 0, 0)) > 0) {
        TranslateMessage(&message);
        DispatchMessageW(&message);
    }
    return result < 0 ? 1 : (int)message.wParam;
}
