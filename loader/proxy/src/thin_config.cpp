#include "thin_config.h"
#include <cstring>
#include <map>
#include <vector>

namespace {
std::wstring trim(const std::wstring& value)
{
    const auto begin = value.find_first_not_of(L" \t\r");
    return begin == std::wstring::npos ? L"" :
        value.substr(begin, value.find_last_not_of(L" \t\r") - begin + 1);
}
std::wstring lower(std::wstring value)
{
    for (auto& character : value)
        if (character >= L'A' && character <= L'Z') character += L'a' - L'A';
    return value;
}
bool fail(std::string& error, const char* reason)
{
    error = reason;
    return false;
}
bool decode(HANDLE file, std::wstring& text, std::string& error)
{
    LARGE_INTEGER size;
    if (!GetFileSizeEx(file, &size) || size.QuadPart <= 0 || size.QuadPart > 65536)
        return fail(error, "configuration is empty, unreadable, or exceeds 64 KiB");
    std::vector<char> bytes(static_cast<size_t>(size.QuadPart));
    DWORD read = 0;
    if (!ReadFile(file, bytes.data(), static_cast<DWORD>(bytes.size()), &read, nullptr) || read != bytes.size())
        return fail(error, "configuration read failed");
    if (bytes.size() >= 2 && static_cast<unsigned char>(bytes[0]) == 0xff &&
        static_cast<unsigned char>(bytes[1]) == 0xfe) {
        if (bytes.size() % 2) return fail(error, "invalid UTF-16LE configuration");
        text.resize((bytes.size() - 2) / sizeof(wchar_t));
        std::memcpy(text.data(), bytes.data() + 2, text.size() * sizeof(wchar_t));
        if (text.empty() || !WideCharToMultiByte(CP_UTF8, WC_ERR_INVALID_CHARS, text.data(),
            static_cast<int>(text.size()), nullptr, 0, nullptr, nullptr))
            return fail(error, "invalid UTF-16LE configuration");
    } else {
        const size_t skip = bytes.size() >= 3 && static_cast<unsigned char>(bytes[0]) == 0xef &&
            static_cast<unsigned char>(bytes[1]) == 0xbb && static_cast<unsigned char>(bytes[2]) == 0xbf ? 3 : 0;
        const int count = MultiByteToWideChar(CP_UTF8, MB_ERR_INVALID_CHARS, bytes.data() + skip,
            static_cast<int>(bytes.size() - skip), nullptr, 0);
        if (!count) return fail(error, "invalid UTF-8 configuration");
        text.resize(static_cast<size_t>(count));
        if (!MultiByteToWideChar(CP_UTF8, MB_ERR_INVALID_CHARS, bytes.data() + skip,
            static_cast<int>(bytes.size() - skip), text.data(), count))
            return fail(error, "invalid UTF-8 configuration");
    }
    for (wchar_t character : text)
        if ((character < 32 && character != L'\t' && character != L'\r' && character != L'\n') ||
            character == 127) return fail(error, "configuration contains control characters");
    return true;
}
}
bool thin_read_config(HANDLE file, ThinConfig& config, std::string& error)
{
    std::wstring text;
    if (!decode(file, text, error)) return false;
    std::wstring section;
    bool loader_seen = false;
    std::map<std::wstring, std::wstring> values;
    for (size_t begin = 0; begin < text.size();) {
        const auto end = text.find(L'\n', begin);
        const auto line = trim(text.substr(begin, end == std::wstring::npos ? end : end - begin));
        begin = end == std::wstring::npos ? text.size() : end + 1;
        if (line.empty() || line[0] == L';' || line[0] == L'#') continue;
        if (line[0] == L'[') {
            if (line.back() != L']' || line.find(L']') != line.size() - 1)
                return fail(error, "malformed INI section");
            section = lower(trim(line.substr(1, line.size() - 2)));
            if (section.empty()) return fail(error, "empty INI section");
            if (section == L"loader") {
                if (loader_seen) return fail(error, "duplicate Loader section");
                loader_seen = true;
            }
            continue;
        }
        const auto equals = line.find(L'=');
        if (section.empty() || equals == std::wstring::npos || trim(line.substr(0, equals)).empty())
            return fail(error, "malformed INI key/value");
        if (section != L"loader") continue;
        const auto key = lower(trim(line.substr(0, equals)));
        if (key != L"autostart" && key != L"runtimemodule" && key != L"entrymodule" && key != L"entrypoint")
            return fail(error, "unknown Loader key");
        if (!values.emplace(key, trim(line.substr(equals + 1))).second)
            return fail(error, "duplicate Loader key");
    }
    if (!loader_seen || !values.count(L"autostart")) return fail(error, "missing Loader AutoStart");
    const auto boolean = lower(values[L"autostart"]);
    if (boolean != L"1" && boolean != L"0" && boolean != L"true" && boolean != L"false")
        return fail(error, "invalid AutoStart boolean");
    config.auto_start = boolean == L"1" || boolean == L"true";
    if (values.count(L"runtimemodule")) config.runtime_module = values[L"runtimemodule"];
    if (values.count(L"entrymodule")) config.entry_module = values[L"entrymodule"];
    if (values.count(L"entrypoint")) {
        const auto& name = values[L"entrypoint"];
        if (name.empty() || name.size() > 255) return fail(error, "invalid entry point name");
        for (size_t i = 0; i < name.size(); ++i) {
            const auto c = name[i];
            if (!((c >= L'A' && c <= L'Z') || (c >= L'a' && c <= L'z') || c == L'_' ||
                (i && c >= L'0' && c <= L'9'))) return fail(error, "invalid entry point name");
            config.entry_point += static_cast<char>(c);
        }
    }
    if (config.auto_start && (config.runtime_module.empty() || config.entry_module.empty() || config.entry_point.empty()))
        return fail(error, "missing configured module or entry point");
    return true;
}
