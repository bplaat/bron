#pragma once
// Headless shim: the UTF-8 conversions of Boost.Nowide that Cemu's headers use
#include <string>
#include <string_view>
namespace boost::nowide {
inline std::string narrow(std::wstring_view text) {
    std::string result;
    for (wchar_t c : text) {
        char32_t code = c;
        if (code < 0x80) {
            result += char(code);
        } else if (code < 0x800) {
            result += char(0xc0 | code >> 6);
            result += char(0x80 | (code & 0x3f));
        } else if (code < 0x10000) {
            result += char(0xe0 | code >> 12);
            result += char(0x80 | (code >> 6 & 0x3f));
            result += char(0x80 | (code & 0x3f));
        } else {
            result += char(0xf0 | code >> 18);
            result += char(0x80 | (code >> 12 & 0x3f));
            result += char(0x80 | (code >> 6 & 0x3f));
            result += char(0x80 | (code & 0x3f));
        }
    }
    return result;
}
inline std::string narrow(std::string_view text) {
    return std::string(text);
}
inline std::wstring widen(std::string_view text) {
    std::wstring result;
    for (size_t i = 0; i < text.size();) {
        unsigned char lead = text[i];
        size_t length = lead < 0x80 ? 1 : lead < 0xe0 ? 2 : lead < 0xf0 ? 3 : 4;
        char32_t code = length == 1 ? lead : lead & (0x7f >> length);
        for (size_t j = 1; j < length && i + j < text.size(); j++)
            code = code << 6 | (text[i + j] & 0x3f);
        result += wchar_t(code);
        i += length;
    }
    return result;
}
inline std::wstring widen(const char* text, size_t length) {
    return widen(std::string_view(text, length));
}
}  // namespace boost::nowide
