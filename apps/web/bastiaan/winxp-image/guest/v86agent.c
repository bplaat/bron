/*
 * v86agent.exe - guest agent for v86, started at logon: follows the page size
 * and host clock, syncs the clipboard (text, HTML and images) and saves files
 * dropped on the page to the desktop and renews the LAN address when the page
 * asks. Started as "v86agent.exe /open <url>" it opens the link in a new
 * browser tab instead (the http and https handler).
 */
#define WIN32_LEAN_AND_MEAN
#include <windows.h>
#include <winioctl.h>
#include <shlobj.h>
#include <commctrl.h>
#include <iphlpapi.h>
#include <string.h>
#include "v86.h"

#define CHUNK_SIZE (64 * 1024)
#define POLL_MS 30
#define RESIZE_EVERY 8 /* polls */
#define CLOCK_EVERY 300 /* polls */

static HANDLE device = INVALID_HANDLE_VALUE;
static HWND next_viewer;
static ULONG last_size;
static ULONG packet[2 + CHUNK_SIZE / sizeof(ULONG)];

static HANDLE file = INVALID_HANDLE_VALUE;
static WCHAR file_path[MAX_PATH];
static BOOL file_failed;

static BOOL open_device(void)
{
    if (device == INVALID_HANDLE_VALUE)
        device = CreateFileA(V86_USER_PATH, GENERIC_READ | GENERIC_WRITE, 0, NULL, OPEN_EXISTING, 0, NULL);
    return device != INVALID_HANDLE_VALUE;
}

static BOOL ioctl(DWORD code, void *in, DWORD in_len, void *out, DWORD out_len, DWORD *got)
{
    DWORD dummy;
    return DeviceIoControl(device, code, in, in_len, out, out_len, got ? got : &dummy, NULL);
}

static void send_message(ULONG type, const void *data, ULONG len)
{
    ULONG *buf = HeapAlloc(GetProcessHeap(), 0, sizeof(ULONG) + len);
    if (!buf) return;
    buf[0] = type;
    CopyMemory(buf + 1, data, len);
    ioctl(IOCTL_V86_SEND, buf, sizeof(ULONG) + len, NULL, 0, NULL);
    HeapFree(GetProcessHeap(), 0, buf);
}

static void log_message(const WCHAR *format, ...)
{
    WCHAR text[512];
    va_list args;
    va_start(args, format);
    int len = wvsprintfW(text, format, args);
    va_end(args);
    send_message(V86_MSG_LOG, text, len * sizeof(WCHAR));
}

/* Display: follow the size the page asks for */
static DEVMODEW find_mode(DWORD w, DWORD h)
{
    /* Mode 0 makes the system re-read the driver's mode list, which offers the host size */
    DEVMODEW dm = {.dmSize = sizeof(dm)}, best = {0};
    for (DWORD i = 0; EnumDisplaySettingsW(NULL, i, &dm); i++) {
        if (dm.dmBitsPerPel < 16 || dm.dmPelsWidth > w || dm.dmPelsHeight > h) continue;
        if (dm.dmPelsWidth * dm.dmPelsHeight > best.dmPelsWidth * best.dmPelsHeight ||
            (dm.dmPelsWidth * dm.dmPelsHeight == best.dmPelsWidth * best.dmPelsHeight &&
             dm.dmBitsPerPel > best.dmBitsPerPel))
            best = dm;
    }
    return best;
}

static void follow_size(void)
{
    ULONG size = 0;
    if (!ioctl(IOCTL_V86_GETSIZE, NULL, 0, &size, sizeof(size), NULL) || !size || size == last_size) return;
    /* Fullscreen games switch to 8 or 16 bit modes, leave those alone until the desktop is back */
    HDC screen = GetDC(NULL);
    int bpp = GetDeviceCaps(screen, BITSPIXEL);
    ReleaseDC(NULL, screen);
    if (bpp != 32) return;
    last_size = size;
    DWORD w = size >> 16, h = size & 0xFFFF;
    if ((DWORD)GetSystemMetrics(SM_CXSCREEN) == w && (DWORD)GetSystemMetrics(SM_CYSCREEN) == h) return;

    /* The refreshed list only shows up on a later enumeration */
    DEVMODEW best;
    for (int i = 0; i < 3; i++) {
        best = find_mode(w, h);
        if (best.dmPelsWidth == w && best.dmPelsHeight == h) break;
    }
    if (!best.dmPelsWidth) return;
    best.dmFields = DM_PELSWIDTH | DM_PELSHEIGHT | DM_BITSPERPEL | DM_DISPLAYFREQUENCY;
    LONG result = ChangeDisplaySettingsW(&best, CDS_UPDATEREGISTRY);
    if (result != DISP_CHANGE_SUCCESSFUL) {
        log_message(L"resize to %ux%u failed: %d", w, h, result);
        return;
    }
    /* Bring desktop icons back into view */
    HWND icons = FindWindowExW(FindWindowExW(FindWindowW(L"Progman", NULL), NULL, L"SHELLDLL_DefView", NULL), NULL,
                               L"SysListView32", NULL);
    if (icons) SendMessageW(icons, LVM_ARRANGE, LVA_DEFAULT, 0);
}

/* Clock: follow the host (a restored snapshot would otherwise show its own time) */
static void enable_privilege(const WCHAR *name)
{
    HANDLE token;
    TOKEN_PRIVILEGES tp = {.PrivilegeCount = 1};
    if (!OpenProcessToken(GetCurrentProcess(), TOKEN_ADJUST_PRIVILEGES, &token)) return;
    if (LookupPrivilegeValueW(NULL, name, &tp.Privileges[0].Luid)) {
        tp.Privileges[0].Attributes = SE_PRIVILEGE_ENABLED;
        AdjustTokenPrivileges(token, FALSE, &tp, 0, NULL, NULL);
    }
    CloseHandle(token);
}

static void follow_clock(void)
{
    ULONG now[3];
    if (!ioctl(IOCTL_V86_GETTIME, NULL, 0, now, sizeof(now), NULL)) return;

    TIME_ZONE_INFORMATION tz;
    LONG bias = -(LONG)now[2];
    if (GetTimeZoneInformation(&tz) == TIME_ZONE_ID_INVALID || tz.Bias != bias || tz.DaylightBias || tz.StandardBias) {
        ZeroMemory(&tz, sizeof(tz));
        tz.Bias = bias;
        lstrcpyW(tz.StandardName, L"Host Time");
        lstrcpyW(tz.DaylightName, L"Host Time");
        SetTimeZoneInformation(&tz);
    }

    /* Unix seconds to FILETIME (100 ns ticks since 1601) */
    ULONGLONG host = ((ULONGLONG)now[0] + 11644473600ULL) * 10000000ULL + (ULONGLONG)now[1] * 10;
    FILETIME ft;
    GetSystemTimeAsFileTime(&ft);
    ULONGLONG guest = ((ULONGLONG)ft.dwHighDateTime << 32) | ft.dwLowDateTime;
    if ((guest > host ? guest - host : host - guest) < 20000000ULL) return; /* within 2 s */
    ft.dwLowDateTime = (DWORD)host;
    ft.dwHighDateTime = (DWORD)(host >> 32);
    SYSTEMTIME st;
    if (FileTimeToSystemTime(&ft, &st)) SetSystemTime(&st);
}

/* Clipboard: text, HTML and images, each format travels in chunks */
typedef struct {
    BYTE *data[V86_CLIP_COUNT];
    ULONG size[V86_CLIP_COUNT];
} CLIP;

static const char html_header[] =
    "Version:0.9\r\nStartHTML:%010lu\r\nEndHTML:%010lu\r\nStartFragment:%010lu\r\nEndFragment:%010lu\r\n";
static const char html_prefix[] = "<html><body>\r\n<!--StartFragment-->";
static const char html_suffix[] = "<!--EndFragment-->\r\n</body></html>";

static UINT cf_html;
static ULONG last_clip_hash;
static CLIP clip_in;
static ULONG clip_format = V86_CLIP_COUNT, clip_got;

static BYTE *clip_alloc(CLIP *clip, ULONG format, ULONG size)
{
    if (clip->data[format]) HeapFree(GetProcessHeap(), 0, clip->data[format]);
    clip->size[format] = size;
    return clip->data[format] = HeapAlloc(GetProcessHeap(), 0, max(size, 1));
}

static void clip_free(CLIP *clip)
{
    for (int i = 0; i < V86_CLIP_COUNT; i++)
        if (clip->data[i]) HeapFree(GetProcessHeap(), 0, clip->data[i]);
    ZeroMemory(clip, sizeof(*clip));
}

/* FNV-1a over all formats, so a clipboard the host just set is not echoed back */
static ULONG clip_hash(const CLIP *clip)
{
    ULONG hash = 2166136261u;
    for (ULONG i = 0; i < V86_CLIP_COUNT; i++) {
        const BYTE *p = clip->data[i];
        ULONG n = clip->size[i];
        if (!p) continue;
        hash = (hash ^ i ^ n) * 16777619u;
        for (; n >= sizeof(ULONG); p += sizeof(ULONG), n -= sizeof(ULONG)) hash = (hash ^ *(const ULONG *)p) * 16777619u;
        for (; n; p++, n--) hash = (hash ^ *p) * 16777619u;
    }
    return hash;
}

static void read_text(CLIP *clip)
{
    HANDLE mem = GetClipboardData(CF_UNICODETEXT);
    const WCHAR *text = mem ? GlobalLock(mem) : NULL;
    if (!text) return;
    ULONG size = lstrlenW(text) * sizeof(WCHAR);
    BYTE *dst = clip_alloc(clip, V86_CLIP_TEXT, size);
    if (dst) CopyMemory(dst, text, size);
    GlobalUnlock(mem);
}

/* CF_HTML is an ASCII header with the byte offsets of the fragment in the UTF-8 data that follows */
static ULONG html_offset(const char *header, const char *key)
{
    const char *p = strstr(header, key);
    ULONG value = 0;
    if (p)
        for (p += lstrlenA(key); *p >= '0' && *p <= '9'; p++) value = value * 10 + *p - '0';
    return value;
}

static void read_html(CLIP *clip)
{
    HANDLE mem = cf_html ? GetClipboardData(cf_html) : NULL;
    const char *html = mem ? GlobalLock(mem) : NULL;
    if (!html) return;
    SIZE_T avail = GlobalSize(mem);
    char header[512];
    lstrcpynA(header, html, (int)min(avail, sizeof(header) - 1) + 1);
    ULONG start = html_offset(header, "StartFragment:"), end = html_offset(header, "EndFragment:");
    if (start && start <= end && end <= avail) {
        BYTE *dst = clip_alloc(clip, V86_CLIP_HTML, end - start);
        if (dst) CopyMemory(dst, html + start, end - start);
    }
    GlobalUnlock(mem);
}

/* Any CF_DIB (palette, 16/24/32 bit, RLE) is drawn into a 32 bit top-down DIB section */
static void dib_to_image(CLIP *clip, const BITMAPINFOHEADER *bi, SIZE_T avail)
{
    if (avail < sizeof(*bi) || bi->biSize < sizeof(*bi)) return;
    LONG w = bi->biWidth, h = bi->biHeight < 0 ? -bi->biHeight : bi->biHeight;
    if (w <= 0 || h <= 0 || (ULONGLONG)w * h * 4 > V86_CLIP_MAX - 8) return;
    DWORD colors = bi->biClrUsed ? bi->biClrUsed : bi->biBitCount <= 8 ? 1u << bi->biBitCount : 0;
    SIZE_T offset = bi->biSize + colors * sizeof(RGBQUAD) +
                    (bi->biCompression == BI_BITFIELDS && bi->biSize == sizeof(*bi) ? 3 * sizeof(DWORD) : 0);
    if (offset > avail) return;
    if (bi->biCompression == BI_RGB || bi->biCompression == BI_BITFIELDS) {
        if (((SIZE_T)w * bi->biBitCount + 31) / 32 * 4 * h > avail - offset) return;
    } else if (bi->biCompression != BI_RLE8 && bi->biCompression != BI_RLE4) {
        return;
    }

    BITMAPINFO out = {{sizeof(BITMAPINFOHEADER), w, -h, 1, 32, BI_RGB, 0, 0, 0, 0, 0}, {{0}}};
    void *pixels;
    HBITMAP dib = CreateDIBSection(NULL, &out, DIB_RGB_COLORS, &pixels, NULL, 0);
    if (!dib) return;
    HDC dc = CreateCompatibleDC(NULL);
    HGDIOBJ old = SelectObject(dc, dib);
    if (SetDIBitsToDevice(dc, 0, 0, w, h, 0, 0, 0, h, (const BYTE *)bi + offset, (const BITMAPINFO *)bi,
                          DIB_RGB_COLORS) > 0) {
        GdiFlush();
        ULONG count = (ULONG)w * h, *dst = (ULONG *)clip_alloc(clip, V86_CLIP_IMAGE, 8 + count * 4);
        const ULONG *src = pixels;
        if (dst) {
            dst[0] = w;
            dst[1] = h;
            for (ULONG i = 0; i < count; i++) dst[2 + i] = src[i] & 0xFFFFFF;
        }
    }
    SelectObject(dc, old);
    DeleteDC(dc);
    DeleteObject(dib);
}

static void read_image(CLIP *clip)
{
    HANDLE mem = GetClipboardData(CF_DIB);
    const BITMAPINFOHEADER *bi = mem ? GlobalLock(mem) : NULL;
    if (!bi) return;
    dib_to_image(clip, bi, GlobalSize(mem));
    GlobalUnlock(mem);
}

static void clipboard_changed(HWND hwnd)
{
    if (!OpenClipboard(hwnd)) return;
    CLIP clip = {0};
    read_text(&clip);
    read_html(&clip);
    read_image(&clip);
    CloseClipboard();

    ULONG hash = clip_hash(&clip);
    if (hash != last_clip_hash) {
        last_clip_hash = hash;
        for (ULONG i = 0; i < V86_CLIP_COUNT; i++) {
            if (!clip.data[i]) continue;
            ULONG head[2] = {i, clip.size[i]};
            send_message(V86_MSG_CLIP_FORMAT, head, sizeof(head));
            for (ULONG off = 0; off < clip.size[i]; off += CHUNK_SIZE)
                send_message(V86_MSG_CLIP_DATA, clip.data[i] + off, min(CHUNK_SIZE, clip.size[i] - off));
        }
        send_message(V86_MSG_CLIP_END, NULL, 0);
    }
    clip_free(&clip);
}

static void *clip_global(HGLOBAL *mem, SIZE_T size)
{
    void *ptr = (*mem = GlobalAlloc(GMEM_MOVEABLE, size)) ? GlobalLock(*mem) : NULL;
    if (!ptr && *mem) GlobalFree(*mem);
    return ptr;
}

static void clip_put(UINT format, HGLOBAL mem)
{
    GlobalUnlock(mem);
    if (!SetClipboardData(format, mem)) GlobalFree(mem);
}

static void put_text(const BYTE *data, ULONG size)
{
    HGLOBAL mem;
    BYTE *dst = clip_global(&mem, (size &= ~1u) + sizeof(WCHAR));
    if (!dst) return;
    CopyMemory(dst, data, size);
    dst[size] = dst[size + 1] = 0;
    clip_put(CF_UNICODETEXT, mem);
}

static void put_html(const BYTE *data, ULONG size)
{
    char header[sizeof(html_header) + 32];
    ULONG start_html = wsprintfA(header, html_header, 0ul, 0ul, 0ul, 0ul);
    ULONG start_fragment = start_html + sizeof(html_prefix) - 1, end_fragment = start_fragment + size;
    ULONG end_html = end_fragment + sizeof(html_suffix) - 1;
    HGLOBAL mem;
    char *dst = clip_global(&mem, end_html + 1);
    if (!dst) return;
    wsprintfA(dst, html_header, start_html, end_html, start_fragment, end_fragment);
    CopyMemory(dst + start_html, html_prefix, sizeof(html_prefix) - 1);
    CopyMemory(dst + start_fragment, data, size);
    CopyMemory(dst + end_fragment, html_suffix, sizeof(html_suffix));
    clip_put(cf_html, mem);
}

/* A bottom-up CF_DIB, the layout old apps expect */
static void put_image(const BYTE *data, ULONG size)
{
    if (size < 8) return;
    ULONG w = ((const ULONG *)data)[0], h = ((const ULONG *)data)[1], stride = w * 4;
    if (!w || !h || (ULONGLONG)stride * h > size - 8) return;
    HGLOBAL mem;
    BITMAPINFOHEADER *bi = clip_global(&mem, sizeof(*bi) + stride * h);
    if (!bi) return;
    *bi = (BITMAPINFOHEADER){sizeof(*bi), w, h, 1, 32, BI_RGB, stride * h, 0, 0, 0, 0};
    for (ULONG y = 0; y < h; y++) CopyMemory((BYTE *)(bi + 1) + (h - 1 - y) * stride, data + 8 + y * stride, stride);
    clip_put(CF_DIB, mem);
}

static void clip_begin(const ULONG *data, ULONG len)
{
    clip_format = V86_CLIP_COUNT;
    clip_got = 0;
    if (len < 2 * sizeof(ULONG) || data[0] >= V86_CLIP_COUNT || data[1] > V86_CLIP_MAX) return;
    if (clip_alloc(&clip_in, data[0], data[1])) clip_format = data[0];
}

static void clip_data(const BYTE *data, ULONG len)
{
    if (clip_format >= V86_CLIP_COUNT) return;
    len = min(len, clip_in.size[clip_format] - clip_got);
    CopyMemory(clip_in.data[clip_format] + clip_got, data, len);
    clip_got += len;
}

static void clip_end(HWND hwnd)
{
    last_clip_hash = clip_hash(&clip_in);
    if (OpenClipboard(hwnd)) {
        EmptyClipboard();
        if (clip_in.data[V86_CLIP_TEXT]) put_text(clip_in.data[V86_CLIP_TEXT], clip_in.size[V86_CLIP_TEXT]);
        if (clip_in.data[V86_CLIP_HTML] && cf_html) put_html(clip_in.data[V86_CLIP_HTML], clip_in.size[V86_CLIP_HTML]);
        if (clip_in.data[V86_CLIP_IMAGE]) put_image(clip_in.data[V86_CLIP_IMAGE], clip_in.size[V86_CLIP_IMAGE]);
        CloseClipboard();
    }
    clip_free(&clip_in);
    clip_format = V86_CLIP_COUNT;
}

/* Dropped files land on the desktop */
static DWORD WINAPI error_box(LPVOID text)
{
    MessageBoxW(NULL, text, L"v86", MB_ICONWARNING | MB_SYSTEMMODAL);
    return 0;
}

static void file_begin(const BYTE *data, ULONG len)
{
    if (file != INVALID_HANDLE_VALUE) CloseHandle(file);
    file = INVALID_HANDLE_VALUE;
    file_failed = FALSE;
    if (len < sizeof(ULONG)) return;

    /* The name is UTF-16 like FAT32 long names, so any character survives, only ones Windows forbids are dropped */
    WCHAR name[MAX_PATH];
    ULONG chars = (len - sizeof(ULONG)) / sizeof(WCHAR), n = 0;
    const WCHAR *src = (const WCHAR *)(data + sizeof(ULONG));
    for (ULONG i = 0; i < chars && n < MAX_PATH - 1; i++)
        if (src[i] >= 32 && !wcschr(L"\\/:*?\"<>|", src[i])) name[n++] = src[i];
    while (n && (name[n - 1] == L'.' || name[n - 1] == L' ')) n--; /* Windows strips these */
    name[n] = 0;
    if (!n) lstrcpyW(name, L"file");

    WCHAR desktop[MAX_PATH];
    if (!SHGetSpecialFolderPathW(NULL, desktop, CSIDL_DESKTOPDIRECTORY, TRUE)) return;
    WCHAR *dot = wcsrchr(name, L'.');
    WCHAR ext[MAX_PATH] = L"";
    if (dot && dot != name && lstrlenW(dot) <= 16) {
        lstrcpyW(ext, dot);
        *dot = 0;
    }
    /* Keep the path short enough: shorten the name before the extension, never inside a surrogate pair */
    int keep = 120 - lstrlenW(ext);
    if (lstrlenW(name) > keep) {
        if (name[keep - 1] >= 0xD800 && name[keep - 1] <= 0xDBFF) keep--;
        name[keep] = 0;
    }
    wsprintfW(file_path, L"%s\\%s%s", desktop, name, ext);
    for (int i = 2; GetFileAttributesW(file_path) != INVALID_FILE_ATTRIBUTES && i < 1000; i++)
        wsprintfW(file_path, L"%s\\%s (%d)%s", desktop, name, i, ext);
    file = CreateFileW(file_path, GENERIC_WRITE, 0, NULL, CREATE_NEW, FILE_ATTRIBUTE_NORMAL, NULL);
    file_failed = file == INVALID_HANDLE_VALUE;
}

static void file_data(const BYTE *data, ULONG len)
{
    DWORD written;
    if (file == INVALID_HANDLE_VALUE || file_failed) return;
    if (!WriteFile(file, data, len, &written, NULL) || written != len) file_failed = TRUE;
}

static void file_end(void)
{
    if (file == INVALID_HANDLE_VALUE) {
        if (file_failed) CreateThread(NULL, 0, error_box, L"Could not save the dropped file.", 0, NULL);
        return;
    }
    CloseHandle(file);
    file = INVALID_HANDLE_VALUE;
    if (file_failed) {
        DeleteFileW(file_path);
        CreateThread(NULL, 0, error_box, L"Not enough disk space for the dropped file.", 0, NULL);
        return;
    }
    SHChangeNotify(SHCNE_CREATE, SHCNF_PATHW, file_path, NULL);
}

/* Every visitor runs the same snapshot named WINXP: NetBIOS off on all adapters so the shared LAN has no duplicate
   names, then a DHCP lease from the relay. In a thread, the renew waits for the relay's answer. */
static DWORD WINAPI renew_network(LPVOID unused)
{
    (void)unused;
    HKEY interfaces;
    if (RegOpenKeyExW(HKEY_LOCAL_MACHINE, L"SYSTEM\\CurrentControlSet\\Services\\NetBT\\Parameters\\Interfaces", 0,
                      KEY_READ | KEY_SET_VALUE, &interfaces) == ERROR_SUCCESS) {
        WCHAR name[MAX_PATH];
        DWORD disabled = 2;
        for (DWORD i = 0, n = MAX_PATH; RegEnumKeyExW(interfaces, i, name, &n, NULL, NULL, NULL, NULL) == ERROR_SUCCESS;
             i++, n = MAX_PATH) {
            HKEY h;
            if (RegOpenKeyExW(interfaces, name, 0, KEY_SET_VALUE, &h) != ERROR_SUCCESS) continue;
            RegSetValueExW(h, L"NetbiosOptions", 0, REG_DWORD, (const BYTE *)&disabled, sizeof(disabled));
            RegCloseKey(h);
        }
        RegCloseKey(interfaces);
    }

    ULONG size = 0;
    if (GetInterfaceInfo(NULL, &size) != ERROR_INSUFFICIENT_BUFFER) return 0;
    IP_INTERFACE_INFO *info = HeapAlloc(GetProcessHeap(), 0, size);
    if (!info) return 0;
    if (GetInterfaceInfo(info, &size) == NO_ERROR) {
        for (LONG i = 0; i < info->NumAdapters; i++) IpRenewAddress(&info->Adapter[i]);
    }
    HeapFree(GetProcessHeap(), 0, info);
    return 0;
}

static void poll(HWND hwnd)
{
    static int tick;
    if (!open_device()) return;
    if (tick % RESIZE_EVERY == 0) follow_size();
    if (tick % CLOCK_EVERY == 0) follow_clock();
    tick++;

    for (int i = 0; i < 64; i++) {
        DWORD got = 0;
        if (!ioctl(IOCTL_V86_RECV, NULL, 0, packet, sizeof(packet), &got) || got < 2 * sizeof(ULONG)) return;
        ULONG type = packet[0], len = min(packet[1], got - 2 * sizeof(ULONG));
        const BYTE *data = (const BYTE *)(packet + 2);
        switch (type) {
        case V86_MSG_NONE: return;
        case V86_MSG_CLIP_FORMAT: clip_begin((const ULONG *)data, len); break;
        case V86_MSG_CLIP_DATA: clip_data(data, len); break;
        case V86_MSG_CLIP_END: clip_end(hwnd); break;
        case V86_MSG_FILE_BEGIN: file_begin(data, len); break;
        case V86_MSG_FILE_DATA: file_data(data, len); break;
        case V86_MSG_FILE_END: file_end(); break;
        case V86_MSG_SHUTDOWN: ExitWindowsEx(EWX_POWEROFF | EWX_FORCE, 0); break;
        case V86_MSG_NET_RENEW: CloseHandle(CreateThread(NULL, 0, renew_network, NULL, 0, NULL)); break;
        }
    }
}

static LRESULT CALLBACK wndproc(HWND hwnd, UINT msg, WPARAM wp, LPARAM lp)
{
    switch (msg) {
    case WM_TIMER:
        poll(hwnd);
        return 0;
    case WM_DRAWCLIPBOARD:
        if (open_device()) clipboard_changed(hwnd);
        if (next_viewer) SendMessageW(next_viewer, msg, wp, lp);
        return 0;
    case WM_CHANGECBCHAIN:
        if ((HWND)wp == next_viewer) next_viewer = (HWND)lp;
        else if (next_viewer) SendMessageW(next_viewer, msg, wp, lp);
        return 0;
    case WM_DESTROY:
        ChangeClipboardChain(hwnd, next_viewer);
        PostQuitMessage(0);
        return 0;
    }
    return DefWindowProcW(hwnd, msg, wp, lp);
}

int WINAPI wWinMain(HINSTANCE inst, HINSTANCE prev, LPWSTR cmd, int show)
{
    (void)prev;
    (void)show;
    if (!wcsncmp(cmd, L"/open ", 6)) {
        WCHAR *url = cmd + 6;
        if (*url == L'"') url++;
        WCHAR *end = url + lstrlenW(url);
        if (end > url && end[-1] == L'"') *--end = 0;
        if (open_device()) send_message(V86_MSG_OPEN_URL, url, (end - url) * sizeof(WCHAR));
        return 0;
    }
    CreateMutexW(NULL, TRUE, L"v86agent");
    if (GetLastError() == ERROR_ALREADY_EXISTS) return 0;
    CoInitialize(NULL);
    enable_privilege(SE_SYSTEMTIME_NAME);
    enable_privilege(SE_SHUTDOWN_NAME);

    WNDCLASSW wc = {.lpfnWndProc = wndproc, .hInstance = inst, .lpszClassName = L"v86agent"};
    RegisterClassW(&wc);
    HWND hwnd = CreateWindowW(L"v86agent", L"v86agent", WS_POPUP, 0, 0, 0, 0, NULL, NULL, inst, NULL);
    cf_html = RegisterClipboardFormatW(L"HTML Format");
    next_viewer = SetClipboardViewer(hwnd);
    SetTimer(hwnd, 1, POLL_MS, NULL);

    MSG msg;
    while (GetMessageW(&msg, NULL, 0, 0) > 0) DispatchMessageW(&msg);
    return 0;
}
