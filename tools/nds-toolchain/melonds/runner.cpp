// Runs a .nds headless in melonDS and does what the commands say, frame exact, so a script or an
// agent can play a port and take screenshots. The commands come from the arguments, or from stdin
// one per line when there are none, then it answers each with ok.
//
// Usage: melonds-runner [--dsi] [--fat <directory>] <program.nds> ['command args']...
//   --dsi                     run as a DSi instead of a DS
//   --fat <directory>         put an SD card with the files of the directory in the R4 card (DS)
//                             or the SD card slot (DSi), its writes stay in emulated memory
//   test <address> <frames>   await test results, fail on errors or timeout
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
//   pak                       put a Memory Expansion Pak in the GBA slot (DS only)
//   peek <address> [words]    print "peek <address> <word>..." of the ARM9's memory, in hex
//   quit                      stop, also the end of the commands
// Keys are a, b, x, y, l, r, start, select, up, down, left and right, joined by +, like up+a.

#include <unistd.h>
#include <zlib.h>

#include <algorithm>
#include <chrono>
#include <climits>
#include <condition_variable>
#include <cstdarg>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <ctime>
#include <filesystem>
#include <memory>
#include <mutex>
#include <string>
#include <thread>
#include <vector>

#include "DSi.h"
#include "FreeBIOS.h"
#include "GBACart.h"
#include "NDS.h"
#include "NDSCart.h"
#include "Platform.h"
#include "Savestate.h"

using namespace melonDS;

#define WIDTH 256
#define HEIGHT 384

// The rate of the recorded sound
#define SAMPLE_RATE 48000

// System cycles per frame, the ARM9 runs 2 (or on a DSi 4) times as fast
#define FRAME_SYSTEM_CYCLES 560190

// The profile's indexes: main RAM by halfword, then ITCM by halfword, then one for everything else
#define PROFILE_MAIN_SIZE (0x1000000 >> 1)
#define PROFILE_ITCM_SIZE (0x8000 >> 1)
#define PROFILE_SIZE (PROFILE_MAIN_SIZE + PROFILE_ITCM_SIZE + 1)

// The homebrew argv header, melonDS sets it up for an R4 card, the runner for the DSi's SD card
#define ARGV_MAGIC_ADDRESS 0x02fffe70
#define ARGV_MAGIC 0x5f617267

// In the bit order of the DS keypad registers, which melonDS's key mask uses too
static const char* const key_names[] = {"a", "b", "select", "start", "right", "left", "up", "down", "r", "l", "x", "y"};

static NDS* nds;
static std::filesystem::path temp_dir;
static u32 held_keys;
static u32 hook_address = UINT32_MAX;
static unsigned executions;
static std::vector<u32> profile;
static u64 profile_frames, busy_cycles, busy_frames;
static FILE* answers;
static FILE* wav;
static u32 wav_samples;

static void die(const char* format, ...) {
    va_list args;
    va_start(args, format);
    fprintf(stderr, "melonds-runner: ");
    vfprintf(stderr, format, args);
    fputc('\n', stderr);
    va_end(args);
    exit(1);
}

// The headless platform: files are stdio files, local files (the SD card's image) go to a temporary
// directory, there is no network, camera, microphone or saving, logs are errors only
namespace melonDS::Platform {

void SignalStop(StopReason, void*) {}

std::string GetLocalFilePath(const std::string& filename) {
    return (temp_dir / filename).string();
}

static const char* stdio_mode(FileMode mode, bool exists) {
    bool read = mode & FileMode::Read;
    if (mode & FileMode::Append)
        return read ? "a+b" : "ab";
    if (!(mode & FileMode::Write))
        return "rb";
    if ((mode & FileMode::Preserve) && exists)
        return "r+b";
    return read ? "w+b" : "wb";
}

FileHandle* OpenFile(const std::string& path, FileMode mode) {
    bool exists = std::filesystem::exists(path);
    if ((mode & FileMode::Write) && (mode & FileMode::NoCreate) && !exists)
        return nullptr;
    return reinterpret_cast<FileHandle*>(fopen(path.c_str(), stdio_mode(mode, exists)));
}

FileHandle* OpenLocalFile(const std::string& path, FileMode mode) {
    return OpenFile(GetLocalFilePath(path), mode);
}

bool FileExists(const std::string& name) {
    return std::filesystem::exists(name);
}

bool LocalFileExists(const std::string& name) {
    return FileExists(GetLocalFilePath(name));
}

bool CheckFileWritable(const std::string&) {
    return true;
}

bool CheckLocalFileWritable(const std::string&) {
    return true;
}

static FILE* stdio(FileHandle* file) {
    return reinterpret_cast<FILE*>(file);
}

bool CloseFile(FileHandle* file) {
    return fclose(stdio(file)) == 0;
}

bool IsEndOfFile(FileHandle* file) {
    return feof(stdio(file)) != 0;
}

bool FileReadLine(char* str, int count, FileHandle* file) {
    return fgets(str, count, stdio(file)) != nullptr;
}

u64 FilePosition(FileHandle* file) {
    return ftello(stdio(file));
}

bool FileSeek(FileHandle* file, s64 offset, FileSeekOrigin origin) {
    int whence = origin == FileSeekOrigin::Start ? SEEK_SET : origin == FileSeekOrigin::Current ? SEEK_CUR : SEEK_END;
    return fseeko(stdio(file), offset, whence) == 0;
}

void FileRewind(FileHandle* file) {
    rewind(stdio(file));
}

u64 FileRead(void* data, u64 size, u64 count, FileHandle* file) {
    return fread(data, size, count, stdio(file));
}

bool FileFlush(FileHandle* file) {
    return fflush(stdio(file)) == 0;
}

u64 FileWrite(const void* data, u64 size, u64 count, FileHandle* file) {
    return fwrite(data, size, count, stdio(file));
}

u64 FileWriteFormatted(FileHandle* file, const char* fmt, ...) {
    va_list args;
    va_start(args, fmt);
    int length = vfprintf(stdio(file), fmt, args);
    va_end(args);
    return length < 0 ? 0 : length;
}

u64 FileLength(FileHandle* file) {
    off_t position = ftello(stdio(file));
    fseeko(stdio(file), 0, SEEK_END);
    off_t length = ftello(stdio(file));
    fseeko(stdio(file), position, SEEK_SET);
    return length;
}

void Log(LogLevel level, const char* fmt, ...) {
    if (level != LogLevel::Error)
        return;
    va_list args;
    va_start(args, fmt);
    fprintf(stderr, "melonDS: ");
    vfprintf(stderr, fmt, args);
    va_end(args);
}

struct Thread {
    std::thread thread;
};

Thread* Thread_Create(std::function<void()> func) {
    return new Thread{std::thread(std::move(func))};
}

void Thread_Free(Thread* thread) {
    if (thread->thread.joinable())
        thread->thread.detach();
    delete thread;
}

void Thread_Wait(Thread* thread) {
    thread->thread.join();
}

struct Semaphore {
    std::mutex mutex;
    std::condition_variable condition;
    int count = 0;
};

Semaphore* Semaphore_Create() {
    return new Semaphore;
}

void Semaphore_Free(Semaphore* sema) {
    delete sema;
}

void Semaphore_Reset(Semaphore* sema) {
    std::lock_guard lock(sema->mutex);
    sema->count = 0;
}

void Semaphore_Wait(Semaphore* sema) {
    std::unique_lock lock(sema->mutex);
    sema->condition.wait(lock, [sema] { return sema->count > 0; });
    sema->count--;
}

bool Semaphore_TryWait(Semaphore* sema, int timeout_ms) {
    std::unique_lock lock(sema->mutex);
    if (!sema->condition.wait_for(lock, std::chrono::milliseconds(timeout_ms), [sema] { return sema->count > 0; }))
        return false;
    sema->count--;
    return true;
}

void Semaphore_Post(Semaphore* sema, int count) {
    {
        std::lock_guard lock(sema->mutex);
        sema->count += count;
    }
    sema->condition.notify_all();
}

struct Mutex {
    std::mutex mutex;
};

Mutex* Mutex_Create() {
    return new Mutex;
}

void Mutex_Free(Mutex* mutex) {
    delete mutex;
}

void Mutex_Lock(Mutex* mutex) {
    mutex->mutex.lock();
}

void Mutex_Unlock(Mutex* mutex) {
    mutex->mutex.unlock();
}

bool Mutex_TryLock(Mutex* mutex) {
    return mutex->mutex.try_lock();
}

void Sleep(u64 usecs) {
    std::this_thread::sleep_for(std::chrono::microseconds(usecs));
}

u64 GetMSCount() {
    return GetUSCount() / 1000;
}

u64 GetUSCount() {
    using namespace std::chrono;
    return duration_cast<microseconds>(steady_clock::now().time_since_epoch()).count();
}

void WriteNDSSave(const u8*, u32, u32, u32, void*) {}
void WriteGBASave(const u8*, u32, u32, u32, void*) {}
void WriteFirmware(const Firmware&, u32, u32, void*) {}
void WriteDateTime(int, int, int, int, int, int, void*) {}

void MP_Begin(void*) {}
void MP_End(void*) {}
int MP_SendPacket(u8*, int, u64, void*) {
    return 0;
}
int MP_RecvPacket(u8*, u64*, void*) {
    return 0;
}
int MP_SendCmd(u8*, int, u64, void*) {
    return 0;
}
int MP_SendReply(u8*, int, u64, u16, void*) {
    return 0;
}
int MP_SendAck(u8*, int, u64, void*) {
    return 0;
}
int MP_RecvHostPacket(u8*, u64*, void*) {
    return 0;
}
u16 MP_RecvReplies(u8*, u64, u16, void*) {
    return 0;
}
int Net_SendPacket(u8*, int, void*) {
    return 0;
}
int Net_RecvPacket(u8*, void*) {
    return 0;
}

void Camera_Start(int, void*) {}
void Camera_Stop(int, void*) {}
void Camera_CaptureFrame(int, u32*, int, int, bool, void*) {}
void Mic_Start(void*) {}
void Mic_Stop(void*) {}
int Mic_ReadInput(s16*, int, void*) {
    return 0;
}

AACDecoder* AAC_Init() {
    return nullptr;
}
void AAC_DeInit(AACDecoder*) {}
bool AAC_Configure(AACDecoder*, int, int) {
    return false;
}
bool AAC_DecodeFrame(AACDecoder*, const void*, int, void*, int) {
    return false;
}

bool Addon_KeyDown(KeyType, void*) {
    return false;
}
void Addon_RumbleStart(u32, void*) {}
void Addon_RumbleStop(void*) {}
float Addon_MotionQuery(MotionQueryType, void*) {
    return 0;
}

DynamicLibrary* DynamicLibrary_Load(const char*) {
    return nullptr;
}
void DynamicLibrary_Unload(DynamicLibrary*) {}
void* DynamicLibrary_LoadFunction(DynamicLibrary*, const char*) {
    return nullptr;
}

}  // namespace melonDS::Platform

static int parse_number(const char* text) {
    char* end;
    long value = text != nullptr ? strtol(text, &end, 10) : -1;
    if (text == nullptr || *text == '\0' || *end != '\0' || value < 0 || value > INT_MAX)
        die("Expected a number, got '%s'", text != nullptr ? text : "nothing");
    return (int)value;
}

static u32 parse_address(const char* text) {
    char* end;
    unsigned long address = text != nullptr ? strtoul(text, &end, 16) : 0;
    if (text == nullptr || *end != '\0' || address > UINT32_MAX)
        die("Expected a hex address");
    return (u32)address;
}

static u32 parse_keys(const char* text) {
    if (text == nullptr)
        die("Expected keys");
    u32 keys = 0;
    char copy[256];
    snprintf(copy, sizeof(copy), "%s", text);
    char* save;
    for (char* name = strtok_r(copy, "+", &save); name != nullptr; name = strtok_r(nullptr, "+", &save)) {
        size_t i = 0;
        while (i < sizeof(key_names) / sizeof(key_names[0]) && strcmp(name, key_names[i]) != 0)
            i++;
        if (i == sizeof(key_names) / sizeof(key_names[0]))
            die("Unknown key '%s'", name);
        keys |= 1 << i;
    }
    return keys;
}

static u32 frame_cycles() {
    return FRAME_SYSTEM_CYCLES << nds->ARM9ClockShift;
}

static u32 profile_index(u32 address) {
    if ((address & 0x0f000000) == 0x02000000)
        return (address & 0xffffff) >> 1;
    if (address < 0x02000000)
        return PROFILE_MAIN_SIZE + ((address & 0x7fff) >> 1);
    return PROFILE_SIZE - 1;
}

static void count_instruction(void*, u32 address, s32 cycles) {
    if (address == hook_address)
        executions++;
    if (!profile.empty())
        profile[profile_index(address)] += cycles;
}

// The ARM9 calls count_instruction only while something is counted
static void update_instruction_hook() {
    nds->ARM9.InstrHook = hook_address != UINT32_MAX || !profile.empty() ? count_instruction : nullptr;
}

static void put_le(FILE* file, u32 value, int bytes) {
    for (int i = 0; i < bytes; i++)
        fputc(value >> (i * 8) & 0xff, file);
}

// Writes the 44 byte header of a 16-bit stereo WAV with the samples so far
static void wav_header() {
    u32 size = wav_samples * 4;
    fseek(wav, 0, SEEK_SET);
    fwrite("RIFF", 1, 4, wav);
    put_le(wav, 36 + size, 4);
    fwrite("WAVEfmt ", 1, 8, wav);
    put_le(wav, 16, 4);
    put_le(wav, 1, 2);  // PCM
    put_le(wav, 2, 2);  // channels
    put_le(wav, SAMPLE_RATE, 4);
    put_le(wav, SAMPLE_RATE * 4, 4);
    put_le(wav, 4, 2);
    put_le(wav, 16, 2);
    fwrite("data", 1, 4, wav);
    put_le(wav, size, 4);
    fseek(wav, 0, SEEK_END);
}

static void wav_end() {
    if (wav == nullptr)
        return;
    wav_header();
    fclose(wav);
    wav = nullptr;
}

// Takes the sound of the frames that ran from the SPU, to the WAV file when recording
static void drain_sound() {
    static s16 samples[1024 * 2];
    int count;
    while ((count = nds->SPU.ReadOutput(samples, 1024)) > 0) {
        if (wav != nullptr) {
            fwrite(samples, 4, count, wav);
            wav_samples += count;
        }
    }
}

static void run(int frames) {
    nds->SetKeyMask(~held_keys & 0xfff);
    for (int i = 0; i < frames; i++) {
        nds->RunFrame();
        drain_sound();
    }
    profile_frames += frames;
}

static u64 profile_total() {
    u64 total = 0;
    for (u32 cycles : profile)
        total += cycles;
    return total;
}

static void profile_save(const char* path) {
    FILE* file = fopen(path, "w");
    if (file == nullptr)
        die("Can't write %s", path);
    std::vector<u32> order;
    for (u32 i = 0; i < PROFILE_SIZE; i++)
        if (profile[i] != 0)
            order.push_back(i);
    std::sort(order.begin(), order.end(), [](u32 a, u32 b) { return profile[a] > profile[b]; });
    fprintf(file, "# frames %llu busy %.1f%%\n", (unsigned long long)profile_frames,
            profile_frames ? 100.0 * profile_total() / (profile_frames * frame_cycles()) : 0.0);
    for (u32 index : order) {
        u32 address = index < PROFILE_MAIN_SIZE                       ? 0x02000000 + index * 2
                      : index < PROFILE_MAIN_SIZE + PROFILE_ITCM_SIZE ? 0x01ff8000 + (index - PROFILE_MAIN_SIZE) * 2
                                                                      : 0;
        fprintf(file, "%08x %u\n", address, profile[index]);
    }
    fclose(file);
}

static void put_u32(u8* out, u32 value) {
    out[0] = value >> 24;
    out[1] = value >> 16;
    out[2] = value >> 8;
    out[3] = value;
}

static void write_chunk(FILE* file, const char* type, const u8* data, u32 size) {
    u8 header[8];
    put_u32(header, size);
    memcpy(header + 4, type, 4);
    u32 crc = crc32(crc32(0, header + 4, 4), data, size);
    u8 footer[4];
    put_u32(footer, crc);
    fwrite(header, 1, 8, file);
    fwrite(data, 1, size, file);
    fwrite(footer, 1, 4, file);
}

// Writes the screens as an 8-bit RGB PNG, every row with filter 0 before its pixels
static void shot(const char* path) {
    void* screens[2];
    if (!nds->GPU.GetFramebuffers(&screens[0], &screens[1]))
        die("No framebuffers");
    static u8 raw[HEIGHT * (1 + WIDTH * 3)];
    u8* out = raw;
    for (int y = 0; y < HEIGHT; y++) {
        const u32* pixels = (const u32*)screens[y / (HEIGHT / 2)] + (y % (HEIGHT / 2)) * WIDTH;
        *out++ = 0;
        for (int x = 0; x < WIDTH; x++) {
            u32 pixel = pixels[x];
            *out++ = pixel >> 16;
            *out++ = pixel >> 8;
            *out++ = pixel;
        }
    }
    uLongf size = compressBound(sizeof(raw));
    std::vector<u8> compressed(size);
    if (compress2(compressed.data(), &size, raw, sizeof(raw), 9) != Z_OK)
        die("Can't compress %s", path);

    FILE* file = fopen(path, "wb");
    if (file == nullptr)
        die("Can't write %s", path);
    u8 header[13] = {0};
    put_u32(header, WIDTH);
    put_u32(header + 4, HEIGHT);
    header[8] = 8;  // bits per channel
    header[9] = 2;  // RGB
    fwrite("\x89PNG\r\n\x1a\n", 1, 8, file);
    write_chunk(file, "IHDR", header, sizeof(header));
    write_chunk(file, "IDAT", compressed.data(), size);
    write_chunk(file, "IEND", nullptr, 0);
    fclose(file);
}

static std::vector<u8> read_file(const char* path) {
    FILE* file = fopen(path, "rb");
    if (file == nullptr)
        die("Can't open %s", path);
    fseeko(file, 0, SEEK_END);
    std::vector<u8> data(ftello(file));
    rewind(file);
    if (fread(data.data(), 1, data.size(), file) != data.size())
        die("Can't read %s", path);
    fclose(file);
    return data;
}

static void save_state(const char* path) {
    Savestate state;
    FILE* file = fopen(path, "wb");
    if (file == nullptr || state.Error || !nds->DoSavestate(&state) || state.Error ||
        fwrite(state.Buffer(), 1, state.Length(), file) != state.Length())
        die("Can't save savestate %s", path);
    fclose(file);
}

static void load_state(const char* path) {
    std::vector<u8> buffer = read_file(path);
    Savestate state(buffer.data(), buffer.size(), false);
    if (state.Error || !nds->DoSavestate(&state) || state.Error)
        die("Can't load savestate %s", path);
}

// Runs one command from its words, false on quit
static bool command(char** words) {
    const char* name = words[0];
    if (strcmp(name, "wait") == 0) {
        run(parse_number(words[1]));
    } else if (strcmp(name, "press") == 0) {
        u32 keys = parse_keys(words[1]);
        held_keys |= keys;
        run(words[2] != nullptr ? parse_number(words[2]) : 6);
        held_keys &= ~keys;
        run(1);
    } else if (strcmp(name, "hold") == 0) {
        held_keys |= parse_keys(words[1]);
        run(1);
    } else if (strcmp(name, "release") == 0) {
        held_keys = 0;
        nds->ReleaseScreen();
        run(1);
    } else if (strcmp(name, "touch") == 0) {
        int x = parse_number(words[1]), y = parse_number(words[2]);
        nds->TouchScreen(x, y);
        run(words[3] != nullptr ? parse_number(words[3]) : 6);
        nds->ReleaseScreen();
        run(1);
    } else if (strcmp(name, "shot") == 0) {
        if (words[1] == nullptr)
            die("Expected a file");
        shot(words[1]);
    } else if (strcmp(name, "save") == 0 || strcmp(name, "load") == 0) {
        if (words[1] == nullptr)
            die("Expected a file");
        name[0] == 's' ? save_state(words[1]) : load_state(words[1]);
    } else if (strcmp(name, "hook") == 0) {
        // Thumb function symbols have bit 0 set, the instruction is at the even address
        hook_address = parse_address(words[1]) & ~1u;
        executions = 0;
        update_instruction_hook();
    } else if (strcmp(name, "count") == 0) {
        fprintf(answers, "count %u\n", executions);
        fflush(answers);
        executions = 0;
    } else if (strcmp(name, "profile") == 0 && words[1] != nullptr && strcmp(words[1], "start") == 0) {
        profile.assign(PROFILE_SIZE, 0);
        profile_frames = busy_cycles = busy_frames = 0;
        update_instruction_hook();
    } else if (strcmp(name, "profile") == 0 && words[1] != nullptr && strcmp(words[1], "save") == 0) {
        if (profile.empty())
            die("No profile started");
        if (words[2] == nullptr)
            die("Expected a file");
        profile_save(words[2]);
    } else if (strcmp(name, "profile") == 0 && words[1] != nullptr && strcmp(words[1], "stop") == 0) {
        profile.clear();
        profile.shrink_to_fit();
        update_instruction_hook();
    } else if (strcmp(name, "busy") == 0) {
        if (profile.empty())
            die("No profile started");
        u64 total = profile_total(), frames = profile_frames - busy_frames;
        fprintf(answers, "busy %.1f\n", frames ? 100.0 * (total - busy_cycles) / (frames * frame_cycles()) : 0.0);
        fflush(answers);
        busy_cycles = total;
        busy_frames = profile_frames;
    } else if (strcmp(name, "wav") == 0) {
        if (words[1] == nullptr)
            die("Expected a file");
        wav_end();
        if (strcmp(words[1], "stop") != 0) {
            if ((wav = fopen(words[1], "wb")) == nullptr)
                die("Can't write %s", words[1]);
            wav_samples = 0;
            wav_header();
        }
    } else if (strcmp(name, "peek") == 0) {
        u32 address = parse_address(words[1]);
        int count = words[2] != nullptr ? parse_number(words[2]) : 1;
        fprintf(answers, "peek %08x", address);
        for (int i = 0; i < count; i++)
            fprintf(answers, " %08x", nds->ARM9Read32(address + i * 4));
        fprintf(answers, "\n");
        fflush(answers);
    } else if (strcmp(name, "test") == 0) {
        u32 address = parse_address(words[1]);
        if (address < 0x02000000 || address > 0x023fbffc || (address & 3))
            die("Expected a test results address in main RAM");
        int frames = parse_number(words[2]);
        u32 status = 0;
        for (int i = 0; i < frames && status == 0; i++) {
            run(1);
            status = nds->ARM9Read32(address);
        }
        for (u32 i = 0; i < 16 * 1024; i++) {
            u8 c = nds->ARM9Read8(address + 4 + i);
            if (c == 0)
                break;
            fputc(c, answers);
        }
        fflush(answers);
        if (status != 1)
            die(status == 0 ? "Test timed out after %d frames" : "Test failed (status %d)",
                status == 0 ? frames : (int)status);
    } else if (strcmp(name, "pak") == 0) {
        if (nds->ConsoleType != 0)
            die("The DSi has no GBA slot");
        nds->SetGBACart(std::make_unique<GBACart::CartRAMExpansion>());
    } else if (strcmp(name, "quit") == 0) {
        return false;
    } else {
        die("Unknown command '%s'", name);
    }
    return true;
}

// Splits a command line in words and runs it, false on quit, empty lines and # comments are skipped
static bool line(char* text) {
    char* words[5] = {0};
    int count = 0;
    char* save;
    for (char* word = strtok_r(text, " \t\r\n", &save); word != nullptr && count < 4;
         word = strtok_r(nullptr, " \t\r\n", &save))
        words[count++] = word;
    return count == 0 || words[0][0] == '#' || command(words);
}

static void remove_temp_dir() {
    std::error_code error;
    std::filesystem::remove_all(temp_dir, error);
}

// Starts from the DSi's SD card like a homebrew loader: melonDS boots the ROM like the DSi menu
// boots a card, which turns the SD card off for the ARM7, so it gets turned on again (SCFG_EXT7 bit
// 18), and the homebrew argv points to sd:/<rom>, like melonDS does with fat:/<rom> for an R4 card,
// the command line goes after the ARM9 program
static void setup_sd_boot(const std::string& rom_name) {
    static_cast<DSi*>(nds)->SCFG_EXT[1] |= 1 << 18;
    std::string argv = "sd:/" + rom_name;
    const NDSHeader& header = nds->NDSCartSlot.GetCart()->GetHeader();
    u32 base = (header.ARM9RAMAddress + header.ARM9Size + 15) & ~15u;
    for (u32 i = 0; i <= argv.size(); i++)
        nds->ARM9Write8(base + i, argv.c_str()[i]);
    nds->ARM9Write32(ARGV_MAGIC_ADDRESS, ARGV_MAGIC);
    nds->ARM9Write32(ARGV_MAGIC_ADDRESS + 4, base);
    nds->ARM9Write32(ARGV_MAGIC_ADDRESS + 8, argv.size() + 1);
}

int main(int argc, char** argv) {
    const char* program = argv[0];
    // The answers go to stdout, what melonDS prints itself goes to stderr
    answers = fdopen(dup(STDOUT_FILENO), "w");
    dup2(STDERR_FILENO, STDOUT_FILENO);
    const char* fat_directory = nullptr;
    bool dsi = false;
    for (;;) {
        if (argc >= 2 && strcmp(argv[1], "--dsi") == 0) {
            dsi = true;
            argc--;
            argv++;
        } else if (argc >= 3 && strcmp(argv[1], "--fat") == 0) {
            fat_directory = argv[2];
            argc -= 2;
            argv += 2;
        } else {
            break;
        }
    }
    if (argc < 2) {
        fprintf(stderr, "Usage: %s [--dsi] [--fat <directory>] <program.nds> ['command args']...\n", program);
        return 1;
    }

    std::vector<u8> rom = read_file(argv[1]);

    // The SD card's image goes to a temporary directory. The NDS is never destroyed, which would
    // sync the card's writes back to the directory, so they stay in emulated memory
    std::string temp_template = (std::filesystem::temp_directory_path() / "melonds-runner.XXXXXX").string();
    if (mkdtemp(temp_template.data()) == nullptr)
        die("Can't create a temporary directory");
    temp_dir = temp_template;
    atexit(remove_temp_dir);
    std::optional<FATStorageArgs> card;
    if (fat_directory != nullptr)
        card = FATStorageArgs{"sd.bin", 0, false, std::filesystem::absolute(fat_directory).string()};

    auto cart =
        NDSCart::ParseROM(rom.data(), rom.size(), nullptr, NDSCart::NDSCartArgs{dsi ? std::nullopt : card, nullptr, 0});
    if (cart == nullptr)
        die("Can't load %s", argv[1]);

    // The free BIOSes and generated firmware, no JIT, so the emulation is frame exact
    if (dsi) {
        DSiArgs args;
        args.Firmware = Firmware(1);
        args.JIT = std::nullopt;
        args.OutputSampleRate = SAMPLE_RATE;
        args.ARM9iBIOS = std::make_unique<DSiBIOSImage>(FreeBIOSGetTwlArm9());
        args.ARM7iBIOS = std::make_unique<DSiBIOSImage>(FreeBIOSGetTwlArm7());
        if (card)
            args.DSiSDCard = FATStorage(*card);
        nds = new DSi(std::move(args));
    } else {
        NDSArgs args;
        args.JIT = std::nullopt;
        args.OutputSampleRate = SAMPLE_RATE;
        nds = new NDS(std::move(args));
    }
    // Reset before the card goes in, which applies its DLDI patch, again would patch it twice
    nds->Reset();
    nds->SetNDSCart(std::move(cart));
    std::string rom_name = std::filesystem::path(argv[1]).filename().string();
    nds->SetupDirectBoot(rom_name);
    if (dsi && card)
        setup_sd_boot(rom_name);
    time_t now = time(nullptr);
    struct tm* local = localtime(&now);
    nds->RTC.SetDateTime(local->tm_year + 1900, local->tm_mon + 1, local->tm_mday, local->tm_hour, local->tm_min,
                         local->tm_sec);
    nds->Start();

    if (argc > 2) {
        for (int i = 2; i < argc && line(argv[i]); i++) {}
    } else {
        char text[1024];
        while (fgets(text, sizeof(text), stdin) != nullptr && line(text)) {
            fprintf(answers, "ok\n");
            fflush(answers);
        }
    }
    wav_end();
    return 0;
}
