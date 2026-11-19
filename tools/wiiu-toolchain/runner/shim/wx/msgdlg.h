#pragma once
// Headless shim: message boxes go to stderr
#include <cstdio>
#define wxOK 0
#define wxCENTRE 0
#define wxICON_ERROR 0
#define wxICON_WARNING 0
#define wxYES_NO 0
#define wxYES 0
template <typename... Args>
inline int wxMessageBox(const char* message, const char* title = "", Args...) {
    fprintf(stderr, "%s: %s\n", title, message);
    return 0;
}
