// An Age of Empires I demo thingy
// clang-format off
// gcc -Wall -Wextra -Wshadow -Wpedantic --std=c11 empires.c $(pkg-config --cflags --libs sdl2) -o empires && ./empires
// clang-format on
#include <SDL.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#ifdef _WIN32
#include <windows.h>
#endif
#define EMPIRES_DEFINE
#include "empires.h"
#define FRAMEBUFFER_DEFINE
#include "framebuffer.h"
#define PERLIN_DEFINE
#include "perlin.h"

typedef struct Unit {
    int32_t id;
    float x;
    float y;
} Unit;

int main(int argc, char **argv) {
    (void)argc;
    (void)argv;

    // Find age of empires dir in Window registery
#ifdef _WIN32
    HKEY key;
    RegOpenKeyExA(HKEY_LOCAL_MACHINE, "SOFTWARE\\Wow6432Node\\Microsoft\\Games\\Age of Empires\\1.00", 0, KEY_READ,
                  &key);
    char root_path[255];
    DWORD root_path_size = sizeof(root_path);
    RegQueryValueExA(key, "InstallationDirectory", NULL, NULL, (BYTE *)root_path, &root_path_size);
#else
    char *root_path = "/Users/bplaat/Software/Age of Empires";
#endif

    // Interfac.drs
    char interface_path[255];
    strcpy(interface_path, root_path);
    strcat(interface_path, "/data/Interfac.drs");
    DRS *interface_drs = drs_new_from_file(interface_path);

    // Graphics.drs
    char graphics_path[255];
    strcpy(graphics_path, root_path);
    strcat(graphics_path, "/data/graphics.drs");
    DRS *graphics_drs = drs_new_from_file(graphics_path);

    // Terrain.drs
    char terrain_path[255];
    strcpy(terrain_path, root_path);
    strcat(terrain_path, "/data/Terrain.drs");
    DRS *terrain_drs = drs_new_from_file(terrain_path);

    // Border.drs
    char border_path[255];
    strcpy(border_path, root_path);
    strcat(border_path, "/data/Border.drs");
    DRS *border_drs = drs_new_from_file(border_path);

    // Pallet
    char *palette_text = drs_read_file(interface_drs, DRS_TABLE_BIN, 50500, NULL);
    Palette *palette = palette_new_from_text(palette_text);
    free(palette_text);

    // Load hud
    slp_header *hud_slp = drs_read_file(interface_drs, DRS_TABLE_SLP, 50743, NULL);

    // Load terrain SLP's
    // 0 sand, 1 grass, 1 water, 2 deep water
    slp_header *terrain_slps[4];
    for (int32_t i = 0; i < 4; i++) {
        terrain_slps[i] = drs_read_file(terrain_drs, DRS_TABLE_SLP, 15000 + i, NULL);
    }

    // Load border SLP's
    // 0 sea sand borders, 1 sand grass borders
    slp_header *border_slps[6];
    for (int32_t i = 0; i < 6; i++) {
        border_slps[i] = drs_read_file(border_drs, DRS_TABLE_SLP, 20000 + i, NULL);
    }

    // Load unit SLP's
    slp_header *unit_slps[716];
    for (int32_t i = 0; i < 716; i++) {
        unit_slps[i] = drs_read_file(graphics_drs, DRS_TABLE_SLP, i, NULL);
    }

    // Window
    SDL_Init(SDL_INIT_VIDEO);
    SDL_Window *window = SDL_CreateWindow("Age of Empires", SDL_WINDOWPOS_CENTERED, SDL_WINDOWPOS_CENTERED, 1024, 768,
                                          SDL_WINDOW_RESIZABLE);
    Framebuffer *framebuffer = framebuffer_new(window);

    // Camera state
    int32_t camera_x = 0;
    int32_t camera_y = 0;
    bool camera_drag = false;

    // Map generation
    int32_t map_width = 64;
    int32_t map_height = 64;
    uint16_t map[map_height * map_width];

    size_t units_size = 0;
    Unit units[1024];

#ifdef _WIN32
    srand(1);
#else
    srand(343412);
#endif
    perlin_init(rand());

    for (int32_t y = 0; y < map_height; y++) {
        for (int32_t x = 0; x < map_width; x++) {
            double n = perlin_noise((double)x / 50.0, (double)y / 50.0, 0);

            if (n > 0)
                map[y * map_width + x] = (1 << 8) | rand() % 9;
            else if (n > -0.25)
                map[y * map_width + x] = (0 << 8) | rand() % 9;
            else if (n > -0.5)
                map[y * map_width + x] = (2 << 8) | rand() % 4;
            else
                map[y * map_width + x] = (3 << 8) | rand() % 4;

            if (n > 0.3 && rand() % 2 == 1) {
                units[units_size++] = (Unit){.id = 503 + rand() % 4, .x = x, .y = y};
            }
        }
    }

    // Buildings
    units[units_size++] = (Unit){.id = 19, .x = 2, .y = 1};
    units[units_size++] = (Unit){.id = 117, .x = 5, .y = 5};
    units[units_size++] = (Unit){.id = 447, .x = 6, .y = 10};

    // Unit
    units[units_size++] = (Unit){.id = 442, .x = 11.5, .y = 11};
    units[units_size++] = (Unit){.id = 443, .x = 11, .y = 13};
    units[units_size++] = (Unit){.id = 421, .x = 8, .y = 13.5};

    // Event loop
    bool running = false;
    while (!running) {
        SDL_Event event;
        while (SDL_PollEvent(&event)) {
            if (event.type == SDL_MOUSEBUTTONDOWN && event.button.button == SDL_BUTTON_RIGHT) {
                camera_drag = true;
                break;
            }
            if (event.type == SDL_MOUSEMOTION) {
                if (camera_drag) {
                    camera_x += event.motion.xrel;
                    camera_y += event.motion.yrel;
                }
                break;
            }
            if (event.type == SDL_MOUSEBUTTONUP) {
                camera_drag = false;
                break;
            }

            if (event.type == SDL_WINDOWEVENT && event.window.event == SDL_WINDOWEVENT_RESIZED) {
                framebuffer_resize(framebuffer);
                break;
            }

            if (event.type == SDL_QUIT) {
                running = true;
                break;
            }
        }

        framebuffer_begin(framebuffer);
        framebuffer_clear(framebuffer, 0x000000);

        int32_t start_x = camera_x + framebuffer->width / 2;
        int32_t start_y = camera_y + 50;

        int32_t terrain_width = ((slp_frame *)((uint8_t *)terrain_slps[0] + sizeof(slp_header)))->width;
        int32_t terrain_hwidth = terrain_width / 2;
        int32_t terrain_height = ((slp_frame *)((uint8_t *)terrain_slps[0] + sizeof(slp_header)))->height;
        int32_t terrain_hheight = terrain_height / 2;

        // Draw map
        for (int32_t y = 0; y < map_height; y++) {
            for (int32_t x = 0; x < map_width; x++) {
                uint8_t terrain_type = map[y * map_width + x] >> 8;
                uint8_t terrain_version = map[y * map_width + x] & 0xff;

                slp_header *slp;
                slp_frame *frame;
                // if (map[y * map_width + (x - 1)] >> 8 == 0 && map[y * map_width + x] >> 8 == 1) {
                //     slp = border_slps[1];
                //     frame = (slp_frame *)((uint8_t *)slp + sizeof(slp_header) + 2 * sizeof(slp_frame));
                // } else {
                    slp = terrain_slps[terrain_type];
                    frame = (slp_frame *)((uint8_t *)slp + sizeof(slp_header) + terrain_version * sizeof(slp_frame));
                // }

                framebuffer_draw_slp(framebuffer, slp, frame,
                                     start_x - terrain_hwidth + x * terrain_hwidth - y * terrain_hwidth,
                                     start_y + x * terrain_hheight + y * terrain_hheight, palette);

                // char line[256];
                // sprintf(line, "%dx%d", x, y);
                // framebuffer_draw_text(framebuffer,
                //                       start_x - terrain_hwidth / 2 + x * terrain_hwidth - y * terrain_hwidth,
                //                       start_y + terrain_hheight + x * terrain_hheight + y * terrain_hheight, line,
                //                       strlen(line), 0xffffff);
            }
        }

        // Draw units
        for (size_t i = 0; i < units_size; i++) {
            Unit *unit = &units[i];
            slp_header *slp = unit_slps[unit->id];
            slp_frame *frame = (slp_frame *)((uint8_t *)slp + sizeof(slp_header));
            framebuffer_draw_slp(
                framebuffer, slp, frame,
                start_x - frame->center_x + (int32_t)(unit->x * terrain_hwidth) - (int32_t)(unit->y * terrain_hwidth),
                start_y - frame->center_y + terrain_hheight + (int32_t)(unit->x * terrain_hheight) +
                    (int32_t)(unit->y * terrain_hheight),
                palette);
        }

        // Draw hud
        slp_frame *hud_top_frame = (slp_frame *)((uint8_t *)hud_slp + sizeof(slp_header));
        framebuffer_draw_slp(framebuffer, hud_slp, hud_top_frame, (framebuffer->width - hud_top_frame->width) / 2, 0,
                             palette);

        slp_frame *hud_bottom_frame = (slp_frame *)((uint8_t *)hud_slp + sizeof(slp_header) + 1 * sizeof(slp_frame));
        framebuffer_fill_rect(framebuffer, (framebuffer->width - hud_bottom_frame->width) / 2,
                              framebuffer->height - hud_bottom_frame->height, hud_bottom_frame->width,
                              hud_bottom_frame->height, 0x000000);
        framebuffer_draw_slp(framebuffer, hud_slp, hud_bottom_frame, (framebuffer->width - hud_bottom_frame->width) / 2,
                             framebuffer->height - hud_bottom_frame->height, palette);

        // Draw Info
        char info[256];
        int32_t info_x = (framebuffer->width - hud_top_frame->width) / 2 + 8;
        int32_t info_y = hud_top_frame->height + 8;
        sprintf(info, "Bassie Age of Empires Demo");
        framebuffer_draw_text(framebuffer, info_x, info_y, info, strlen(info), 0xffffff);
        info_y += 12;

        sprintf(info, "Map size: %dx%d", map_width, map_height);
        framebuffer_draw_text(framebuffer, info_x, info_y, info, strlen(info), 0xffffff);
        info_y += 12;

        framebuffer_end(framebuffer);
        framebuffer_present(framebuffer);
    }

    palette_free(palette);
    drs_free(graphics_drs);
    drs_free(interface_drs);

    framebuffer_free(framebuffer);
    SDL_DestroyWindow(window);
    SDL_Quit();
    return 0;
}
