// A cross-platform SDL2 application which you can use to explorer Age of Empires DRS container files
// clang-format off
// gcc -Wall -Wextra -Wshadow -Wpedantic --std=c11 drsviewer.c $(pkg-config --cflags --libs sdl2) -o drsviewer && ./drsviewer
// clang-format on
#include <SDL.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdbool.h>
#include <stdio.h>
#include <stdlib.h>
#ifdef _WIN32
#include <windows.h>
#endif
#define EMPIRES_DEFINE
#include "empires.h"
#define FRAMEBUFFER_DEFINE
#include "framebuffer.h"

#ifdef _WIN32
#define SCROLL_SENSITIVITY 24
#else
#define SCROLL_SENSITIVITY 8
#endif

bool isAsciiFile(char *text, size_t size) {
    for (size_t i = 0; i < size; i++) {
        if (!(text[i] == '\t' || text[i] == '\n' || text[i] == '\r' || (text[i] >= ' ' && text[i] <= '~'))) {
            return false;
        }
    }
    return true;
}

int main(int argc, char **argv) {
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

    // Palette
    char *palette_text = drs_read_file(interface_drs, DRS_TABLE_BIN, 50500, NULL);
    Palette *default_palette = palette_new_from_text(palette_text);
    free(palette_text);

    // Load DRS
    char drs_path[255];
    strcpy(drs_path, root_path);
    strcat(drs_path, "/data/");
    strcat(drs_path, argc == 1 ? "Interfac.drs" : argv[1]);
    DRS *drs = drs_new_from_file(drs_path);

    // Window
    SDL_Init(SDL_INIT_VIDEO | SDL_INIT_AUDIO);
    char window_title[512];
    sprintf(window_title, "%s - DRS Viewer", drs_path);
    SDL_Window *window =
        SDL_CreateWindow(window_title, SDL_WINDOWPOS_CENTERED, SDL_WINDOWPOS_CENTERED, 1280, 720, SDL_WINDOW_RESIZABLE);
    Framebuffer *framebuffer = framebuffer_new(window);

    // State
    int32_t scroll_y = 0;
    struct {
        uint32_t extension;
        int32_t id;
        size_t size;
        void *ptr;
        int32_t frame;
    } selected = {0};

    // Event loop
    bool running = false;
    while (!running) {
        SDL_Event event;
        while (SDL_PollEvent(&event)) {
            if (event.type == SDL_MOUSEBUTTONDOWN) {
                int32_t y = 8 - scroll_y;
                for (int32_t i = 0; i < drs->header.table_count; i++) {
                    DRSTable *table = &drs->tables[i];
                    y += 12;
                    for (int32_t j = 0; j < table->header.file_count; j++) {
                        drs_file *file = &table->files[j];
                        if (event.button.x < 300 && event.button.y >= y && event.button.y < y + 12) {
                            if (selected.ptr != NULL) free(selected.ptr);

                            selected.extension = table->header.extension.integer;
                            selected.id = file->id;
                            selected.ptr = drs_read_file(drs, selected.extension, file->id, &selected.size);
                            selected.frame = 0;

                            if (selected.extension == DRS_TABLE_WAV) {
                                FILE *f = fopen("tmp.wav", "wb");
                                fwrite(selected.ptr, selected.size, 1, f);
                                fclose(f);

                                SDL_AudioSpec wavSpec;
                                uint8_t *wavBuffer;
                                uint32_t wavLength;
                                SDL_LoadWAV("tmp.wav", &wavSpec, &wavBuffer, &wavLength);
                                SDL_AudioDeviceID deviceId = SDL_OpenAudioDevice(NULL, 0, &wavSpec, NULL, 0);
                                SDL_QueueAudio(deviceId, wavBuffer, wavLength);
                                SDL_PauseAudioDevice(deviceId, 0);

                                // TODO releasing SDL audio stuff
                                // SDL_CloseAudioDevice(deviceId);
                                // SDL_FreeWAV(wavBuffer);
                            }
                            break;
                        }
                        y += 12;
                    }
                    y += 12;
                }

                if (event.button.x >= 300 && selected.extension == DRS_TABLE_SLP) {
                    if (selected.extension == DRS_TABLE_SLP) {
                        slp_header *slp = (slp_header *)selected.ptr;
                        selected.frame++;
                        if (selected.frame == slp->frame_count) {
                            selected.frame = 0;
                        }
                    }
                }
                break;
            }

            if (event.type == SDL_WINDOWEVENT && event.window.event == SDL_WINDOWEVENT_RESIZED) {
                framebuffer_resize(framebuffer);
                break;
            }

            if (event.type == SDL_MOUSEWHEEL) {
                scroll_y -= event.wheel.y * SCROLL_SENSITIVITY;
                if (scroll_y < 0) scroll_y = 0;
                break;
            }

            if (event.type == SDL_QUIT) {
                running = true;
                break;
            }
        }

        framebuffer_begin(framebuffer);

        // Draw background
        framebuffer_clear(framebuffer, 0xffffff);

        // Draw sidebar
        framebuffer_fill_rect(framebuffer, 0, 0, 299, framebuffer->height, 0xeeeeee);
        framebuffer_fill_rect(framebuffer, 299, 0, 1, framebuffer->height, 0x000000);

        int32_t sidebar_y = 8 - scroll_y;
        char line[256];
        for (int32_t i = 0; i < drs->header.table_count; i++) {
            DRSTable *table = &drs->tables[i];
            char tableExtension[5];
            if (table->header.extension.integer == DRS_TABLE_BIN) {
                strcpy(tableExtension, "bin");
            } else {
                int32_t j = 0;
                while (j < 4 && table->header.extension.string[3 - j] != ' ') {
                    tableExtension[j] = table->header.extension.string[3 - j];
                    j++;
                }
                tableExtension[j] = '\0';
            }

            sprintf(line, ".%s (%d files):", tableExtension, table->header.file_count);
            framebuffer_draw_text(framebuffer, 8, sidebar_y, line, strlen(line), 0x000000);
            sidebar_y += 12;
            for (int32_t j = 0; j < table->header.file_count; j++) {
                drs_file *file = &table->files[j];
                if (table->header.extension.integer == selected.extension && file->id == selected.id) {
                    framebuffer_fill_rect(framebuffer, 0, sidebar_y, 300, 12, 0xcccccc);
                }
                sprintf(line, "%d.%s: %d bytes", file->id, tableExtension, file->size);
                framebuffer_draw_text(framebuffer, 8, sidebar_y + 2, line, strlen(line), 0x000000);
                sidebar_y += 12;
            }
            sidebar_y += 12;
        }

        // Draw selected item
        if (selected.ptr != NULL) {
            if (selected.extension == DRS_TABLE_BIN || isAsciiFile(selected.ptr, selected.size)) {
                int32_t text_y = 8 - scroll_y;
                char *c = selected.ptr;
                while (c < (char *)selected.ptr + selected.size && *c != '\0') {
                    char *lineStart = c;
                    while (*c != '\r' && *c != '\n' && *c != '\0') c++;
                    framebuffer_draw_text(framebuffer, 308, text_y, lineStart, c - lineStart, 0x000000);
                    if (*c == '\r') c++;
                    c++;
                    text_y += 12;
                }

                char *paletteFirstline = "JASC-PAL\n";
                if (memcmp(selected.ptr, paletteFirstline, strlen(paletteFirstline))) {
                    Palette *palette = palette_new_from_text(selected.ptr);
                    int32_t columns = 16;
                    for (int32_t y = 0; y < (int32_t)palette->size / columns; y++) {
                        for (int32_t x = 0; x < columns; x++) {
                            framebuffer_fill_rect(framebuffer, framebuffer->width - 8 - columns * 16 + x * 16,
                                                  8 + y * 16, 16, 16, palette->colors[y * columns + x]);
                        }
                    }

                    palette_free(palette);
                }
            } else if (selected.extension == DRS_TABLE_SLP) {
                slp_header *slp = (slp_header *)selected.ptr;
                sprintf(line, "SLP Frame %d / %d", selected.frame + 1, slp->frame_count);
                framebuffer_draw_text(framebuffer, 308, 8, line, strlen(line), 0x000000);

                slp_frame *frame =
                    (slp_frame *)((uint8_t *)selected.ptr + sizeof(slp_header) + selected.frame * (sizeof(slp_frame)));
                framebuffer_draw_slp(framebuffer, (slp_header *)selected.ptr, frame,
                                     300 + ((framebuffer->width - 300) - frame->width) / 2,
                                     (framebuffer->height - frame->height) / 2, default_palette);
            } else if (selected.extension == DRS_TABLE_WAV) {
                char *text = "You can hear the WAV sound playing!";
                framebuffer_draw_text(framebuffer, 308, 8, text, strlen(text), 0x000000);
            } else {
                char *text = "Can't view this file type!";
                framebuffer_draw_text(framebuffer, 308, 8, text, strlen(text), 0x000000);
            }
        } else {
            char *text = "Open a file with the sidebar!";
            framebuffer_draw_text(framebuffer, 308, 8, text, strlen(text), 0x000000);
        }

        framebuffer_end(framebuffer);
        framebuffer_present(framebuffer);
    }

    drs_free(drs);
    palette_free(default_palette);
    drs_free(interface_drs);

    framebuffer_free(framebuffer);
    SDL_DestroyWindow(window);
    SDL_Quit();
    return 0;
}
