// Bastiaan van der Plaat's personal website as a native Windows XP program (Win32 + GDI only)
#define WIN32_LEAN_AND_MEAN
#define _WIN32_WINNT 0x0501
#include <windows.h>
#include <shellapi.h>
#include <objbase.h>
#include <stdlib.h>

#define ID_ICON 1
#define ID_ATLAS 2
#define ID_TIMER 1

#define WINDOW_TITLE L"Bastiaan van der Plaat"
#define WHITE RGB(255, 255, 255)
#define LINK_COLOR RGB(51, 51, 51) // black at 80% opacity over white
#define MAX_BOXES 256
#define MAX_PARTICLES 200
#define MIN_CLIENT_W 400 // the page is laid out at least 350 wide, see layout()
#define MIN_CLIENT_H 300

// Images in atlas.bmp: the 48x48 avatar, then 24x24 icons in the Makefile ICONS order
enum {
    IMG_NONE,
    IMG_AVATAR,
    IMG_BASSIEMUSIC,
    IMG_STREPEN,
    IMG_BIBLE,
    IMG_COINLIST,
    IMG_REDSQUARE,
    IMG_WARQUEST,
    IMG_BLOG,
    IMG_GITHUB,
    IMG_CODEPEN,
    IMG_LINKEDIN,
    IMG_FACEBOOK,
    IMG_INSTAGRAM,
    IMG_REDDIT,
    IMG_X,
    IMG_YOUTUBE,
    IMG_SOUNDCLOUD,
};
enum { FONT_NORMAL, FONT_BOLD, FONT_HEADING, FONT_COUNT };

// Content
typedef struct {
    const wchar_t *text;
    const wchar_t *url;
    int img;
    int dy;
} Link;

#define LINK(text, url) {text, url, IMG_NONE, 0}

static wchar_t age_text[32];

static const Link facts[] = {
    LINK(L"Male human being", L"https://en.wikipedia.org/wiki/Man"),
    LINK(age_text, L"https://days.to/until/1-july"),
    LINK(L"Dutch", L"https://en.wikipedia.org/wiki/Netherlands"),
    LINK(L"Protestant Christian", L"https://www.oostpoort.net/"),
    LINK(L"Programming is my hobby", L"https://github.com/bplaat"),
    LINK(L"Volunteer by Scouting Die Kantanky's in Gouda, Netherlands", L"https://www.diekantankys.nl/"),
    LINK(L"Member of Christen-Democratisch App\u00e8l (CDA)", L"https://cda.nl/"),
};

static const Link sites[] = {
    LINK(L"plaatsoft.nl", L"https://www.plaatsoft.nl/"),
    LINK(L"Bron des Codes", L"https://github.com/bplaat/bron"),
    LINK(L"Strepen Systeem", L"https://stam.diekantankys.nl/"),
    LINK(L"Neva Processor", L"https://neva-processor.bplaat.nl/"),
    LINK(L"Coinlist", L"https://coinlist.bplaat.nl/"),
    LINK(L"BassieMusic", L"https://github.com/bplaat/bassiemusic"),
    LINK(L"WarQuest", L"https://warquest.plaatsoft.nl/"),
    LINK(L"Battle of Teripa", L"https://teripa.bplaat.nl/"),
    LINK(L"PlaatWorld", L"https://plaatworld.bplaat.nl/"),
    LINK(L"PlaatWorld 3D", L"https://plaatworld3d.bplaat.nl/"),
    LINK(L"Welpen Games", L"https://welpen.bplaat.nl/"),
    LINK(L"PlaatGames", L"https://plaatgames.bplaat.nl/"),
    LINK(L"Ru\u00efnes game", L"https://bplaat.itch.io/ruines"),
    LINK(L"PlaatCraft", L"https://github.com/bplaat/bron/tree/master/games/plaatcraft"),
};

static const Link apps[] = {
    {L"BassieMusic", L"https://play.google.com/store/apps/details?id=nl.plaatsoft.bassiemusic", IMG_BASSIEMUSIC, 1},
    {L"Strepen", L"https://play.google.com/store/apps/details?id=nl.plaatsoft.strepen", IMG_STREPEN, 0},
    {L"Bible", L"https://github.com/bplaat/android-apps/tree/master/bin/bible", IMG_BIBLE, 0},
    {L"CoinList", L"https://github.com/bplaat/android-apps/tree/master/bin/coinlist", IMG_COINLIST, 0},
    {L"RedSquare", L"https://www.plaatsoft.nl/android/redsquare/", IMG_REDSQUARE, 0},
    {L"WarQuest", L"https://www.plaatsoft.nl/android/warquest/", IMG_WARQUEST, 2},
};

static const Link socials[] = {
    {L"Blog", L"https://blog.bplaat.nl/", IMG_BLOG, 0},
    {L"GitHub", L"https://github.com/bplaat", IMG_GITHUB, 0},
    {L"CodePen", L"https://codepen.io/bplaat", IMG_CODEPEN, 0},
    {L"Linkedin", L"https://www.linkedin.com/in/bplaat/", IMG_LINKEDIN, 0},
    {L"Facebook", L"https://www.facebook.com/bplaat", IMG_FACEBOOK, 0},
    {L"Instagram", L"https://www.instagram.com/bplaat2/", IMG_INSTAGRAM, 0},
    {L"Reddit", L"https://reddit.com/u/bplaat", IMG_REDDIT, 0},
    {L"X/Twitter", L"https://x.com/bplaat2", IMG_X, 0},
    {L"YouTube", L"https://www.youtube.com/@bplaat", IMG_YOUTUBE, 0},
    {L"SoundCloud", L"https://soundcloud.com/bplaat", IMG_SOUNDCLOUD, 0},
};

// Headings and paragraphs have a 12px vertical margin, the lists have none
typedef enum { BLOCK_HEADING, BLOCK_TEXT, BLOCK_FACTS, BLOCK_SITES, BLOCK_ICONS } BlockType;

typedef struct {
    BlockType type;
    const wchar_t *text;
    const wchar_t *bold;
    const Link *links;
    int count;
} Block;

#define LINKS(array, start, n) .links = (array) + (start), .count = (n)

static const Block blocks[] = {
    {BLOCK_HEADING, .text = L"Bastiaan van der Plaat"},
    {BLOCK_TEXT, .text = L"Some facts about me:"},
    {BLOCK_FACTS, LINKS(facts, 0, ARRAYSIZE(facts))},
    {BLOCK_TEXT, .text = L"Some sites and games I've made:"},
    {BLOCK_SITES, LINKS(sites, 0, ARRAYSIZE(sites))},
    {BLOCK_TEXT, .text = L"Some Android apps I've made:"},
    {BLOCK_ICONS, LINKS(apps, 0, 3)},
    {BLOCK_ICONS, LINKS(apps, 3, 3)},
    {BLOCK_TEXT, .text = L"You can also find me on:"},
    {BLOCK_ICONS, LINKS(socials, 0, 4)},
    {BLOCK_ICONS, LINKS(socials, 4, 3)},
    {BLOCK_ICONS, LINKS(socials, 7, 3)},
    {BLOCK_TEXT, .text = L"And my nickname around the internet is", .bold = L"@bplaat"},
};

// Layout box: [pad][image][gap][text][pad], positioned in content coordinates
typedef struct {
    int x, y, w, h;
    int font;
    const wchar_t *text;
    int len;
    int img;
    int dy;
    int pad, gap;
    const Link *link;
} Box;

typedef struct {
    float x, y, vx, vy, size;
    COLORREF color;
    RECT rect, dirty;
} Particle;

static struct {
    HWND hwnd;
    HDC static_dc, back_dc, image_dc;
    HBITMAP static_bmp, back_bmp, atlas_bmp;
    HFONT fonts[FONT_COUNT];
    int font_h[FONT_COUNT], space_w[FONT_COUNT];
    int font_size;
    int width, height, bmp_width, bmp_height;
    int content_h, line_h, scroll, offset, wheel;
    Box boxes[MAX_BOXES];
    int box_count;
    const Link *hover, *pressed;
    Particle particles[MAX_PARTICLES];
    int particle_count;
    BOOL active, paused, user_placed;
    UINT interval;
    DWORD tick;
    RECT size_move_rect;
} g;

static struct {
    int left, width, line_h, x, y, line_start;
} flow;

static int clamp(int v, int lo, int hi) { return v < lo ? lo : v > hi ? hi : v; }

static int img_size(int img) { return img == IMG_AVATAR ? 48 : 24; }

static int img_x(int img) { return img == IMG_AVATAR ? 0 : 48 + (img - IMG_BASSIEMUSIC) * 24; }

static int text_width(int font, const wchar_t *text, int len) {
    SIZE size;
    SelectObject(g.static_dc, g.fonts[font]);
    GetTextExtentPoint32W(g.static_dc, text, len, &size);
    return size.cx;
}

static void delete_fonts(void) {
    SelectObject(g.static_dc, GetStockObject(SYSTEM_FONT));
    for (int i = 0; i < FONT_COUNT; i++) DeleteObject(g.fonts[i]);
}

static void create_fonts(int size) {
    static const struct {
        int scale, weight;
    } styles[FONT_COUNT] = {{1, FW_NORMAL}, {1, FW_BOLD}, {2, FW_BOLD}};
    delete_fonts();
    g.font_size = size;
    for (int i = 0; i < FONT_COUNT; i++) {
        TEXTMETRICW tm;
        g.fonts[i] = CreateFontW(-size * styles[i].scale, 0, 0, 0, styles[i].weight, FALSE, FALSE, FALSE,
                                 DEFAULT_CHARSET, OUT_TT_PRECIS, CLIP_DEFAULT_PRECIS, CLEARTYPE_QUALITY,
                                 VARIABLE_PITCH | FF_ROMAN, L"Georgia");
        SelectObject(g.static_dc, g.fonts[i]);
        GetTextMetricsW(g.static_dc, &tm);
        g.font_h[i] = tm.tmHeight;
        g.space_w[i] = text_width(i, L" ", 1);
    }
}

// Inline flow: boxes are placed left to right, wrapped and each line centered
static void flow_begin(int left, int width, int y, int line_h) {
    flow.left = left;
    flow.width = width;
    flow.line_h = line_h;
    flow.x = 0;
    flow.y = y;
    flow.line_start = g.box_count;
}

static void flow_line(void) {
    if (g.box_count == flow.line_start) return;
    int shift = flow.left + (flow.width - flow.x) / 2;
    for (int i = flow.line_start; i < g.box_count; i++) g.boxes[i].x += shift;
    flow.y += flow.line_h;
    flow.x = 0;
    flow.line_start = g.box_count;
}

// The glue is the spacing before the box, it is dropped at the start of a line
static void flow_box(int glue, Box box) {
    if (g.box_count == MAX_BOXES) return;
    box.w = 2 * box.pad + (box.img ? img_size(box.img) + box.gap : 0) +
            (box.len ? text_width(box.font, box.text, box.len) : 0);
    if (flow.x > 0 && flow.x + glue + box.w > flow.width) flow_line();
    if (flow.x > 0) flow.x += glue;
    box.x = flow.x;
    box.y = flow.y;
    box.h = flow.line_h;
    g.boxes[g.box_count++] = box;
    flow.x += box.w;
}

static void flow_words(int glue, int font, const wchar_t *text, const Link *link) {
    while (*text) {
        const wchar_t *space = wcschr(text, L' ');
        int len = space ? space - text : (int)wcslen(text);
        flow_box(glue, (Box){.font = font, .text = text, .len = len, .link = link});
        glue = g.space_w[font];
        text += len + (space != NULL);
    }
}

static void layout(int full_width) {
    int font_size = full_width >= 768 ? 20 : 16;
    if (font_size != g.font_size) create_fonts(font_size);
    g.line_h = font_size * 3 / 2;

    SYSTEMTIME now;
    GetLocalTime(&now);
    wsprintfW(age_text, L"%d years old", now.wYear - 2002 - (now.wMonth < 7));

    int container_w = clamp(g.width, 350, 800);
    int container_x = g.width > container_w ? (g.width - container_w) / 2 : 0;
    int list_x = container_x + container_w * 15 / 100, list_w = container_w * 70 / 100;
    int y = 0, margin = 0;
    g.box_count = 0;
    for (const Block *block = blocks; block < blocks + ARRAYSIZE(blocks); block++) {
        int block_margin = block->type <= BLOCK_TEXT ? 12 : 0;
        y += max(margin, block_margin);
        margin = block_margin;

        if (block->type == BLOCK_SITES) {
            int column_w = min(300, container_w / 2), bottom = y, half = block->count / 2;
            for (int column = 0; column < 2; column++) {
                flow_begin(container_x + (container_w - 2 * column_w) / 2 + column * column_w, column_w, y, g.line_h);
                for (int i = column * half; i < (column + 1) * half; i++) {
                    flow_words(0, FONT_BOLD, block->links[i].text, &block->links[i]);
                    flow_line();
                }
                bottom = max(bottom, flow.y);
            }
            y = bottom;
            continue;
        }

        BOOL is_list = block->type == BLOCK_FACTS || block->type == BLOCK_ICONS;
        flow_begin(is_list ? list_x : container_x, is_list ? list_w : container_w, y,
                   block->type == BLOCK_HEADING ? font_size * 3 : g.line_h);
        switch (block->type) {
        case BLOCK_HEADING:
            flow_box(0, (Box){.img = IMG_AVATAR, .dy = -4});
            flow_words(4 + g.space_w[FONT_HEADING], FONT_HEADING, block->text, NULL);
            break;
        case BLOCK_TEXT:
            flow_words(0, FONT_NORMAL, block->text, NULL);
            if (block->bold) flow_words(g.space_w[FONT_NORMAL], FONT_BOLD, block->bold, NULL);
            break;
        case BLOCK_FACTS:
            for (int i = 0; i < block->count; i++) {
                int glue = g.space_w[FONT_NORMAL] + 6;
                if (i > 0) flow_words(glue, FONT_NORMAL, L"-", NULL);
                flow_words(i > 0 ? glue : 0, FONT_BOLD, block->links[i].text, &block->links[i]);
            }
            break;
        default:
            for (int i = 0; i < block->count; i++) {
                const Link *link = &block->links[i];
                flow_box(i > 0 ? g.space_w[FONT_NORMAL] + 12 : 0,
                         (Box){.font = FONT_BOLD, .text = link->text, .len = wcslen(link->text), .img = link->img,
                               .dy = link->dy, .pad = 4, .gap = 8, .link = link});
            }
        }
        flow_line();
        y = flow.y;
    }
    g.content_h = y + margin;
}

// Rendering: the static page goes into static_dc, back_dc holds static page + particles
static RECT box_rect(const Box *box) {
    return (RECT){box->x, box->y + g.offset, box->x + box->w, box->y + g.offset + box->h};
}

static void blit(HDC dst, HDC src, const RECT *rect) {
    if (!IsRectEmpty(rect))
        BitBlt(dst, rect->left, rect->top, rect->right - rect->left, rect->bottom - rect->top, src, rect->left,
               rect->top, SRCCOPY);
}

static void draw_box(const Box *box) {
    RECT rect = box_rect(box);
    FillRect(g.static_dc, &rect, GetStockObject(WHITE_BRUSH));
    int x = rect.left + box->pad;
    if (box->img) {
        int size = img_size(box->img);
        BitBlt(g.static_dc, x, rect.top + (box->h - size) / 2 + box->dy, size, size, g.image_dc, img_x(box->img), 0,
               SRCCOPY);
        x += size + box->gap;
    }
    if (box->len) {
        SelectObject(g.static_dc, g.fonts[box->font]);
        SetTextColor(g.static_dc, box->link && box->link != g.hover ? LINK_COLOR : 0);
        TextOutW(g.static_dc, x, rect.top + (box->h - g.font_h[box->font]) / 2, box->text, box->len);
    }
}

static float rnd(float a, float b) { return a + (b - a) * rand() / RAND_MAX; }

// Same distribution as background.js, positions are relative to the window center
static void particle_init(Particle *p, BOOL start) {
    p->x = rnd(-g.width, start ? g.width : -g.width / 2.0f);
    p->y = rnd(-g.height, start ? g.height : g.height * 2.0f);
    p->vx = rnd(0.5f, 1.5f);
    p->vy = rnd(-1.5f, -0.5f);
    p->size = rnd(0.5f, 3);
    int gray = 255 - (int)(rnd(0.2f, 0.6f) * 255);
    p->color = RGB(gray, gray, gray);
    SetRectEmpty(&p->rect);
}

// Restore the old particle rects, move and draw them behind the page, then present the touched rects
static void particles_step(float dt, HDC present_dc) {
    RECT client = {0, 0, g.width, g.height};
    for (Particle *p = g.particles; p < g.particles + g.particle_count; p++) blit(g.back_dc, g.static_dc, &p->rect);

    for (Particle *p = g.particles; p < g.particles + g.particle_count; p++) {
        if (dt > 0) {
            float vx = p->vx + rnd(-0.1f, 0.1f), vy = p->vy + rnd(-0.1f, 0.1f);
            p->vx = min(max(vx, 0.3f), 2.0f);
            p->vy = min(max(vy, -2.0f), -0.3f);
            p->x += p->vx * 30 * dt;
            p->y += p->vy * 30 * dt;
            if (p->x > g.width || p->y < -g.height) particle_init(p, FALSE);
        }

        RECT old = p->rect;
        int d = max(1, (int)(p->size * 2 + 0.5f));
        int x = (int)(p->x + g.width / 2.0f - d / 2.0f), y = (int)(p->y + g.height / 2.0f - d / 2.0f);
        RECT rect = {x, y, x + d + 1, y + d + 1}; // 1px margin: covers NULL_PEN ellipse edge variations
        IntersectRect(&p->rect, &rect, &client);
        if (!IsRectEmpty(&p->rect)) {
            SetDCBrushColor(g.back_dc, p->color);
            if (d <= 2)
                PatBlt(g.back_dc, x, y, d, d, PATCOPY);
            else
                Ellipse(g.back_dc, x, y, x + d + 1, y + d + 1);
            int w = p->rect.right - p->rect.left, h = p->rect.bottom - p->rect.top;
            TransparentBlt(g.back_dc, p->rect.left, p->rect.top, w, h, g.static_dc, p->rect.left, p->rect.top, w, h,
                           WHITE);
        }
        UnionRect(&p->dirty, &old, &p->rect);
    }

    if (present_dc)
        for (Particle *p = g.particles; p < g.particles + g.particle_count; p++) blit(present_dc, g.back_dc, &p->dirty);
}

static void render(void) {
    RECT client = {0, 0, g.width, g.height};
    g.offset = max(0, (g.height - g.content_h) / 2) - g.scroll;
    FillRect(g.static_dc, &client, GetStockObject(WHITE_BRUSH));
    for (const Box *box = g.boxes; box < g.boxes + g.box_count; box++) draw_box(box);
    blit(g.back_dc, g.static_dc, &client);
    for (Particle *p = g.particles; p < g.particles + g.particle_count; p++) SetRectEmpty(&p->rect);
    particles_step(0, NULL);
    InvalidateRect(g.hwnd, NULL, FALSE);
}

static void redraw_link(const Link *link) {
    if (!link) return;
    for (const Box *box = g.boxes; box < g.boxes + g.box_count; box++) {
        if (box->link != link) continue;
        RECT rect = box_rect(box);
        draw_box(box);
        blit(g.back_dc, g.static_dc, &rect);
        InvalidateRect(g.hwnd, &rect, FALSE);
    }
}

static void set_hover(const Link *link) {
    if (link == g.hover) return;
    const Link *old = g.hover;
    g.hover = link;
    redraw_link(old);
    redraw_link(link);
}

static const Link *hit_test(int x, int y) {
    for (const Box *box = g.boxes; box < g.boxes + g.box_count; box++) {
        RECT rect = box_rect(box);
        POINT pt = {x, y};
        if (box->link && PtInRect(&rect, pt)) return box->link;
    }
    return NULL;
}

static HBITMAP resize_bitmap(HDC dc, HBITMAP old, HDC screen_dc) {
    HBITMAP bmp = CreateCompatibleBitmap(screen_dc, g.width, g.height);
    SelectObject(dc, bmp);
    DeleteObject(old);
    return bmp;
}

static void on_size(void) {
    RECT client;
    GetClientRect(g.hwnd, &client);
    if (client.right == 0 || client.bottom == 0) return;
    g.width = client.right;
    g.height = client.bottom;

    // Base the font size on the width without scrollbar, so toggling it can't oscillate
    BOOL scrollbar = (GetWindowLongW(g.hwnd, GWL_STYLE) & WS_VSCROLL) != 0;
    layout(g.width + (scrollbar ? GetSystemMetrics(SM_CXVSCROLL) : 0));
    g.scroll = clamp(g.scroll, 0, max(0, g.content_h - g.height));
    SCROLLINFO si = {sizeof(si), SIF_RANGE | SIF_PAGE | SIF_POS, 0, g.content_h - 1, g.height, g.scroll, 0};
    SetScrollInfo(g.hwnd, SB_VERT, &si, TRUE);
    GetClientRect(g.hwnd, &client);
    if (client.right != g.width) return; // The scrollbar toggled, the nested WM_SIZE did the work

    if (g.width != g.bmp_width || g.height != g.bmp_height) {
        HDC screen_dc = GetDC(g.hwnd);
        g.static_bmp = resize_bitmap(g.static_dc, g.static_bmp, screen_dc);
        g.back_bmp = resize_bitmap(g.back_dc, g.back_bmp, screen_dc);
        ReleaseDC(g.hwnd, screen_dc);
        g.bmp_width = g.width;
        g.bmp_height = g.height;
    }

    int count = clamp(g.width * g.height / 6000, 16, MAX_PARTICLES);
    while (g.particle_count < count) particle_init(&g.particles[g.particle_count++], TRUE);
    g.particle_count = count;
    render();
}

static void scroll_to(int y) {
    y = clamp(y, 0, max(0, g.content_h - g.height));
    if (y == g.scroll) return;
    g.scroll = y;
    SetScrollPos(g.hwnd, SB_VERT, y, TRUE);
    render();
}

static void on_vscroll(int code) {
    SCROLLINFO si = {sizeof(si), SIF_TRACKPOS, 0, 0, 0, 0, 0};
    switch (code) {
    case SB_LINEUP: scroll_to(g.scroll - g.line_h); break;
    case SB_LINEDOWN: scroll_to(g.scroll + g.line_h); break;
    case SB_PAGEUP: scroll_to(g.scroll - g.height); break;
    case SB_PAGEDOWN: scroll_to(g.scroll + g.height); break;
    case SB_TOP: scroll_to(0); break;
    case SB_BOTTOM: scroll_to(g.content_h); break;
    case SB_THUMBTRACK:
        GetScrollInfo(g.hwnd, SB_VERT, &si);
        scroll_to(si.nTrackPos);
        break;
    }
}

// Window placement: 90% of the work area, centered
static void place_window(void) {
    RECT work;
    SystemParametersInfoW(SPI_GETWORKAREA, 0, &work, 0);
    int work_w = work.right - work.left, work_h = work.bottom - work.top, w = work_w * 9 / 10, h = work_h * 9 / 10;
    SetWindowPos(g.hwnd, NULL, work.left + (work_w - w) / 2, work.top + (work_h - h) / 2, w, h,
                 SWP_NOZORDER | SWP_NOACTIVATE);
}

static void replace_window(void) {
    if (!g.user_placed && !IsZoomed(g.hwnd) && !IsIconic(g.hwnd)) place_window();
}

// A fullscreen game runs: the display left the desktop's 32 bit mode, or another window covers the whole screen
static BOOL game_running(void) {
    HDC screen = GetDC(NULL);
    int bpp = GetDeviceCaps(screen, BITSPIXEL);
    ReleaseDC(NULL, screen);
    if (bpp != 32) return TRUE;
    HWND fg = GetForegroundWindow();
    RECT r;
    return fg && fg != g.hwnd && fg != GetShellWindow() && GetWindowRect(fg, &r) && r.left <= 0 && r.top <= 0 &&
           r.right >= GetSystemMetrics(SM_CXSCREEN) && r.bottom >= GetSystemMetrics(SM_CYSCREEN);
}

// Animate at ~16 fps when active, slower when inactive, not at all when hidden or minimized; while a fullscreen
// game runs only check once a second whether it is gone, the game needs the CPU
static void update_timer(void) {
    g.paused = game_running();
    UINT interval = IsIconic(g.hwnd) || !IsWindowVisible(g.hwnd) ? 0 : g.paused ? 1000 : g.active ? 60 : 125;
    if (interval == g.interval) return;
    if (!g.interval) g.tick = GetTickCount();
    g.interval = interval;
    if (interval)
        SetTimer(g.hwnd, ID_TIMER, interval, NULL);
    else
        KillTimer(g.hwnd, ID_TIMER);
}

static void on_timer(void) {
    update_timer();
    if (g.paused) return;
    DWORD tick = GetTickCount();
    float dt = min((tick - g.tick) / 1000.0f, 0.25f);
    g.tick = tick;
    HDC dc = GetDC(g.hwnd);
    particles_step(dt, dc);
    ReleaseDC(g.hwnd, dc);
}

static const Link *cursor_link(void) {
    POINT pt;
    GetCursorPos(&pt);
    ScreenToClient(g.hwnd, &pt);
    return hit_test(pt.x, pt.y);
}

static LRESULT CALLBACK wnd_proc(HWND hwnd, UINT msg, WPARAM wParam, LPARAM lParam) {
    static const struct {
        WPARAM key;
        int code;
    } keys[] = {{VK_UP, SB_LINEUP}, {VK_DOWN, SB_LINEDOWN}, {VK_PRIOR, SB_PAGEUP},
                {VK_NEXT, SB_PAGEDOWN}, {VK_HOME, SB_TOP}, {VK_END, SB_BOTTOM}};

    switch (msg) {
    case WM_CREATE: {
        g.hwnd = hwnd;
        g.static_dc = CreateCompatibleDC(NULL);
        g.back_dc = CreateCompatibleDC(NULL);
        g.image_dc = CreateCompatibleDC(NULL);
        g.atlas_bmp = LoadImageW(GetModuleHandleW(NULL), MAKEINTRESOURCEW(ID_ATLAS), IMAGE_BITMAP, 0, 0, 0);
        SelectObject(g.image_dc, g.atlas_bmp);
        SelectObject(g.back_dc, GetStockObject(DC_BRUSH));
        SelectObject(g.back_dc, GetStockObject(NULL_PEN));
        SetBkMode(g.static_dc, TRANSPARENT);
        srand(GetTickCount());
        place_window();
        return 0;
    }

    case WM_ERASEBKGND:
        return 1;

    case WM_PAINT: {
        PAINTSTRUCT ps;
        HDC dc = BeginPaint(hwnd, &ps);
        blit(dc, g.back_dc, &ps.rcPaint);
        EndPaint(hwnd, &ps);
        return 0;
    }

    case WM_SIZE:
        on_size();
        return 0;

    case WM_GETMINMAXINFO: {
        RECT rect = {0, 0, MIN_CLIENT_W, MIN_CLIENT_H};
        AdjustWindowRectEx(&rect, WS_OVERLAPPEDWINDOW, FALSE, 0);
        MINMAXINFO *info = (MINMAXINFO *)lParam;
        info->ptMinTrackSize.x = rect.right - rect.left + GetSystemMetrics(SM_CXVSCROLL);
        info->ptMinTrackSize.y = rect.bottom - rect.top;
        return 0;
    }

    case WM_TIMECHANGE:
        on_size();
        return 0;

    case WM_WINDOWPOSCHANGED:
        update_timer();
        break;

    case WM_ACTIVATE:
        g.active = LOWORD(wParam) != WA_INACTIVE;
        update_timer();
        break;

    case WM_TIMER:
        on_timer();
        return 0;

    case WM_ENTERSIZEMOVE:
        GetWindowRect(hwnd, &g.size_move_rect);
        return 0;

    case WM_EXITSIZEMOVE: {
        RECT rect;
        GetWindowRect(hwnd, &rect);
        if (!EqualRect(&rect, &g.size_move_rect)) g.user_placed = TRUE;
        return 0;
    }

    case WM_DISPLAYCHANGE:
        replace_window();
        update_timer();
        break;

    case WM_SETTINGCHANGE:
        if (wParam == SPI_SETWORKAREA) replace_window();
        break;

    case WM_VSCROLL:
        on_vscroll(LOWORD(wParam));
        return 0;

    case WM_MOUSEWHEEL:
        g.wheel += GET_WHEEL_DELTA_WPARAM(wParam);
        scroll_to(g.scroll - g.wheel / WHEEL_DELTA * 3 * g.line_h);
        g.wheel %= WHEEL_DELTA;
        return 0;

    case WM_KEYDOWN:
        for (size_t i = 0; i < ARRAYSIZE(keys); i++)
            if (keys[i].key == wParam) on_vscroll(keys[i].code);
        return 0;

    case WM_MOUSEMOVE: {
        const Link *link = hit_test((short)LOWORD(lParam), (short)HIWORD(lParam));
        if (link && !g.hover) {
            TRACKMOUSEEVENT tme = {sizeof(tme), TME_LEAVE, hwnd, 0};
            TrackMouseEvent(&tme);
        }
        set_hover(link);
        return 0;
    }

    case WM_MOUSELEAVE:
        set_hover(NULL);
        return 0;

    case WM_SETCURSOR:
        if (LOWORD(lParam) == HTCLIENT && cursor_link()) {
            SetCursor(LoadCursorW(NULL, IDC_HAND));
            return TRUE;
        }
        break;

    case WM_LBUTTONDOWN:
        g.pressed = hit_test((short)LOWORD(lParam), (short)HIWORD(lParam));
        return 0;

    case WM_LBUTTONUP: {
        const Link *link = hit_test((short)LOWORD(lParam), (short)HIWORD(lParam));
        if (link && link == g.pressed) ShellExecuteW(NULL, L"open", link->url, NULL, NULL, SW_SHOWNORMAL);
        g.pressed = NULL;
        return 0;
    }

    case WM_DESTROY:
        KillTimer(hwnd, ID_TIMER);
        delete_fonts();
        DeleteDC(g.static_dc);
        DeleteDC(g.back_dc);
        DeleteDC(g.image_dc);
        DeleteObject(g.static_bmp);
        DeleteObject(g.back_bmp);
        DeleteObject(g.atlas_bmp);
        PostQuitMessage(0);
        return 0;
    }
    return DefWindowProcW(hwnd, msg, wParam, lParam);
}

int WINAPI wWinMain(HINSTANCE instance, HINSTANCE prev, PWSTR cmd_line, int cmd_show) {
    (void)prev;
    (void)cmd_line;
    // Single instance: a second start brings the running window back instead
    CreateMutexW(NULL, FALSE, L"Bastiaan");
    if (GetLastError() == ERROR_ALREADY_EXISTS) {
        HWND running = FindWindowW(L"Bastiaan", NULL);
        if (running) {
            if (IsIconic(running)) ShowWindow(running, SW_RESTORE);
            SetForegroundWindow(running);
        }
        return 0;
    }
    CoInitializeEx(NULL, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE);

    WNDCLASSEXW wc = {0};
    wc.cbSize = sizeof(wc);
    wc.lpfnWndProc = wnd_proc;
    wc.hInstance = instance;
    wc.hIcon = LoadIconW(instance, MAKEINTRESOURCEW(ID_ICON));
    wc.hIconSm = LoadImageW(instance, MAKEINTRESOURCEW(ID_ICON), IMAGE_ICON, GetSystemMetrics(SM_CXSMICON),
                            GetSystemMetrics(SM_CYSMICON), LR_SHARED);
    wc.hCursor = LoadCursorW(NULL, IDC_ARROW);
    wc.lpszClassName = L"Bastiaan";
    RegisterClassExW(&wc);

    HWND hwnd = CreateWindowExW(0, wc.lpszClassName, WINDOW_TITLE, WS_OVERLAPPEDWINDOW, CW_USEDEFAULT,
                                CW_USEDEFAULT, CW_USEDEFAULT, CW_USEDEFAULT, NULL, NULL, instance, NULL);
    ShowWindow(hwnd, cmd_show);
    UpdateWindow(hwnd);

    MSG msg;
    while (GetMessageW(&msg, NULL, 0, 0) > 0) {
        TranslateMessage(&msg);
        DispatchMessageW(&msg);
    }
    CoUninitialize();
    return (int)msg.wParam;
}
