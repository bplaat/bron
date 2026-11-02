// CPU-only frontend for Cemu's Espresso interpreter. No executable memory or graphics backend.
#include <sys/mman.h>
#include <zlib.h>

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

// Loads an RPX like Cafe OS: inflates its zlib sections, puts the allocated ones at their addresses
// and applies its relocations. Imports of the system libraries resolve to OS call stubs below the
// code, in range of its branches, a data import to a pointer to one.
struct Section {
    uint32 name, type, flags, address, link, info, entrySize;
    std::vector<uint8> data;
};
static std::map<uint32, std::string> imports;
static uint32 load(const char* path) {
    std::ifstream file(path, std::ios::binary | std::ios::ate);
    require(bool(file), "Cannot open RPX");
    auto length = file.tellg();
    require(length >= 52 && length <= 128 * 1024 * 1024, "Invalid RPX size");
    std::vector<uint8> bytes(static_cast<size_t>(length));
    file.seekg(0);
    require(bool(file.read(reinterpret_cast<char*>(bytes.data()), bytes.size())), "Cannot read RPX");
    auto word = [](const std::vector<uint8>& data, size_t offset, size_t count = 4) {
        require(offset <= data.size() && count <= data.size() - offset, "Truncated RPX");
        uint32 result = 0;
        for (size_t i = 0; i < count; i++)
            result = (result << 8) | data[offset + i];
        return result;
    };
    require(word(bytes, 0) == 0x7f454c46 && word(bytes, 4) == 0x010201ca && bytes[8] == 0xfe,
            "Expected a big-endian ELF32 Cafe OS file");
    require(word(bytes, 16, 2) == 0xfe01 && word(bytes, 18, 2) == 20, "Expected a PowerPC RPX executable");
    uint32 entry = word(bytes, 24), table = word(bytes, 32), stride = word(bytes, 46, 2), count = word(bytes, 48, 2),
           names = word(bytes, 50, 2);
    require(stride == 40 && count > 0 && names < count && uint64(table) + uint64(count) * stride <= bytes.size(),
            "Invalid RPX section table");
    std::vector<Section> sections;
    for (uint32 i = 0; i < count; i++) {
        size_t pos = table + i * stride;
        Section section{word(bytes, pos),      word(bytes, pos + 4),  word(bytes, pos + 8),  word(bytes, pos + 12),
                        word(bytes, pos + 24), word(bytes, pos + 28), word(bytes, pos + 36), {}};
        uint32 offset = word(bytes, pos + 16), size = word(bytes, pos + 20);
        if (section.type == 8) {
            section.data.resize(size);
        } else if (size > 0) {
            require(uint64(offset) + size <= bytes.size(), "Truncated RPX section");
            if (section.flags & 0x08000000) {
                // The inflated size, then the zlib stream
                uLongf inflated = word(bytes, offset);
                section.data.resize(inflated);
                require(uncompress(section.data.data(), &inflated, bytes.data() + offset + 4, size - 4) == Z_OK &&
                            inflated == section.data.size(),
                        "Cannot inflate RPX section");
            } else {
                section.data.assign(bytes.begin() + offset, bytes.begin() + offset + size);
            }
        }
        sections.push_back(std::move(section));
    }
    auto string = [&](const Section& section, uint32 offset) {
        require(section.type == 3 && offset < section.data.size(), "Invalid RPX string table");
        const char* start = reinterpret_cast<const char*>(section.data.data() + offset);
        const char* end = static_cast<const char*>(memchr(start, 0, section.data.size() - offset));
        require(end != nullptr, "Unterminated RPX string");
        return std::string(start, end);
    };
    bool executableEntry = false;
    for (const auto& section : sections) {
        if (!(section.flags & 2) || section.data.empty())
            continue;
        uint64 end = uint64(section.address) + section.data.size();
        require((section.address >= 0x02000000 && end <= 0x42000000) ||
                    (section.address >= 0xc0000000 && end <= 0xc8000000),
                "RPX section outside toolchain memory");
        if ((section.flags & 4) && entry >= section.address && entry < end)
            executableEntry = true;
        memcpy(memory_base + section.address, section.data.data(), section.data.size());
    }
    require(executableEntry && (entry & 3) == 0, "Invalid RPX entry point");

    uint32 stubs = 0x01000000;
    auto stub = [&](const std::string& name) {
        imports[stubs] = name;
        return (stubs += 4) - 4;
    };
    std::map<uint32, std::vector<uint32>> values;
    for (uint32 index = 0; index < count; index++) {
        const auto& section = sections[index];
        if (section.type != 2)
            continue;
        require(section.entrySize == 16 && section.data.size() % 16 == 0 && section.link < count,
                "Invalid RPX symbols");
        auto& symbols = values[index];
        for (size_t pos = 0; pos < section.data.size(); pos += 16) {
            uint32 value = word(section.data, pos + 4), shndx = word(section.data, pos + 14, 2);
            auto symbol = string(sections[section.link], word(section.data, pos));
            auto kind = shndx > 0 && shndx < count ? string(sections[names], sections[shndx].name) : "";
            bool function = kind.starts_with(".fimport_");
            // The import sections' own section symbols have no name and aren't imports
            if (!symbol.empty() && (function || kind.starts_with(".dimport_"))) {
                if (function) {
                    value = stub(symbol);
                } else {
                    require(symbol == "MEMAllocFromDefaultHeapEx" || symbol == "MEMFreeToDefaultHeap",
                            ("Unsupported data import: " + symbol).c_str());
                    value = stubs;
                    stubs += 4;
                    memory_writeU32(value, stub(symbol));
                }
            }
            symbols.push_back(value);
        }
    }
    require(!values.empty(), "RPX without symbols");

    for (const auto& section : sections) {
        if (section.type != 4)
            continue;
        require(section.entrySize == 12 && section.data.size() % 12 == 0 && values.contains(section.link) &&
                    section.info < count && (sections[section.info].flags & 2),
                "Invalid RPX relocations");
        const auto& symbols = values.at(section.link);
        const auto& target = sections[section.info];
        for (size_t pos = 0; pos < section.data.size(); pos += 12) {
            uint32 address = word(section.data, pos), info = word(section.data, pos + 4), type = info & 0xff;
            require(address >= target.address && uint64(address) + 4 <= uint64(target.address) + target.data.size(),
                    "RPX relocation outside its section");
            require((info >> 8) < symbols.size(), "Invalid RPX relocation symbol");
            uint32 value = symbols[info >> 8] + word(section.data, pos + 8), relative = value - address;
            uint8* place = memory_base + address;
            auto half = [&](uint32 bits) {
                place[0] = uint8(bits >> 8);
                place[1] = uint8(bits);
            };
            switch (type) {
                case 0:  // R_PPC_NONE
                    break;
                case 1:  // R_PPC_ADDR32
                    memory_writeU32(address, value);
                    break;
                case 4:  // R_PPC_ADDR16_LO
                    half(value);
                    break;
                case 5:  // R_PPC_ADDR16_HI
                    half(value >> 16);
                    break;
                case 6:  // R_PPC_ADDR16_HA
                    half((value + 0x8000) >> 16);
                    break;
                case 10:  // R_PPC_REL24
                    require(sint32(relative) >= -0x02000000 && sint32(relative) < 0x02000000 && !(relative & 3),
                            "RPX branch out of range");
                    memory_writeU32(address, (memory_readU32(address) & ~0x03fffffcu) | (relative & 0x03fffffc));
                    break;
                case 11:  // R_PPC_REL14
                    require(sint32(relative) >= -0x8000 && sint32(relative) < 0x8000 && !(relative & 3),
                            "RPX branch out of range");
                    memory_writeU32(address, (memory_readU32(address) & ~0xfffcu) | (relative & 0xfffc));
                    break;
                case 251:  // R_PPC_GHS_REL16_HA
                    half((relative + 0x8000) >> 16);
                    break;
                case 252:  // R_PPC_GHS_REL16_HI
                    half(relative >> 16);
                    break;
                case 253:  // R_PPC_GHS_REL16_LO
                    half(relative);
                    break;
                default:
                    throw std::runtime_error("Unsupported RPX relocation " + std::to_string(type));
            }
        }
    }
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
                "Usage: cemu-runner <program.rpx> ['test <instructions>' | 'run <instructions>']");
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
