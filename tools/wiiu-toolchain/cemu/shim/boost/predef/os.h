#pragma once
// Headless shim: the operating system macros of Boost.Predef that Cemu checks
#if defined(_WIN32)
#define BOOST_OS_WINDOWS 1
#elif defined(__APPLE__)
#define BOOST_OS_MACOS 1
#define BOOST_OS_UNIX 1
#elif defined(__linux__)
#define BOOST_OS_LINUX 1
#define BOOST_OS_UNIX 1
#endif
#define BOOST_OS_BSD 0
