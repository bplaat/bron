// Runs a Wii program headless in Dolphin and does what the commands say, field exact, so a script
// or an agent can play a program and take screenshots. The commands come from the arguments, or
// from stdin one per line when there are none, then it answers each with ok. What the program
// writes to the debug UART goes to stderr.
//
// Usage: dolphin-runner [--sd <directory>] <program.elf|dol> ['command args']...
//   --sd <directory>          put an SD card with the files of the directory in the SD card slot,
//                             its writes stay in a temporary card image
//   await <text> <frames>     run until a UART line after the last awaited one contains text
//   test <frames>             await "DONE <n> failed" on the UART, fail on failures or timeout
//   wait <frames>             run frames (60 a second, VI fields)
//   press <keys> [frames]     hold keys for frames (default 6), then release them
//   hold <keys>               hold keys until release
//   release                   release all keys and center the Nunchuk stick
//   point <x> <y>             point the Wii Remote at a pixel of the 640x480 screen
//   stick <x> <y>             tilt the Nunchuk stick, -100 to 100 (right and up are positive)
//   shot <file.png>           write the frame the video interface shows
//   save <file>, load <file>  save or load a savestate
//   hook <address>            count the executions of the code at address, like a function
//   count                     print "count <executions>" since hook or the last count
//   wav <file>, wav stop      record the sound of the DSP to a WAV file, stop recording
//   peek <address> [words]    print "peek <address> <word>..." of the memory, in hex
//   quit                      stop, also the end of the commands
// Keys are the Wii Remote's a, b, 1, 2, plus, minus, home, up, down, left and right and the
// Nunchuk's c and z, joined by +, like up+a.

#include <unistd.h>

#include <atomic>
#include <chrono>
#include <climits>
#include <condition_variable>
#include <cstdarg>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <filesystem>
#include <mutex>
#include <optional>
#include <string>
#include <string_view>
#include <thread>
#include <vector>

#include "AudioCommon/Mixer.h"
#include "AudioCommon/SoundStream.h"
#include "Common/Image.h"
#include "Common/Logging/LogManager.h"
#include "Common/WindowSystemInfo.h"
#include "Core/Boot/Boot.h"
#include "Core/BootManager.h"
#include "Core/Config/GraphicsSettings.h"
#include "Core/Config/MainSettings.h"
#include "Core/Config/WiimoteSettings.h"
#include "Core/ConfigManager.h"
#include "Core/Core.h"
#include "Core/HW/Memmap.h"
#include "Core/HW/VideoInterface.h"
#include "Core/HW/Wiimote.h"
#include "Core/HW/WiimoteEmu/Extension/Nunchuk.h"
#include "Core/HW/WiimoteEmu/WiimoteEmu.h"
#include "Core/Host.h"
#include "Core/PowerPC/BreakPoints.h"
#include "Core/PowerPC/MMU.h"
#include "Core/PowerPC/PowerPC.h"
#include "Core/State.h"
#include "Core/System.h"
#include "InputCommon/ControllerEmu/ControlGroup/Attachments.h"
#include "InputCommon/ControllerEmu/StickGate.h"
#include "InputCommon/InputConfig.h"
#include "UICommon/UICommon.h"
#include "VideoCommon/TextureDecoder.h"

// The clock's start, 2026-01-01 00:00:00 UTC, in seconds since 1970
#define RTC_START 1767225600

// The screen that point coordinates are in
#define SCREEN_WIDTH 640
#define SCREEN_HEIGHT 480

struct Key {
    const char* name;
    bool nunchuk;
    const char* group;
    const char* control;
};

static const Key keys[] = {
    {"a", false, WiimoteEmu::Wiimote::BUTTONS_GROUP, WiimoteEmu::Wiimote::A_BUTTON},
    {"b", false, WiimoteEmu::Wiimote::BUTTONS_GROUP, WiimoteEmu::Wiimote::B_BUTTON},
    {"1", false, WiimoteEmu::Wiimote::BUTTONS_GROUP, WiimoteEmu::Wiimote::ONE_BUTTON},
    {"2", false, WiimoteEmu::Wiimote::BUTTONS_GROUP, WiimoteEmu::Wiimote::TWO_BUTTON},
    {"plus", false, WiimoteEmu::Wiimote::BUTTONS_GROUP, WiimoteEmu::Wiimote::PLUS_BUTTON},
    {"minus", false, WiimoteEmu::Wiimote::BUTTONS_GROUP, WiimoteEmu::Wiimote::MINUS_BUTTON},
    {"home", false, WiimoteEmu::Wiimote::BUTTONS_GROUP, WiimoteEmu::Wiimote::HOME_BUTTON},
    {"up", false, WiimoteEmu::Wiimote::DPAD_GROUP, "Up"},
    {"down", false, WiimoteEmu::Wiimote::DPAD_GROUP, "Down"},
    {"left", false, WiimoteEmu::Wiimote::DPAD_GROUP, "Left"},
    {"right", false, WiimoteEmu::Wiimote::DPAD_GROUP, "Right"},
    {"c", true, WiimoteEmu::Nunchuk::BUTTONS_GROUP, WiimoteEmu::Nunchuk::C_BUTTON},
    {"z", true, WiimoteEmu::Nunchuk::BUTTONS_GROUP, WiimoteEmu::Nunchuk::Z_BUTTON},
};
static constexpr size_t KEY_COUNT = sizeof(keys) / sizeof(keys[0]);

// The input, written by the commands while the emulation is paused and read by the CPU thread
static std::atomic<u32> held_keys;
static std::atomic<bool> pointing;
static std::atomic<double> point_x, point_y, stick_x, stick_y;

static std::filesystem::path temp_dir;
static FILE* answers;
static std::mutex uart_mutex;
static std::vector<std::string> uart_lines;
static u32 hook_address;
static bool hooked;

static void remove_temp_dir() {
    std::error_code error;
    std::filesystem::remove_all(temp_dir, error);
}

// Stops without shutting the emulation down, which could sync the SD card back to its directory
[[noreturn]] static void die(const char* format, ...) {
    va_list args;
    va_start(args, format);
    fprintf(stderr, "dolphin-runner: ");
    vfprintf(stderr, format, args);
    fputc('\n', stderr);
    va_end(args);
    remove_temp_dir();
    _exit(1);
}

// The frontend functions Dolphin's core calls, there is no window or user interface
std::vector<std::string> Host_GetPreferredLocales() {
    return {};
}
bool Host_UIBlocksControllerState() {
    return false;
}
bool Host_RendererHasFocus() {
    return true;
}
bool Host_RendererHasFullFocus() {
    return true;
}
bool Host_RendererIsFullscreen() {
    return false;
}
bool Host_TASInputHasFocus() {
    return false;
}
void Host_Message(HostMessageID) {}
void Host_PPCSymbolsChanged() {}
void Host_PPCBreakpointsChanged() {}
void Host_RequestRenderWindowSize(int, int) {}
void Host_UpdateDisasmDialog() {}
void Host_JitCacheInvalidation() {}
void Host_JitProfileDataWiped() {}
void Host_UpdateTitle(const std::string&) {}
void Host_YieldToUI() {}
void Host_TitleChanged() {}
void Host_UpdateDiscordClientID(const std::string&) {}
bool Host_UpdateDiscordPresenceRaw(const std::string&, const std::string&, const std::string&, const std::string&,
                                   const std::string&, const std::string&, const int64_t, const int64_t, const int,
                                   const int) {
    return false;
}
std::unique_ptr<GBAHostInterface> Host_CreateGBAHost(std::weak_ptr<HW::GBA::Core>) {
    return nullptr;
}

// Writes the debug UART's lines, which Dolphin logs as OSREPORT, to stderr and keeps them for test
class UartListener : public Common::Log::LogListener {
   public:
    void Log(Common::Log::LogLevel, const char* message) override {
        const char* text = strstr(message, "[OSREPORT]: ");
        if (text == nullptr)
            return;
        text += strlen("[OSREPORT]: ");
        std::lock_guard lock(uart_mutex);
        fputs(text, stderr);
        uart_lines.emplace_back(text);
    }
};

static std::optional<ControlState> override_input(bool nunchuk, std::string_view group, std::string_view control) {
    if (nunchuk && group == WiimoteEmu::Nunchuk::STICK_GROUP)
        return control == ControllerEmu::ReshapableInput::X_INPUT_OVERRIDE ? stick_x.load() : stick_y.load();
    if (!nunchuk && group == WiimoteEmu::Wiimote::IR_GROUP) {
        if (!pointing)
            return std::nullopt;
        return control == ControllerEmu::ReshapableInput::X_INPUT_OVERRIDE ? point_x.load() : point_y.load();
    }
    for (size_t i = 0; i < KEY_COUNT; i++)
        if (keys[i].nunchuk == nunchuk && group == keys[i].group && control == keys[i].control)
            return held_keys & (1u << i) ? 1.0 : 0.0;
    return std::nullopt;
}

// Gives the first Wii Remote a Nunchuk and takes all their input from the commands
static void setup_input() {
    auto* wiimote = static_cast<WiimoteEmu::Wiimote*>(Wiimote::GetConfig()->GetController(0));
    auto* attachments =
        static_cast<ControllerEmu::Attachments*>(wiimote->GetWiimoteGroup(WiimoteEmu::WiimoteGroup::Attachments));
    attachments->SetSelectedAttachment(WiimoteEmu::ExtensionNumber::NUNCHUK);
    wiimote->SetInputOverrideFunction([](std::string_view group, std::string_view control, ControlState) {
        return override_input(false, group, control);
    });
    attachments->GetAttachmentList()[WiimoteEmu::ExtensionNumber::NUNCHUK]->SetInputOverrideFunction(
        [](std::string_view group, std::string_view control, ControlState) {
            return override_input(true, group, control);
        });
}

static int parse_int(const char* text, long min, long max) {
    char* end;
    long value = text != nullptr ? strtol(text, &end, 10) : 0;
    if (text == nullptr || *text == '\0' || *end != '\0' || value < min || value > max)
        die("Expected a number from %ld to %ld, got '%s'", min, max, text != nullptr ? text : "nothing");
    return (int)value;
}

static int parse_number(const char* text) {
    return parse_int(text, 0, INT_MAX);
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
    u32 mask = 0;
    std::string_view rest = text;
    while (!rest.empty()) {
        std::string_view name = rest.substr(0, rest.find('+'));
        rest.remove_prefix(std::min(rest.size(), name.size() + 1));
        size_t i = 0;
        while (i < KEY_COUNT && name != keys[i].name)
            i++;
        if (i == KEY_COUNT)
            die("Unknown key '%.*s'", (int)name.size(), name.data());
        mask |= 1u << i;
    }
    return mask;
}

static Core::System& system() {
    return Core::System::GetInstance();
}

// Waits until the emulation is paused, Dolphin's jobs for the host run meanwhile
static void await_pause() {
    while (Core::GetState(system()) != Core::State::Paused) {
        if (Core::IsUninitialized(system()) || Core::GetState(system()) == Core::State::Stopping)
            die("The emulation stopped");
        Core::HostDispatchJobs(system());
        std::this_thread::sleep_for(std::chrono::microseconds(100));
    }
}

static void run(int frames) {
    for (int i = 0; i < frames; i++) {
        Core::DoFrameStep(system());
        await_pause();
    }
}

// Runs a function on the CPU thread and waits for it, the emulation stays paused
static void on_cpu_thread(std::function<void()> function) {
    std::mutex mutex;
    std::condition_variable done_condition;
    bool done = false;
    Core::RunOnCPUThread(system(), [&] {
        function();
        std::lock_guard lock(mutex);
        done = true;
        done_condition.notify_one();
    });
    std::unique_lock lock(mutex);
    done_condition.wait(lock, [&] { return done; });
}

// Writes the XFB the video interface showed last from RAM as a PNG
static void shot(const char* path) {
    const VideoInterface::VideoInterfaceManager::XFBInfo& xfb = system().GetVideoInterface().GetLastXFB();
    if (xfb.address == 0 || xfb.width == 0 || xfb.height == 0)
        die("No frame shown yet");
    std::vector<u8> yuyv(size_t(xfb.stride) * xfb.height), rgba(size_t(xfb.width) * xfb.height * 4);
    system().GetMemory().CopyFromEmu(yuyv.data(), xfb.address, yuyv.size());
    TexDecoder_DecodeXFB(rgba.data(), yuyv.data(), xfb.width, xfb.height, xfb.stride);
    if (!Common::SavePNG(path, rgba.data(), Common::ImageByteFormat::RGBA, xfb.width, xfb.height, xfb.width * 4))
        die("Can't write %s", path);
}

static void save_state(const char* path) {
    std::filesystem::remove(path);
    on_cpu_thread([path] { State::SaveAs(system(), path); });
    State::WaitForSavesWritten();
    if (!std::filesystem::exists(path))
        die("Can't save savestate %s", path);
}

static void load_state(const char* path) {
    if (!std::filesystem::exists(path))
        die("Can't read savestate %s", path);
    on_cpu_thread([path] { State::LoadAs(system(), path); });
}

static u64 hook_hits() {
    const TBreakPoint* breakpoint = system().GetPowerPC().GetBreakPoints().GetBreakpoint(hook_address);
    return breakpoint != nullptr ? breakpoint->hit_count : 0;
}

// Runs frames until a UART line after the one the last await found contains text, returns it
static std::string await_uart(const char* text, int frames) {
    static size_t searched;
    for (int i = 0; i <= frames; i++) {
        {
            std::lock_guard lock(uart_mutex);
            for (; searched < uart_lines.size(); searched++)
                if (uart_lines[searched].find(text) != std::string::npos)
                    return uart_lines[searched++];
        }
        if (i < frames)
            run(1);
    }
    die("No '%s' on the UART after %d frames", text, frames);
}

// Runs one command from its words, false on quit
static bool command(char** words) {
    const char* name = words[0];
    if (strcmp(name, "wait") == 0) {
        run(parse_number(words[1]));
    } else if (strcmp(name, "press") == 0) {
        u32 mask = parse_keys(words[1]);
        held_keys |= mask;
        run(words[2] != nullptr ? parse_number(words[2]) : 6);
        held_keys &= ~mask;
        run(1);
    } else if (strcmp(name, "hold") == 0) {
        held_keys |= parse_keys(words[1]);
        run(1);
    } else if (strcmp(name, "release") == 0) {
        held_keys = 0;
        stick_x = stick_y = 0.0;
        run(1);
    } else if (strcmp(name, "point") == 0) {
        int x = parse_int(words[1], 0, SCREEN_WIDTH - 1), y = parse_int(words[2], 0, SCREEN_HEIGHT - 1);
        // Dolphin's pointer goes from -1 to 1, up is positive
        point_x = x * 2.0 / (SCREEN_WIDTH - 1) - 1.0;
        point_y = 1.0 - y * 2.0 / (SCREEN_HEIGHT - 1);
        pointing = true;
        run(1);
    } else if (strcmp(name, "stick") == 0) {
        stick_x = parse_int(words[1], -100, 100) / 100.0;
        stick_y = parse_int(words[2], -100, 100) / 100.0;
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
        u32 address = parse_address(words[1]);
        on_cpu_thread([address] {
            BreakPoints& breakpoints = system().GetPowerPC().GetBreakPoints();
            if (hooked)
                breakpoints.Remove(hook_address);
            breakpoints.Add(address, false, false, std::nullopt);
        });
        hook_address = address;
        hooked = true;
    } else if (strcmp(name, "count") == 0) {
        if (!hooked)
            die("No hook set");
        // The breakpoint counts on, so count gives the hits since the last count
        static u64 counted;
        u64 hits = hook_hits();
        fprintf(answers, "count %llu\n", (unsigned long long)(hits - counted));
        fflush(answers);
        counted = hits;
    } else if (strcmp(name, "wav") == 0) {
        if (words[1] == nullptr)
            die("Expected a file");
        Mixer* mixer = system().GetSoundStream()->GetMixer();
        mixer->StopLogDSPAudio();
        if (strcmp(words[1], "stop") != 0)
            mixer->StartLogDSPAudio(words[1]);
    } else if (strcmp(name, "peek") == 0) {
        u32 address = parse_address(words[1]);
        int count = words[2] != nullptr ? parse_number(words[2]) : 1;
        Core::CPUThreadGuard guard(system());
        fprintf(answers, "peek %08x", address);
        for (int i = 0; i < count; i++) {
            auto word = PowerPC::MMU::HostTryRead<u32>(guard, address + i * 4);
            if (!word)
                die("Can't read %08x", address + i * 4);
            fprintf(answers, " %08x", word->value);
        }
        fprintf(answers, "\n");
        fflush(answers);
    } else if (strcmp(name, "await") == 0) {
        if (words[1] == nullptr)
            die("Expected a text");
        await_uart(words[1], parse_number(words[2]));
    } else if (strcmp(name, "test") == 0) {
        int failures = 0;
        sscanf(await_uart("DONE ", parse_number(words[1])).c_str(), "DONE %d failed", &failures);
        if (failures != 0)
            die("Test failed (%d failed)", failures);
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

// No GPU, JIT, sound output or user interface: the cached interpreter, which runs decoded
// instructions without generating code, and the software renderer, which copies the XFB to RAM
// for shot, on one thread, as fast as it goes, with a fixed clock
static void configure(const char* sd_directory) {
    Config::SetCurrent(Config::MAIN_CPU_CORE, PowerPC::CPUCore::CachedInterpreter);
    Config::SetCurrent(Config::MAIN_CPU_THREAD, false);
    Config::SetCurrent(Config::MAIN_GFX_BACKEND, std::string("Software Renderer"));
    Config::SetCurrent(Config::GFX_HACK_SKIP_XFB_COPY_TO_RAM, false);
    Config::SetCurrent(Config::MAIN_EMULATION_SPEED, 0.0f);
    Config::SetCurrent(Config::MAIN_DSP_HLE, true);
    Config::SetCurrent(Config::MAIN_AUDIO_BACKEND, std::string(BACKEND_NULLSOUND));
    Config::SetCurrent(Config::MAIN_USE_PANIC_HANDLERS, false);
    Config::SetCurrent(Config::MAIN_OSD_MESSAGES, false);
    Config::SetCurrent(Config::MAIN_ENABLE_DEBUGGING, true);
    Config::SetCurrent(Config::MAIN_BLUETOOTH_PASSTHROUGH_ENABLED, false);
    Config::SetCurrent(Config::MAIN_WIIMOTE_CONTINUOUS_SCANNING, false);
    Config::SetCurrent(Config::WIIMOTE_1_SOURCE, WiimoteSource::Emulated);
    Config::SetCurrent(Config::MAIN_WII_SD_CARD, sd_directory != nullptr);
    if (sd_directory != nullptr) {
        // Dolphin builds a card image from the directory when it boots
        Config::SetCurrent(Config::MAIN_WII_SD_CARD_ENABLE_FOLDER_SYNC, true);
        Config::SetCurrent(Config::MAIN_WII_SD_CARD_SYNC_FOLDER_PATH, std::filesystem::absolute(sd_directory).string());
        Config::SetCurrent(Config::MAIN_WII_SD_CARD_IMAGE_PATH, (temp_dir / "sd.raw").string());
        Config::SetCurrent(Config::MAIN_ALLOW_SD_WRITES, true);
    }
    // The clock always starts at the same time, so programs that use it run the same every time
    Config::SetCurrent(Config::MAIN_CUSTOM_RTC_ENABLE, true);
    Config::SetCurrent(Config::MAIN_CUSTOM_RTC_VALUE, RTC_START);
    SConfig::GetInstance().bBootToPause = true;
    Core::SetFrameStepFields(true);
}

// Only the debug UART is logged, to the listener
static void setup_logging() {
    auto* log_manager = Common::Log::LogManager::GetInstance();
    for (int i = 0; i < Common::Log::LogListener::NUMBER_OF_LISTENERS; i++)
        log_manager->EnableListener(static_cast<Common::Log::LogListener::LISTENER>(i), false);
    for (int type = 0; type < static_cast<int>(Common::Log::LogType::NUMBER_OF_LOGS); type++)
        log_manager->SetEnable(static_cast<Common::Log::LogType>(type), false);
    log_manager->SetEnable(Common::Log::LogType::OSREPORT, true);
    log_manager->SetConfigLogLevel(Common::Log::LogLevel::LNOTICE);
    log_manager->RegisterListener(Common::Log::LogListener::LOG_WINDOW_LISTENER, std::make_unique<UartListener>());
    log_manager->EnableListener(Common::Log::LogListener::LOG_WINDOW_LISTENER, true);
}

int main(int argc, char** argv) {
    const char* program = argv[0];
    // The answers go to stdout, what Dolphin prints itself goes to stderr
    answers = fdopen(dup(STDOUT_FILENO), "w");
    dup2(STDERR_FILENO, STDOUT_FILENO);
    const char* sd_directory = nullptr;
    if (argc >= 3 && strcmp(argv[1], "--sd") == 0) {
        sd_directory = argv[2];
        argc -= 2;
        argv += 2;
    }
    if (argc < 2) {
        fprintf(stderr, "Usage: %s [--sd <directory>] <program.elf|dol> ['command args']...\n", program);
        return 1;
    }
    if (sd_directory != nullptr && !std::filesystem::is_directory(sd_directory))
        die("No directory %s", sd_directory);

    // A fresh user directory for every run, so nothing of an earlier run or the Dolphin app counts
    std::string temp_template = (std::filesystem::temp_directory_path() / "dolphin-runner.XXXXXX").string();
    if (mkdtemp(temp_template.data()) == nullptr)
        die("Can't create a temporary directory");
    temp_dir = temp_template;

    UICommon::SetUserDirectory((temp_dir / "user").string());
    UICommon::Init();
    setup_logging();
    configure(sd_directory);
    const WindowSystemInfo wsi;
    UICommon::InitControllers(wsi);
    setup_input();

    auto boot = BootParameters::GenerateFromFile(std::filesystem::absolute(argv[1]).string());
    if (boot == nullptr || !BootManager::BootCore(system(), std::move(boot), wsi))
        die("Can't boot %s", argv[1]);
    await_pause();

    if (argc > 2) {
        for (int i = 2; i < argc && line(argv[i]); i++) {}
    } else {
        char text[1024];
        while (fgets(text, sizeof(text), stdin) != nullptr && line(text)) {
            fprintf(answers, "ok\n");
            fflush(answers);
        }
    }

    // The card's writes stay in its image, which goes with the temporary directory
    system().GetSoundStream()->GetMixer()->StopLogDSPAudio();
    Config::SetCurrent(Config::MAIN_ALLOW_SD_WRITES, false);
    Core::Stop(system());
    while (!Core::IsUninitialized(system())) {
        Core::HostDispatchJobs(system());
        std::this_thread::sleep_for(std::chrono::milliseconds(1));
    }
    Core::Shutdown(system());
    UICommon::ShutdownControllers();
    UICommon::Shutdown();
    remove_temp_dir();
    return 0;
}
