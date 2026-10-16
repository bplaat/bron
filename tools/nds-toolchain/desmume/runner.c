// Runs a .nds headless in DeSmuME and does what the commands say, frame exact, so a script or an
// agent can play a port and take screenshots. The commands come from the arguments, or from stdin
// one per line when there are none, then it answers each with ok.
//
// Usage: desmume-runner <program.nds> ['command args']...
//   wait <frames>             run frames (60 a second)
//   press <keys> [frames]     hold keys for frames (default 6), then release them
//   hold <keys>               hold keys until release
//   release                   release all keys and the stylus
//   touch <x> <y> [frames]    touch the touch screen for frames (default 6)
//   shot <file.png>           write both screens, the top one above the touch screen
//   save <file>, load <file>  save or load a savestate
//   hook <address>            count the executions of the code at address, like a function
//   count                     print "count <executions>" since hook or the last count
//   profile start             count the ARM9 cycles per instruction address
//   busy                      print "busy <percent>", the ARM9 time not waiting since the last busy
//   profile save <file>       write "<address> <cycles>" lines so far, the busiest first
//   profile stop              stop counting
//   wav <file>, wav stop      record the sound to a WAV file, stop recording
//   pak                       put a Memory Expansion Pak in the GBA slot, before the first wait
//   peek <address> [words]    print "peek <address> <word>..." of the ARM9's memory, in hex
//   quit                      stop, also the end of the commands
// Keys are a, b, x, y, l, r, start, select, up, down, left and right, joined by +, like up+a.

#include <stdarg.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <zlib.h>

typedef int BOOL;

int desmume_init(void);
void desmume_free(void);
int desmume_open(const char* filename);
void desmume_resume(void);
void desmume_cycle(BOOL with_joystick);
uint16_t* desmume_draw_raw(void);
BOOL desmume_savestate_load(const char* file_name);
BOOL desmume_savestate_save(const char* file_name);
void desmume_input_keypad_update(uint16_t keys);
void desmume_input_set_touch_pos(uint16_t x, uint16_t y);
void desmume_input_release_touch(void);
void desmume_memory_write_long(int address, unsigned long value);
uint32_t* desmume_profile_start(void);
void desmume_profile_stop(void);
BOOL desmume_wav_begin(const char* file_name);
void desmume_wav_end(void);
void desmume_memory_register_exec(int address, int size, BOOL (*callback)(unsigned address, int size));
BOOL desmume_insert_expansion_pak(void);
unsigned long desmume_memory_read_long(int address);

#define WIDTH 256
#define HEIGHT 384

// ARM9 cycles per frame, the profile's indexes like profile9_index in DeSmuME's NDSSystem.h
#define FRAME_CYCLES 1120380
#define PROFILE_MAIN_SIZE (0x400000 >> 1)
#define PROFILE_ITCM_SIZE (0x8000 >> 1)
#define PROFILE_SIZE (PROFILE_MAIN_SIZE + PROFILE_ITCM_SIZE + 1)

// The magic of the homebrew argv header, DeSmuME leaves one with argv[0] fat:/<rom>.nds for an R4
#define ARGV_MAGIC_ADDRESS 0x02fffe70

// In the bit order of the DS keypad registers, which DeSmuME's keypad uses too
static const char* const key_names[] = {"a", "b", "select", "start", "right", "left", "up", "down", "r", "l", "x", "y"};

static uint16_t held_keys;
static unsigned executions;
static uint32_t* profile;
static uint64_t profile_frames, busy_cycles, busy_frames;

static void die(const char* format, ...) {
    va_list args;
    va_start(args, format);
    fprintf(stderr, "desmume-runner: ");
    vfprintf(stderr, format, args);
    fputc('\n', stderr);
    va_end(args);
    exit(1);
}

static int parse_number(const char* text) {
    char* end;
    long value = text != NULL ? strtol(text, &end, 10) : -1;
    if (text == NULL || *end != '\0' || value < 0)
        die("Expected a number, got '%s'", text != NULL ? text : "nothing");
    return (int)value;
}

static BOOL count_execution(unsigned address, int size) {
    (void)address;
    (void)size;
    executions++;
    return 1;
}

static uint16_t parse_keys(const char* text) {
    if (text == NULL)
        die("Expected keys");
    uint16_t keys = 0;
    char copy[256];
    snprintf(copy, sizeof(copy), "%s", text);
    char* save;
    for (char* name = strtok_r(copy, "+", &save); name != NULL; name = strtok_r(NULL, "+", &save)) {
        size_t i = 0;
        while (i < sizeof(key_names) / sizeof(key_names[0]) && strcmp(name, key_names[i]) != 0)
            i++;
        if (i == sizeof(key_names) / sizeof(key_names[0]))
            die("Unknown key '%s'", name);
        keys |= 1 << i;
    }
    return keys;
}

static void run(int frames) {
    desmume_input_keypad_update(held_keys);
    for (int i = 0; i < frames; i++)
        desmume_cycle(0);
    profile_frames += frames;
}

static uint64_t profile_total(void) {
    uint64_t total = 0;
    for (int i = 0; i < PROFILE_SIZE; i++)
        total += profile[i];
    return total;
}

static int compare_cycles(const void* a, const void* b) {
    uint32_t x = profile[*(const int*)a], y = profile[*(const int*)b];
    return x < y ? 1 : x > y ? -1 : 0;
}

static void profile_save(const char* path) {
    FILE* file = fopen(path, "w");
    if (file == NULL)
        die("Can't write %s", path);
    int count = 0;
    static int order[PROFILE_SIZE];
    for (int i = 0; i < PROFILE_SIZE; i++)
        if (profile[i] != 0)
            order[count++] = i;
    qsort(order, count, sizeof(order[0]), compare_cycles);
    fprintf(file, "# frames %llu busy %.1f%%\n", (unsigned long long)profile_frames,
            profile_frames ? 100.0 * profile_total() / (profile_frames * FRAME_CYCLES) : 0.0);
    for (int i = 0; i < count; i++) {
        int index = order[i];
        unsigned address = index < PROFILE_MAIN_SIZE ? 0x02000000 + index * 2
                           : index < PROFILE_MAIN_SIZE + PROFILE_ITCM_SIZE
                               ? 0x01ff8000 + (index - PROFILE_MAIN_SIZE) * 2
                               : 0;
        fprintf(file, "%08x %u\n", address, profile[index]);
    }
    fclose(file);
}

static void put_u32(uint8_t* out, uint32_t value) {
    out[0] = value >> 24;
    out[1] = value >> 16;
    out[2] = value >> 8;
    out[3] = value;
}

static void write_chunk(FILE* file, const char* type, const uint8_t* data, uint32_t size) {
    uint8_t header[8];
    put_u32(header, size);
    memcpy(header + 4, type, 4);
    uint32_t crc = crc32(crc32(0, header + 4, 4), data, size);
    uint8_t footer[4];
    put_u32(footer, crc);
    fwrite(header, 1, 8, file);
    fwrite(data, 1, size, file);
    fwrite(footer, 1, 4, file);
}

// Writes the screens as an 8-bit RGB PNG, every row with filter 0 before its pixels
static void shot(const char* path) {
    static uint8_t raw[HEIGHT * (1 + WIDTH * 3)];
    const uint16_t* pixels = desmume_draw_raw();
    uint8_t* out = raw;
    for (int y = 0; y < HEIGHT; y++) {
        *out++ = 0;
        for (int x = 0; x < WIDTH; x++) {
            uint16_t pixel = *pixels++;
            for (int shift = 0; shift < 15; shift += 5) {
                uint8_t value = (pixel >> shift) & 31;
                *out++ = value << 3 | value >> 2;
            }
        }
    }
    uLongf size = compressBound(sizeof(raw));
    uint8_t* compressed = malloc(size);
    if (compressed == NULL || compress2(compressed, &size, raw, sizeof(raw), 9) != Z_OK)
        die("Can't compress %s", path);

    FILE* file = fopen(path, "wb");
    if (file == NULL)
        die("Can't write %s", path);
    uint8_t header[13] = {0};
    put_u32(header, WIDTH);
    put_u32(header + 4, HEIGHT);
    header[8] = 8;  // bits per channel
    header[9] = 2;  // RGB
    fwrite("\x89PNG\r\n\x1a\n", 1, 8, file);
    write_chunk(file, "IHDR", header, sizeof(header));
    write_chunk(file, "IDAT", compressed, size);
    write_chunk(file, "IEND", NULL, 0);
    fclose(file);
    free(compressed);
}

// Runs one command from its words, false on quit
static int command(char** words) {
    const char* name = words[0];
    if (strcmp(name, "wait") == 0) {
        run(parse_number(words[1]));
    } else if (strcmp(name, "press") == 0) {
        uint16_t keys = parse_keys(words[1]);
        held_keys |= keys;
        run(words[2] != NULL ? parse_number(words[2]) : 6);
        held_keys &= ~keys;
        run(1);
    } else if (strcmp(name, "hold") == 0) {
        held_keys |= parse_keys(words[1]);
        run(1);
    } else if (strcmp(name, "release") == 0) {
        held_keys = 0;
        desmume_input_release_touch();
        run(1);
    } else if (strcmp(name, "touch") == 0) {
        int x = parse_number(words[1]), y = parse_number(words[2]);
        desmume_input_set_touch_pos(x, y);
        run(words[3] != NULL ? parse_number(words[3]) : 6);
        desmume_input_release_touch();
        run(1);
    } else if (strcmp(name, "shot") == 0) {
        if (words[1] == NULL)
            die("Expected a file");
        shot(words[1]);
    } else if (strcmp(name, "save") == 0 || strcmp(name, "load") == 0) {
        if (words[1] == NULL)
            die("Expected a file");
        BOOL ok = name[0] == 's' ? desmume_savestate_save(words[1]) : desmume_savestate_load(words[1]);
        if (!ok)
            die("Can't use savestate %s", words[1]);
    } else if (strcmp(name, "hook") == 0) {
        char* end;
        unsigned long address = words[1] != NULL ? strtoul(words[1], &end, 16) : 0;
        if (words[1] == NULL || *end != '\0')
            die("Expected a hex address");
        // Thumb function symbols have bit 0 set, the instruction is at the even address
        desmume_memory_register_exec(address & ~1UL, 2, count_execution);
        executions = 0;
    } else if (strcmp(name, "count") == 0) {
        printf("count %u\n", executions);
        fflush(stdout);
        executions = 0;
    } else if (strcmp(name, "profile") == 0 && words[1] != NULL && strcmp(words[1], "start") == 0) {
        profile = desmume_profile_start();
        profile_frames = busy_cycles = busy_frames = 0;
    } else if (strcmp(name, "profile") == 0 && words[1] != NULL && strcmp(words[1], "save") == 0) {
        if (profile == NULL)
            die("No profile started");
        if (words[2] == NULL)
            die("Expected a file");
        profile_save(words[2]);
    } else if (strcmp(name, "profile") == 0 && words[1] != NULL && strcmp(words[1], "stop") == 0) {
        desmume_profile_stop();
        profile = NULL;
    } else if (strcmp(name, "busy") == 0) {
        if (profile == NULL)
            die("No profile started");
        uint64_t total = profile_total(), frames = profile_frames - busy_frames;
        printf("busy %.1f\n", frames ? 100.0 * (total - busy_cycles) / (frames * FRAME_CYCLES) : 0.0);
        fflush(stdout);
        busy_cycles = total;
        busy_frames = profile_frames;
    } else if (strcmp(name, "wav") == 0) {
        if (words[1] == NULL)
            die("Expected a file");
        if (strcmp(words[1], "stop") == 0)
            desmume_wav_end();
        else if (!desmume_wav_begin(words[1]))
            die("Can't write %s", words[1]);
    } else if (strcmp(name, "peek") == 0) {
        char* end;
        unsigned long address = words[1] != NULL ? strtoul(words[1], &end, 16) : 0;
        if (words[1] == NULL || *end != '\0')
            die("Expected a hex address");
        int count = words[2] != NULL ? parse_number(words[2]) : 1;
        printf("peek %08lx", address);
        for (int i = 0; i < count; i++)
            printf(" %08lx", desmume_memory_read_long((int)(address + i * 4)) & 0xffffffffUL);
        printf("\n");
        fflush(stdout);
    } else if (strcmp(name, "pak") == 0) {
        desmume_insert_expansion_pak();
    } else if (strcmp(name, "quit") == 0) {
        return 0;
    } else {
        die("Unknown command '%s'", name);
    }
    return 1;
}

// Splits a command line in words and runs it, false on quit, empty lines and # comments are skipped
static int line(char* text) {
    char* words[5] = {0};
    int count = 0;
    char* save;
    for (char* word = strtok_r(text, " \t\r\n", &save); word != NULL && count < 4;
         word = strtok_r(NULL, " \t\r\n", &save))
        words[count++] = word;
    return count == 0 || words[0][0] == '#' || command(words);
}

int main(int argc, char** argv) {
    if (argc < 2) {
        fprintf(stderr, "Usage: %s <program.nds> ['command args']...\n", argv[0]);
        return 1;
    }
    // No sound or window, the emulator only runs
    setenv("SDL_AUDIODRIVER", "dummy", 1);
    setenv("SDL_VIDEODRIVER", "dummy", 1);
    if (desmume_init() != 0)
        die("Can't start DeSmuME");
    if (desmume_open(argv[1]) < 0)
        die("Can't open %s", argv[1]);
    // Without an R4 there is no fat:/, so the argv is dropped and NitroFS reads the card itself,
    // like in melonDS
    desmume_memory_write_long(ARGV_MAGIC_ADDRESS, 0);
    desmume_resume();

    if (argc > 2) {
        for (int i = 2; i < argc && line(argv[i]); i++) {}
    } else {
        char text[1024];
        while (fgets(text, sizeof(text), stdin) != NULL && line(text)) {
            printf("ok\n");
            fflush(stdout);
        }
    }
    desmume_wav_end();
    desmume_free();
    return 0;
}
