#pragma once
// Headless shim: the string comparisons of Boost.StringAlgo that Cemu's headers use, which like
// Boost compare narrow and wide strings character by character
#include <algorithm>
#include <cctype>
#include <string>
#include <string_view>
namespace boost {
namespace detail {
template <typename Char>
std::basic_string_view<Char> view(const Char* text) {
    return text;
}
template <typename Char>
std::basic_string_view<Char> view(std::basic_string_view<Char> text) {
    return text;
}
template <typename Char>
std::basic_string_view<Char> view(const std::basic_string<Char>& text) {
    return text;
}
}  // namespace detail
template <typename Text, typename Prefix>
bool starts_with(const Text& text, const Prefix& prefix) {
    auto t = detail::view(text);
    auto p = detail::view(prefix);
    return t.size() >= p.size() &&
           std::equal(p.begin(), p.end(), t.begin(), [](auto a, auto b) { return char32_t(a) == char32_t(b); });
}
inline bool iequals(std::string_view a, std::string_view b) {
    return std::ranges::equal(
        a, b, [](char x, char y) { return std::tolower((unsigned char)x) == std::tolower((unsigned char)y); });
}
}  // namespace boost
