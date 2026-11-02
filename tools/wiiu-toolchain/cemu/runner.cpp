// Headless frontend for Cemu's Cafe OS: runs an RPX on Cemu's interpreter with its own coreinit,
// RPL loader and FSA, without the GPU, audio, input, JIT or GUI. It replaces Cemu's CafeSystem.cpp
// and stubs what the OS emulation calls of the rest of Cemu.
//
// Usage: cemu-runner <program.rpx> [test | run] [seconds] [user folder]
// The program's OSReport lines go to stderr. A test finishes at a "DONE <n> failed" line and fails
// when n isn't 0, a run returns the program's exit status. Both fail when the program doesn't
// finish in time (60 seconds by default). The user folder (cemu-runner in the temporary folder by
// default) holds the SD card in sdcard/. Set CEMU_RUNNER_LOG to also print Cemu's log.
#include <execinfo.h>
#include <unistd.h>

#include <csignal>

#include "Cafe/CafeSystem.h"
#include "Cafe/Filesystem/fsc.h"
#include "Cafe/GraphicPack/GraphicPack2.h"
#include "Cafe/HW/Espresso/Debugger/Debugger.h"
#include "Cafe/HW/Espresso/Debugger/GDBBreakpoints.h"
#include "Cafe/HW/Espresso/Debugger/GDBStub.h"
#include "Cafe/HW/Espresso/PPCState.h"
#include "Cafe/HW/Espresso/Recompiler/PPCRecompiler.h"
#include "Cafe/HW/Latte/Core/Latte.h"
#include "Cafe/HW/Latte/Core/LatteBufferCache.h"
#include "Cafe/HW/MMU/MMU.h"
#include "Cafe/IOSU/fsa/iosu_fsa.h"
#include "Cafe/IOSU/kernel/iosu_kernel.h"
#include "Cafe/IOSU/legacy/iosu_ioctl.h"
#include "Cafe/OS/RPL/COSModule.h"
#include "Cafe/OS/RPL/rpl.h"
#include "Cafe/OS/RPL/rpl_symbol_storage.h"
#include "Cafe/OS/common/OSCommon.h"
#include "Cafe/OS/libs/coreinit/coreinit.h"
#include "Cafe/OS/libs/coreinit/coreinit_Thread.h"
#include "Cafe/OS/libs/nlibnss/nlibnss.h"
#include "Cafe/OS/libs/nn_act/nn_act.h"
#include "Cafe/TitleList/TitleInfo.h"
#include "Common/SysAllocator.h"
#include "WindowSystem.h"
#include "config/ActiveSettings.h"
#include "config/CemuConfig.h"
#include "util/helpers/helpers.h"

// The globals of CafeSystem.cpp and the default settings, which are never loaded or saved
MPTR _entryPoint;
std::string _pathToExecutable;
RPLModule* applicationRPX;
LatteGPUState_t LatteGPUState = {};
std::unique_ptr<GDBServer> g_gdbstub;
XMLCemuConfig_t g_config;
uint64 s_loggingFlagMask = cemuLog_getFlag(LogType::Force) | cemuLog_getFlag(LogType::CoreinitLogging);

// The program's finish: the failures of a test or the exit status of a run
static std::atomic<int> s_failures{-1}, s_exitStatus{-1};

// OSReport lines come as "[OSConsole] <line>"
bool cemuLog_log(LogType type, std::string_view text) {
    if (type == LogType::CoreinitLogging) {
        constexpr std::string_view prefix = "[OSConsole] ";
        if (text.starts_with(prefix))
            text.remove_prefix(prefix.size());
        fprintf(stderr, "%.*s\n", (int)text.size(), text.data());
        int failures;
        if (sscanf(std::string(text).c_str(), "DONE %d failed", &failures) == 1)
            s_failures = failures;
        return true;
    }
    if (getenv("CEMU_RUNNER_LOG"))
        fprintf(stderr, "cemu: %.*s\n", (int)text.size(), text.data());
    return true;
}
bool cemuLog_log(LogType type, std::u8string_view text) {
    return cemuLog_log(type, std::string_view((const char*)text.data(), text.size()));
}
void cemuLog_waitForFlush() {}
void cemuLog_createLogFile(bool) {}
bool cemuLog_advancedPPCLoggingEnabled() {
    return false;
}
void SetThreadName(const char*) {}

// The user folder holds the SD card, the system libraries are always emulated
void ActiveSettings::SetPaths(bool, const fs::path&, const fs::path& userDataPath, const fs::path&, const fs::path&,
                              const fs::path&, std::set<fs::path>&) {
    s_user_data_path = userDataPath;
}
fs::path ActiveSettings::GetMlcPath() {
    return GetUserDataPath("mlc01");
}
uint8 ActiveSettings::GetTimerShiftFactor() {
    return 3;
}
bool ActiveSettings::LoadSharedLibrariesEnabled() {
    return false;
}
XMLConfigParser CemuConfig::Load(XMLConfigParser& parser) {
    return parser;
}
XMLConfigParser CemuConfig::Save(XMLConfigParser& parser) {
    return parser;
}
void WindowSystem::ShowErrorDialog(std::string_view message, std::string_view, std::optional<ErrorCategory>) {
    fprintf(stderr, "cemu-runner: %.*s\n", (int)message.size(), message.data());
}

// The emulated system libraries, the others aren't there
std::span<COSModule*> GetCOSModules() {
    static COSModule* modules[] = {coreinit::GetModule(), nlibnss::GetModule()};
    return modules;
}

// No JIT, GPU, debugger, graphic packs, audio, input or other system libraries
void PPCRecompiler_attemptEnter(PPCInterpreter_t*, uint32) {}
void PPCRecompiler_attemptEnterWithoutRecompile(PPCInterpreter_t*, uint32) {}
void PPCRecompiler_recompileIfUnvisited(uint32) {}
void PPCRecompiler_allocateRange(uint32, uint32) {}
void PPCRecompiler_invalidateRange(uint32, uint32) {}
void LatteBufferCache_notifyDCFlush(MPTR, uint32) {}
GDBServer::~GDBServer() {}
void GDBServer::HandleTrapInstruction(PPCInterpreter_t*) {}
void debugger_enterTW(PPCInterpreter_t*, bool) {}
DebuggerDispatcher g_debuggerDispatcher;
std::vector<GraphicPackPtr> GraphicPack2::s_active_graphic_packs;
std::vector<std::pair<MPTR, MPTR>> GraphicPack2::GetActiveRAMMappings() {
    return {};
}
void GraphicPack2::NotifyModuleLoaded(const RPLModule*) {}
uint8* extractCafeDefaultFont(sint32* size) {
    *size = 0;
    return nullptr;
}
sint32 iosuIoctl_pushAndWait(uint32, ioQueueEntry_t*) {
    return -1;
}
void nnNfp_update() {}
namespace snd_core {
void AXOut_update() {}
}  // namespace snd_core
namespace nn::act {
uint32 GetUuidEx(uint8* uuid, uint8, sint32) {
    memset(uuid, 0, 16);
    return 0;
}
uint32 GetSimpleAddressIdEx(uint32be* id, uint8) {
    *id = 0;
    return 0;
}
}  // namespace nn::act

// The parts of CafeSystem the OS asks, for a standalone RPX
namespace CafeSystem {
TitleId GetForegroundTitleId() {
    return 0xFFFFFFFF00000001ULL;
}
uint32 GetForegroundTitleSDKVersion() {
    return 21204;
}
CafeConsoleRegion GetForegroundTitleRegion() {
    return CafeConsoleRegion::EUR;
}
CafeConsoleRegion GetPlatformRegion() {
    return CafeConsoleRegion::EUR;
}
std::string GetForegroundTitleArgStr() {
    return {};
}
CosCapabilityBits GetForegroundTitleCosCapabilities(CosCapabilityGroup) {
    return CosCapabilityBits::All;
}
bool GetOverrideArgStr(std::vector<std::string>&) {
    return false;
}
void SetOverrideArgs(std::span<std::string>) {}
void UnsetOverrideArgs() {}
void ShutdownTitle() {}
// coreinit's exit ends here after the atexit cleanups
void NotifyPPCProcessExit(sint32 status) {
    s_exitStatus = status & 0xff;
}
void RequestRecreateCanvas() {}
PREPARE_STATUS_CODE PrepareForegroundTitle(TitleId) {
    return PREPARE_STATUS_CODE::UNABLE_TO_MOUNT;
}
void LaunchForegroundTitle() {}
}  // namespace CafeSystem

// Crashes print where they happened, guest addresses are relative to memory_base
static void crashed(int signal, siginfo_t* info, void*) {
    fprintf(stderr, "cemu-runner: signal %d at %p (guest %08llx)\n", signal, info->si_addr,
            (unsigned long long)((uint8*)info->si_addr - memory_base));
    void* frames[32];
    backtrace_symbols_fd(frames, backtrace(frames, 32), STDERR_FILENO);
    _exit(1);
}

static int fail(const char* message) {
    fprintf(stderr, "cemu-runner: %s\n", message);
    return 1;
}

// Loads the RPX like CafeSystem::LoadMainExecutable, from the code folder
static bool load(const fs::path& rpx) {
    FSCDeviceHostFS_Mount("/internal/code/", _pathToUtf8(rpx.parent_path()), FSC_PRIORITY_BASE);
    _pathToExecutable = "/internal/code/" + _pathToUtf8(rpx.filename());
    uint32 size = 0;
    uint8* data = fsc_extractFile(_pathToExecutable.c_str(), &size);
    if (!data)
        return false;
    RPLLoader_AddDependency(_pathToExecutable.c_str(), true);
    applicationRPX = RPLLoader_LoadFromMemory(data, size, _pathToExecutable.data());
    free(data);
    if (!applicationRPX)
        return false;
    RPLLoader_SetMainModule(applicationRPX);
    _entryPoint = RPLLoader_GetModuleEntrypoint(applicationRPX);
    return true;
}

int main(int argc, char** argv) {
    if (argc < 2 || argc > 5)
        return fail("Usage: cemu-runner <program.rpx> [test | run] [seconds] [user folder]");
    std::string_view mode = argc > 2 ? argv[2] : "run";
    int seconds = argc > 3 ? atoi(argv[3]) : 60;
    if ((mode != "test" && mode != "run") || seconds <= 0)
        return fail("Expected test or run and a number of seconds");
    if (getenv("CEMU_RUNNER_LOG"))
        s_loggingFlagMask = ~0ULL;
    struct sigaction action{};
    action.sa_sigaction = crashed;
    action.sa_flags = SA_SIGINFO;
    for (int signal : {SIGSEGV, SIGBUS, SIGILL, SIGTRAP})
        sigaction(signal, &action, nullptr);
    std::set<fs::path> failedWriteAccess;
    ActiveSettings::SetPaths(false, {}, fs::absolute(argc > 4 ? argv[4] : fs::temp_directory_path() / "cemu-runner"),
                             {}, {}, {}, failedWriteAccess);
    std::error_code error;
    fs::create_directories(ActiveSettings::GetUserDataPath("sdcard"), error);

    // The system, like CafeSystem::Initialize with only the kernel and FSA of IOSU. The PPC clock
    // calibrates itself against the host clock in the background meanwhile.
    PPCTimer_init();
    fsc_init();
    memory_init();
    PPCCore_init();
    RPLLoader_InitState();
    rplSymbolStorage_init();
    SysAllocatorContainer::GetInstance().Initialize();
    iosu::kernel::GetModule()->SystemLaunch();
    iosu::fsa::Initialize();
    memory_mapForCurrentTitle();

    // Start it like cemu_initForGame without the GPU and audio: coreinit is loaded before the RPX
    // and its entry starts the main thread, the scheduler runs one core
    PPCTimer_waitForInit();
    iosu::kernel::GetModule()->TitleStart();
    ppcCyclesSince2000 = ppcCyclesSince2000_UTC = (uint64)(time(nullptr) - 946684800) * ESPRESSO_CORE_CLOCK;
    ppcCyclesSince2000TimerClock = ppcCyclesSince2000 / 20;
    PPCTimer_start();
    RPLLoader_LoadCoreinit();
    if (!load(fs::absolute(argv[1])))
        return fail("Cannot load the RPX");
    RPLLoader_UpdateDependencies();
    RPLLoader_Link();
    RPLLoader_NotifyControlPassedToApplication();
    RPLLoader_CallCoreinitEntrypoint();
    coreinit::OSSchedulerBegin(1);

    bool test = mode == "test";
    for (int i = 0; i < seconds * 100 && s_exitStatus < 0 && !(test && s_failures >= 0); i++)
        std::this_thread::sleep_for(std::chrono::milliseconds(10));
    int status;
    if (!test)
        status = s_exitStatus >= 0 ? s_exitStatus.load() : fail("The program didn't exit in time");
    else if (s_failures < 0)
        status = fail(s_exitStatus < 0 ? "The test didn't finish in time" : "The program exited before DONE");
    else
        status = s_failures > 0 ? fail("Guest tests failed") : 0;
    // The emulation threads don't stop, so the process ends without cleaning up
    fflush(stderr);
    _exit(status);
}
