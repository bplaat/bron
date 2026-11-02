#pragma once
// Minimal host environment for Cemu's interpreter, independent of its desktop frontend.
#include <algorithm>
#include <array>
#include <atomic>
#include <bit>
#include <charconv>
#include <climits>
#include <cmath>
#include <cstdint>
#include <cstdio>
#include <cstring>
#include <fstream>
#include <limits>
#include <map>
#include <stdexcept>
#include <string>
#include <string_view>
#include <thread>
#include <vector>

using uint8 = uint8_t;
using uint16 = uint16_t;
using uint32 = uint32_t;
using uint64 = uint64_t;
using sint8 = int8_t;
using sint16 = int16_t;
using sint32 = int32_t;
using sint64 = int64_t;
using MPTR = uint32;
using VAddr = uint32;
using PAddr = uint32;
#ifdef __APPLE__
#include <libkern/OSByteOrder.h>
#define BOOST_OS_MACOS 1
#else
#include <byteswap.h>
#define BOOST_OS_LINUX 1
#endif
inline uint32 _swapEndianU32(uint32 value) {
    return __builtin_bswap32(value);
}
inline uint64 _swapEndianU64(uint64 value) {
    return __builtin_bswap64(value);
}
inline uint16 _swapEndianU16(uint16 value) {
    return __builtin_bswap16(value);
}
inline sint32 _swapEndianS32(sint32 value) {
    return __builtin_bswap32(value);
}
#define FORCE_INLINE inline __attribute__((always_inline))
#define cemu_assert_debug(condition)                                 \
    do {                                                             \
        if (!(condition))                                            \
            throw std::runtime_error("Cemu assertion: " #condition); \
    } while (0)
#define cemu_assert_unimplemented() throw std::runtime_error("Unsupported Espresso instruction")
#define cemu_assert_suspicious() cemu_assert_unimplemented()
#define cemu_assert_error() cemu_assert_unimplemented()
#define assert_dbg() cemu_assert_unimplemented()
#define debug_printf(...) ((void)0)
enum class LogType { Force };
template <typename... Args>
void cemuLog_logDebug(Args&&...) {}

#include "Common/betype.h"
inline void debugBreakpoint() {
    throw std::runtime_error("Espresso breakpoint");
}
template <typename T>
std::atomic<T>* _rawPtrToAtomic(T* pointer) {
    return reinterpret_cast<std::atomic<T>*>(pointer);
}
template <typename... Args>
void cemuLog_log(Args&&...) {}
