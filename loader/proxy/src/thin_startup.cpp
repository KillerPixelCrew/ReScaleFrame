#include "thin_startup.h"
#include "thin_config.h"
#include "thin_library.h"
#include <windows.h>
#include <cstdint>
#include <cstdio>
#include <cstring>
#include <string>

namespace {
void log(const std::wstring& root, const char* message)
{
    OutputDebugStringA(message);
    const auto path = root + L"ReScaleFrame.Loader.log";
    HANDLE file = CreateFileW(path.c_str(), FILE_APPEND_DATA, FILE_SHARE_READ | FILE_SHARE_WRITE,
        nullptr, OPEN_ALWAYS, FILE_ATTRIBUTE_NORMAL, nullptr);
    if (file != INVALID_HANDLE_VALUE) {
        DWORD written = 0;
        WriteFile(file, message, static_cast<DWORD>(std::strlen(message)), &written, nullptr);
        CloseHandle(file);
    }
}
bool relative_module(const std::wstring& value)
{
    if (value.empty() || value.front() == L'/' || value.front() == L'\\') return false;
    for (wchar_t c : value)
        if (c < 32 || c == 127 || c == L':' || c == L'*' || c == L'?' || c == L'"' || c == L'<' ||
            c == L'>' || c == L'|') return false;
    for (size_t begin = 0; begin < value.size();) {
        const auto end = value.find_first_of(L"/\\", begin);
        const auto component = value.substr(begin, end == std::wstring::npos ? end : end - begin);
        if (component.empty() || component == L"." || component == L".." ||
            component.back() == L'.' || component.back() == L' ') return false;
        if (end == std::wstring::npos) return true;
        begin = end + 1;
    }
    return false;
}
struct File {
    HANDLE handle = INVALID_HANDLE_VALUE;
    std::wstring path;
    BY_HANDLE_FILE_INFORMATION info{};
    File() = default;
    File(const File&) = delete;
    File& operator=(const File&) = delete;
    ~File() { if (handle != INVALID_HANDLE_VALUE) CloseHandle(handle); }
    bool open(std::wstring name)
    {
        for (auto& c : name) if (c == L'/') c = L'\\';
        // Keep write/delete sharing disabled until after LoadLibrary, so checked files cannot be replaced.
        handle = CreateFileW(name.c_str(), GENERIC_READ, FILE_SHARE_READ, nullptr, OPEN_EXISTING,
            FILE_ATTRIBUTE_NORMAL, nullptr);
        if (handle == INVALID_HANDLE_VALUE || !GetFileInformationByHandle(handle, &info) ||
            (info.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY)) return false;
        wchar_t final[32768];
        const DWORD length = GetFinalPathNameByHandleW(handle, final, _countof(final), FILE_NAME_NORMALIZED);
        if (!length || length >= _countof(final)) return false;
        path.assign(final, length);
        return true;
    }
    bool inside(const std::wstring& root) const
    {
        return path.size() > root.size() &&
            CompareStringOrdinal(path.data(), static_cast<int>(root.size()), root.data(),
                static_cast<int>(root.size()), TRUE) == CSTR_EQUAL;
    }
    bool same(const File& other) const
    {
        return info.dwVolumeSerialNumber == other.info.dwVolumeSerialNumber &&
            info.nFileIndexHigh == other.info.nFileIndexHigh && info.nFileIndexLow == other.info.nFileIndexLow;
    }
};
void start()
{
    HMODULE self = nullptr;
    if (!GetModuleHandleExW(GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS |
        GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT, reinterpret_cast<LPCWSTR>(&thin_startup), &self)) return;
    wchar_t path[32768];
    const DWORD length = GetModuleFileNameW(self, path, _countof(path));
    if (!length || length >= _countof(path)) return;
    File carrier;
    if (!carrier.open(std::wstring(path, length))) return;
    const auto root = carrier.path.substr(0, carrier.path.find_last_of(L'\\') + 1);
    File config;
    if (!config.open(root + L"ReScaleFrame.ini") || !config.inside(root)) {
        log(root, "loader: configuration missing or outside carrier directory\r\n");
        return;
    }
    ThinConfig settings;
    std::string error;
    if (!thin_read_config(config.handle, settings, error)) {
        log(root, ("loader: " + error + "\r\n").c_str());
        return;
    }
    if (!settings.auto_start) { log(root, "loader: AutoStart disabled\r\n"); return; }
    if (!relative_module(settings.runtime_module) || !relative_module(settings.entry_module)) {
        log(root, "loader: module path refused (must be contained relative paths)\r\n");
        return;
    }
    File runtime, adapter;
    // Validate both paths before loading either module, including junction/symlink resolution.
    if (!runtime.open(root + settings.runtime_module) || !runtime.inside(root) ||
        !adapter.open(root + settings.entry_module) || !adapter.inside(root) ||
        runtime.same(adapter) || runtime.same(carrier) || adapter.same(carrier)) {
        log(root, "loader: module missing, escaped carrier directory, or aliases another module\r\n");
        return;
    }
    HMODULE runtime_module = thin_load_library(runtime.path.c_str(),
        LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_SYSTEM32);
    if (!runtime_module) {
        const DWORD error_code = GetLastError();
        char message[128];
        sprintf_s(message, "loader: runtime load failed win32=%lu\r\n", error_code);
        log(root, message);
        return;
    }
    HMODULE entry_module = thin_load_library(adapter.path.c_str(),
        LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_SYSTEM32);
    if (!entry_module) {
        const DWORD error_code = GetLastError();
        char message[128];
        sprintf_s(message, "loader: adapter load failed win32=%lu\r\n", error_code);
        log(root, message);
        return;
    }
    FARPROC address = GetProcAddress(entry_module, settings.entry_point.c_str());
    using Entry = uint32_t (__stdcall*)(const wchar_t*);
    Entry function = nullptr;
    static_assert(sizeof(function) == sizeof(address));
    std::memcpy(&function, &address, sizeof(function));
    if (!function) { log(root, "loader: entry export missing\r\n"); return; }
    const uint32_t result = function(config.path.c_str());
    char message[96];
    sprintf_s(message, "loader: entry %s status=%lu\r\n", result ? "refused" : "ready",
        static_cast<unsigned long>(result));
    log(root, message);
    // References are intentionally never released, including on adapter refusal.
}
}
void thin_startup() noexcept
{
    try { start(); }
    catch (...) { OutputDebugStringA("loader: startup exception; system forwarding preserved\r\n"); }
}
