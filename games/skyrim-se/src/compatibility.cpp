// SPDX-License-Identifier: GPL-3.0-only
#include "compatibility.h"
#include <rescaleframe/sha256_file.h>
#include <windows.h>
#include <array>
#include <cstring>
#include <cwchar>
#include <iterator>
#include <string>

namespace rsf::skyrim {
namespace {
constexpr char release_hash[] = "10ba95e6b1cd6b675ce74d1d681fae41e667bdd8b7b5ba8c362e657511097827";
constexpr DWORD image_size = 0x144b000;
struct Guard { DWORD rva; std::array<unsigned char, 19> bytes; };
constexpr std::array<Guard, 4> guards{{
    {0x3a4b30, {0x48,0x89,0x54,0x24,0x10,0x48,0x89,0x4c,0x24,0x08,0x53,0x56,0x57,0x41,0x54,0x41,0x55,0x41,0x56}},
    {0x4f9380, {0x48,0x83,0xec,0x48,0x48,0x8d,0x05,0xad,0x61,0x21,0x00,0x48,0xc7,0x44,0x24,0x28,0x1d,0x00,0x00}},
    {0x4f8ed0, {0x40,0x55,0x53,0x56,0x57,0x41,0x56,0x48,0x8b,0xec,0x48,0x81,0xec,0x80,0x00,0x00,0x00,0x48,0x8d}},
    {0x4f7030, {0x48,0x89,0x6c,0x24,0x10,0x48,0x89,0x74,0x24,0x18,0x57,0x48,0x83,0xec,0x60,0x80,0x3d,0x1d,0x21}}
}};
struct Module { HMODULE value{}; ~Module() { if (value) FreeLibrary(value); } };
struct File { HANDLE value{INVALID_HANDLE_VALUE}; ~File() { if (value != INVALID_HANDLE_VALUE) CloseHandle(value); } };
struct Mapping { HANDLE value{}; ~Mapping() { if (value) CloseHandle(value); } };
struct View { const void* value{}; ~View() { if (value) UnmapViewOfFile(value); } };
struct Image {
    const unsigned char* bytes{};
    size_t size{};
    bool read(size_t offset, void* output, size_t count) const noexcept {
        if (offset > size || count > size - offset) return false;
        std::memcpy(output, bytes + offset, count); return true;
    }
    bool validate() const noexcept {
        IMAGE_DOS_HEADER dos{}; IMAGE_NT_HEADERS64 nt{};
        if (!read(0, &dos, sizeof(dos)) || dos.e_magic != IMAGE_DOS_SIGNATURE || dos.e_lfanew < 0 ||
            !read(size_t(dos.e_lfanew), &nt, sizeof(nt)) || nt.Signature != IMAGE_NT_SIGNATURE ||
            nt.FileHeader.Machine != IMAGE_FILE_MACHINE_AMD64 || nt.FileHeader.TimeDateStamp != 0x6ab51a9f ||
            nt.FileHeader.SizeOfOptionalHeader != sizeof(IMAGE_OPTIONAL_HEADER64) ||
            nt.OptionalHeader.Magic != IMAGE_NT_OPTIONAL_HDR64_MAGIC || nt.OptionalHeader.SizeOfImage != image_size ||
            !nt.FileHeader.NumberOfSections || nt.FileHeader.NumberOfSections > 96) return false;
        const auto sections = size_t(dos.e_lfanew) + sizeof(nt);
        for (const auto& guard : guards) {
            bool matched = false;
            for (unsigned i = 0; i < nt.FileHeader.NumberOfSections; ++i) {
                IMAGE_SECTION_HEADER section{};
                if (!read(sections + i * sizeof(section), &section, sizeof(section))) return false;
                if (guard.rva < section.VirtualAddress) continue;
                const auto delta = size_t(guard.rva - section.VirtualAddress);
                if (delta > section.SizeOfRawData || guard.bytes.size() > section.SizeOfRawData - delta) continue;
                std::array<unsigned char, 19> actual{};
                if (!read(size_t(section.PointerToRawData) + delta, actual.data(), actual.size()) ||
                    actual != guard.bytes || !(section.Characteristics & IMAGE_SCN_MEM_EXECUTE)) return false;
                matched = true; break;
            }
            if (!matched) return false;
        }
        return true;
    }
};
bool profile(const wchar_t* path, HMODULE module) {
    // Keep a read-only shared handle across hashing/mapping to refuse replacement
    // or a writer during inspection. This never maps the DLL as executable code.
    File file{CreateFileW(path, GENERIC_READ, FILE_SHARE_READ, nullptr, OPEN_EXISTING, FILE_ATTRIBUTE_NORMAL, nullptr)};
    if (file.value == INVALID_HANDLE_VALUE) return false;
    std::string hash;
    if (!rsf::sha256_file(path, hash) || hash != release_hash) return false;
    LARGE_INTEGER size{};
    if (!GetFileSizeEx(file.value, &size) || size.QuadPart <= 0 || size.QuadPart > 128 * 1024 * 1024) return false;
    Mapping mapping{CreateFileMappingW(file.value, nullptr, PAGE_READONLY, 0, 0, nullptr)};
    if (!mapping.value) return false;
    View view{MapViewOfFile(mapping.value, FILE_MAP_READ, 0, 0, 0)};
    if (!view.value || !Image{static_cast<const unsigned char*>(view.value), size_t(size.QuadPart)}.validate()) return false;
    if (module) for (const auto& guard : guards) {
        std::array<unsigned char, 19> actual{}; SIZE_T count{};
        if (!ReadProcessMemory(GetCurrentProcess(), reinterpret_cast<const unsigned char*>(module) + guard.rva,
                actual.data(), actual.size(), &count) || count != actual.size() || actual != guard.bytes) return false;
    }
    return true;
}
}
bool compatibility_checkpoint(const char*& refusal) noexcept try {
    Module module{};
    wchar_t path[32768]{};
    if (GetModuleHandleExW(0, L"CommunityShaders.dll", &module.value)) {
        const auto length = GetModuleFileNameW(module.value, path, DWORD(std::size(path)));
        if (!length || length >= std::size(path) || !profile(path, module.value)) {
            refusal = "Community Shaders refused: loaded module differs from the guarded 1.9.1 profile or cannot be inspected.";
        } else {
            // loaded=false cannot prove Load/PostPostLoad or SDK initialization never ran.
            refusal = "Community Shaders 1.9.1 verified; pre-initialization ownership barrier absent, handover refused.";
        }
        return false;
    }
    const auto length = GetModuleFileNameW(nullptr, path, DWORD(std::size(path)));
    if (!length || length >= std::size(path)) {
        refusal = "Community Shaders presence check failed: executable directory unavailable."; return false;
    }
    const auto* leaf = std::wcsrchr(path, L'\\');
    if (!leaf) { refusal = "Community Shaders presence check failed: executable directory unavailable."; return false; }
    const auto candidate = std::wstring(path, size_t(leaf - path) + 1) + L"Data\\SKSE\\Plugins\\CommunityShaders.dll";
    const auto attributes = GetFileAttributesW(candidate.c_str());
    if (attributes == INVALID_FILE_ATTRIBUTES) {
        const auto error = GetLastError();
        if (error == ERROR_FILE_NOT_FOUND || error == ERROR_PATH_NOT_FOUND) return true;
        refusal = "Community Shaders presence check failed: installed plugin path unreadable."; return false;
    }
    refusal = !(attributes & FILE_ATTRIBUTE_DIRECTORY) && profile(candidate.c_str(), nullptr) ?
        "Community Shaders 1.9.1 installed but not loaded; activation refused until an ordered early handover exists." :
        "Community Shaders installed profile unknown or unreadable; activation refused before its later SKSE load.";
    return false;
} catch (...) {
    refusal = "Community Shaders profile inspection failed; ownership remains refused."; return false;
}
}
