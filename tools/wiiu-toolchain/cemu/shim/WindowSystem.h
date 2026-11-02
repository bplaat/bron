#pragma once
// Headless shim: only the error dialog of the OS emulation, which cemu/runner.cpp prints
namespace WindowSystem {
enum class ErrorCategory {
    KEYS_TXT_CREATION = 0,
    GRAPHIC_PACKS = 1,
};

void ShowErrorDialog(std::string_view message, std::string_view title, std::optional<ErrorCategory> errorCategory = {});
inline void ShowErrorDialog(std::string_view message, std::optional<ErrorCategory> errorCategory = {}) {
    ShowErrorDialog(message, "", errorCategory);
}
}  // namespace WindowSystem
