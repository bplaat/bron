// CPU-only frontend for Cemu's Espresso interpreter. No executable memory or graphics backend.
#include <sys/mman.h>

#include "Cafe/HW/Espresso/PPCState.h"

uint8* memory_base;
uint64 ppcMainThreadDECCycleStart, ppcMainThreadDECCycleValue;
uint64 ppcCyclesSince2000, ppcCyclesSince2000TimerClock, ppcCyclesSince2000_UTC;
static uint64 instructions;
static uint32 lastIP;

uint8* memory_getPointerFromVirtualOffset(uint32 address) {
    return memory_base + address;
}
uint8* memory_getPointerFromVirtualOffsetAllowNull(uint32 address) {
    return address ? memory_base + address : nullptr;
}
uint32 memory_readU32(uint32 address) {
    return _swapEndianU32(*(uint32*)(memory_base + address));
}
void memory_writeU32(uint32 address, uint32 value) {
    *(uint32*)(memory_base + address) = _swapEndianU32(value);
}
void PPCInterpreter_nextInstruction(PPCInterpreter_t* cpu) {
    cpu->instructionPointer += 4;
}
void PPCInterpreter_jumpToInstruction(PPCInterpreter_t* cpu, uint32 address) {
    cpu->instructionPointer = address;
}
uint64 PPCInterpreter_getMainCoreCycleCounter() {
    return instructions * 20;
}
uint64 PPCTimer_getFromRDTSC() {
    return instructions;
}
uint32 PPCInterpreter_getXER(PPCInterpreter_t* cpu) {
    return (cpu->spr.XER & ~(1u << 29)) | (uint32(cpu->xer_ca) << 29);
}
void PPCInterpreter_setXER(PPCInterpreter_t* cpu, uint32 value) {
    cpu->spr.XER = value;
    cpu->xer_ca = (value >> 29) & 1;
}
void PPCInterpreter_setDEC(PPCInterpreter_t* cpu, uint32 value) {
    cpu->sprExtended.DEC = value;
}
void PPCInterpreter_virtualHLE(PPCInterpreter_t*, unsigned int) {
    throw std::runtime_error("Unexpected HLE instruction");
}

static void require(bool condition, const char* message) {
    if (!condition)
        throw std::runtime_error(message);
}

// These ELFs are already relocated by LLD. RPX compression and dynamic loading belong to Cemu's
// full frontend; the toolchain's original ELF keeps the symbols needed to resolve OS imports.
struct Section {
    uint32 name, type, flags, address, offset, size, link, entrySize;
};
static std::map<uint32, std::string> imports;
static uint32 load(const char* path) {
    std::ifstream file(path, std::ios::binary | std::ios::ate);
    require(bool(file), "Cannot open ELF");
    auto length = file.tellg();
    require(length >= 52 && length <= 128 * 1024 * 1024, "Invalid ELF size");
    std::vector<uint8> bytes(static_cast<size_t>(length));
    file.seekg(0);
    require(bool(file.read(reinterpret_cast<char*>(bytes.data()), bytes.size())), "Cannot read ELF");
    auto word = [&](size_t offset, size_t count = 4) {
        require(offset <= bytes.size() && count <= bytes.size() - offset, "Truncated ELF");
        uint32 result = 0;
        for (size_t i = 0; i < count; i++)
            result = (result << 8) | bytes[offset + i];
        return result;
    };
    require(word(0) == 0x7f454c46 && bytes[4] == 1 && bytes[5] == 2 && bytes[6] == 1,
            "Expected a big-endian ELF32 file");
    require(word(16, 2) == 2 && word(18, 2) == 20, "Expected a linked PowerPC executable");
    uint32 entry = word(24), table = word(32), stride = word(46, 2), count = word(48, 2);
    require(stride == 40 && count > 0 && uint64(table) + uint64(count) * stride <= bytes.size(),
            "Invalid ELF section table");
    std::vector<Section> sections;
    for (uint32 i = 0; i < count; i++) {
        size_t pos = table + i * stride;
        sections.push_back({word(pos), word(pos + 4), word(pos + 8), word(pos + 12), word(pos + 16), word(pos + 20),
                            word(pos + 24), word(pos + 36)});
    }
    auto string = [&](const Section& section, uint32 offset) {
        require(section.type == 3 && offset < section.size && uint64(section.offset) + section.size <= bytes.size(),
                "Invalid ELF string table");
        const char* start = reinterpret_cast<const char*>(bytes.data() + section.offset + offset);
        const char* end = static_cast<const char*>(memchr(start, 0, section.size - offset));
        require(end != nullptr, "Unterminated ELF string");
        return std::string(start, end);
    };
    uint32 names = word(50, 2);
    require(names < count, "Invalid ELF section names");
    bool executableEntry = false;
    for (const auto& section : sections) {
        if (!(section.flags & 2) || section.size == 0)
            continue;
        uint64 end = uint64(section.address) + section.size;
        require((section.address >= 0x02000000 && end <= 0x42000000) ||
                    (section.address >= 0xc0000000 && end <= 0xc8000000),
                "ELF section outside toolchain memory");
        if ((section.flags & 4) && entry >= section.address && entry < end)
            executableEntry = true;
        if (section.type != 8) {
            require(uint64(section.offset) + section.size <= bytes.size(), "Truncated ELF section");
            memcpy(memory_base + section.address, bytes.data() + section.offset, section.size);
        }
    }
    require(executableEntry && (entry & 3) == 0, "Invalid ELF entry point");
    uint32 trampoline = 0x01000000;
    bool symbols = false;
    for (const auto& section : sections) {
        if (section.type != 2)
            continue;
        symbols = true;
        require(section.entrySize == 16 && section.size % 16 == 0 && section.link < count &&
                    uint64(section.offset) + section.size <= bytes.size(),
                "Invalid ELF symbols");
        for (size_t pos = section.offset; pos < uint64(section.offset) + section.size; pos += 16) {
            uint32 address = word(pos + 4), index = word(pos + 14, 2);
            if (index == 0 || index >= count)
                continue;
            auto name = string(sections[names], sections[index].name);
            if (!name.starts_with(".fimport_") && !name.starts_with(".dimport_"))
                continue;
            auto symbol = string(sections[section.link], word(pos));
            if (symbol.empty() || (bytes[pos + 12] >> 4) == 0)
                continue;
            require(address >= sections[index].address &&
                        uint64(address) + 4 <= uint64(sections[index].address) + sections[index].size,
                    "Invalid import address");
            if (name.starts_with(".dimport_")) {
                require(symbol == "MEMAllocFromDefaultHeapEx" || symbol == "MEMFreeToDefaultHeap",
                        ("Unsupported data import: " + symbol).c_str());
                memory_writeU32(address, trampoline);
                imports[trampoline] = symbol;
                trampoline += 4;
            } else {
                imports[address] = symbol;
            }
        }
    }
    require(symbols, "ELF must retain its symbol table");
    return entry;
}

static uint32 heapEnd = 0x30000000;
static std::map<uint32, uint32> allocations;
static bool exited, done;
static int exitStatus, failures;
static void report(std::string_view output) {
    fwrite(output.data(), 1, output.size(), stderr);
    static std::string line;
    for (char byte : output) {
        require(line.size() < 4096, "OSReport line exceeds 4096 bytes");
        line += byte;
        if (byte != '\n')
            continue;
        // Only an exact, complete completion line can finish a test.
        if (line.starts_with("DONE ") && line.ends_with(" failed\n")) {
            auto digits = std::string_view(line).substr(5, line.size() - 13);
            int count;
            auto [end, error] = std::from_chars(digits.data(), digits.data() + digits.size(), count);
            if (!digits.empty() && digits.find_first_not_of("0123456789") == std::string_view::npos &&
                error == std::errc{} && end == digits.data() + digits.size()) {
                failures = count;
                done = true;
            }
        }
        line.clear();
    }
}
static std::string_view guestString(uint32 address) {
    require(address >= 0x10000 && address < 0xffff0000, "Invalid guest string");
    const char* start = reinterpret_cast<const char*>(memory_base + address);
    const char* end = static_cast<const char*>(memchr(start, 0, std::min<uint64>(4096, 0xffff0000ull - address)));
    require(end != nullptr, "Guest string exceeds 4096 bytes");
    return {start, size_t(end - start)};
}
static void systemCall(PPCInterpreter_t& cpu, const std::string& name) {
    auto a = cpu.gpr[3], b = cpu.gpr[4];
    if (name == "MEMAllocFromDefaultHeapEx") {
        uint32 alignment = b ? b : 4;
        require((alignment & (alignment - 1)) == 0, "Invalid heap alignment");
        uint64 address = (uint64(heapEnd) + alignment - 1) & ~(uint64(alignment) - 1);
        if (address + a > 0x40000000) {
            cpu.gpr[3] = 0;
        } else {
            cpu.gpr[3] = uint32(address);
            allocations[uint32(address)] = a;
            heapEnd = uint32(address + std::max(a, 1u));
        }
    } else if (name == "MEMFreeToDefaultHeap") {
        require(a == 0 || allocations.erase(a) == 1, "Invalid heap free");
    } else if (name == "MEMGetSizeForMBlockExpHeap") {
        require(allocations.contains(a), "Invalid heap block");
        cpu.gpr[3] = allocations.at(a);
    } else if (name == "OSGetSystemInfo") {
        memory_writeU32(0x00010000, ESPRESSO_BUS_CLOCK);
        memory_writeU32(0x00010004, ESPRESSO_CORE_CLOCK);
        cpu.gpr[3] = 0x00010000;
    } else if (name == "OSGetTime" || name == "OSGetSystemTime") {
        // 2026-01-01, plus a deterministic time base advanced by interpreter instructions.
        uint64 ticks = 820540800ull * ESPRESSO_TIMER_CLOCK + instructions;
        cpu.gpr[3] = ticks >> 32;
        cpu.gpr[4] = uint32(ticks);
    } else if (name == "OSReport") {
        auto format = guestString(a);
        if (format == "%.*s") {
            auto line = guestString(cpu.gpr[5]);
            report(line.substr(0, std::min<size_t>(b, line.size())));
        } else if (format == "%s") {
            report(guestString(b));
        } else {
            throw std::runtime_error("Unsupported OSReport format: " + std::string(format));
        }
    } else if (name == "GX2Invalidate" || name == "DCFlushRange" || name == "DCInvalidateRange") {
        // CPU-only memory is coherent; there is no graphics device to invalidate.
    } else if (name == "exit" || name == "_Exit" || name == "__rplwrap_exit") {
        exited = true;
        exitStatus = int(a);
    } else {
        throw std::runtime_error("Unsupported OS import: " + name);
    }
    cpu.instructionPointer = cpu.spr.LR;
}

int main(int argc, char** argv) {
    try {
        require(argc == 2 || argc == 3,
                "Usage: cemu-runner <program.elf> ['test <instructions>' | 'run <instructions>']");
        std::string command = argc == 3 ? argv[2] : "run 100000000";
        bool test = command.starts_with("test ");
        require(test || command.starts_with("run "), "Expected test or run with an instruction budget");
        auto number = command.substr(command.find(' ') + 1);
        require(!number.empty() && number.find_first_not_of("0123456789") == std::string::npos,
                "Invalid instruction budget");
        size_t consumed;
        uint64 budget = std::stoull(number, &consumed);
        require(budget > 0 && consumed == number.size(), "Invalid instruction budget");
        constexpr uint64 memorySize = 1ull << 32;
        memory_base =
            static_cast<uint8*>(mmap(nullptr, memorySize, PROT_READ | PROT_WRITE, MAP_PRIVATE | MAP_ANON, -1, 0));
        require(memory_base != MAP_FAILED, "Cannot reserve guest memory");
        require(mprotect(memory_base, 0x10000, PROT_NONE) == 0, "Cannot protect null memory");
        PPCInterpreterGlobal_t global{};
        PPCInterpreter_t cpu{};
        cpu.global = &global;
        cpu.instructionPointer = load(argv[1]);
        cpu.gpr[1] = 0x2ffffff0;
        cpu.spr.UPIR = 1;
        cpu.PSE = cpu.LSQE = 1;
        cpu.remainingCycles = std::numeric_limits<sint32>::max();
        while (instructions < budget && !exited && !(test && done)) {
            lastIP = cpu.instructionPointer;
            auto call = imports.find(cpu.instructionPointer);
            if (call != imports.end())
                systemCall(cpu, call->second);
            else
                PPCInterpreterSlim_executeInstruction(&cpu);
            instructions++;
        }
        munmap(memory_base, memorySize);
        if (test) {
            require(done, exited ? "Program exited before DONE" : "Test instruction budget exhausted");
            require(failures == 0, "Guest tests failed");
        } else {
            require(exited, "Run instruction budget exhausted");
            return exitStatus;
        }
        return 0;
    } catch (const std::exception& error) {
        fprintf(stderr, "cemu-runner: %s (at %08x, after %llu instructions)\n", error.what(), lastIP,
                static_cast<unsigned long long>(instructions));
        return 1;
    }
}
