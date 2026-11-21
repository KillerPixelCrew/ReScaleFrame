// SPDX-License-Identifier: MIT
#include "compatibility.h"
#include "observer_platform.h"
#include "postprocess_platform.h"
#include <rescaleframe/sha256_file.h>
#include <MinHook.h>
#include <windows.h>
#include <cstddef>
#include <string>
#if defined(_MSC_VER)
#include <intrin.h>
#endif

extern "C" {
using ProfileRead = bool (*)(uintptr_t, void*, size_t);
using Withdraw = void (*)() noexcept;
struct rsf_skyrim_cs_hdr_scope {
    rsf_skyrim_cs_hdr_call call;
    bool primary, active;
};
struct rsf_skyrim_cs_leaves {
    ProfileRead read;
    uintptr_t (*current_module)();
    uintptr_t (*borrow_module)(uintptr_t);
    void (*release_module)(uintptr_t);
    uint32_t (*module_path)(uintptr_t, wchar_t*, uint32_t);
    uint32_t (*file_kind)(const wchar_t*);
    bool (*profile)(const wchar_t*, uintptr_t);
    bool (*hook_initialize)();
    bool (*loader_callout)(uintptr_t);
    uint32_t (*initialize)(uintptr_t, const void*);
    const void* resolver_detour;
    uintptr_t initialize_detour;
    const void* configure_detour;
    const void* hdr_detour;
};

bool rsf_skyrim_cs_hash_matches(const char*, size_t) noexcept;
bool rsf_skyrim_cs_profile(const unsigned char*, size_t, uintptr_t, ProfileRead) noexcept;
bool rsf_skyrim_cs_checkpoint(const rsf_skyrim_creation_platform*, const rsf_skyrim_cs_leaves*, Withdraw, const char**) noexcept;
uintptr_t rsf_skyrim_cs_original(uint32_t slot) noexcept;
uintptr_t rsf_skyrim_cs_resolved(const rsf_skyrim_cs_leaves*, uintptr_t module, const char*, uintptr_t result) noexcept;
bool rsf_skyrim_cs_initialize(const rsf_skyrim_creation_platform*, const rsf_skyrim_cs_leaves*, const void*) noexcept;
uintptr_t rsf_skyrim_cs_excluded_byte(uintptr_t feature) noexcept;
bool rsf_skyrim_cs_render_allowed(const rsf_skyrim_cs_leaves*) noexcept;
const char* rsf_skyrim_cs_status(const rsf_skyrim_cs_leaves*) noexcept;
void rsf_skyrim_cs_hdr_enter(uintptr_t game, const rsf_skyrim_cs_hdr_call*, rsf_skyrim_cs_hdr_scope*) noexcept;
void rsf_skyrim_cs_hdr_leave(const rsf_skyrim_cs_hdr_scope*) noexcept;
bool rsf_skyrim_cs_postprocess_allowed(uintptr_t game, const rsf_skyrim_cs_hdr_call*, ProfileRead) noexcept;
}

static_assert(sizeof(wchar_t) == 2 && sizeof(rsf_skyrim_cs_leaves) == 112);
static_assert(sizeof(rsf_skyrim_cs_hdr_scope) == 48);
static_assert(sizeof(IMAGE_DOS_HEADER) == 64 && offsetof(IMAGE_DOS_HEADER, e_lfanew) == 0x3c);
static_assert(sizeof(IMAGE_NT_HEADERS64) == 264 && sizeof(IMAGE_OPTIONAL_HEADER64) == 240);
static_assert(offsetof(IMAGE_NT_HEADERS64, OptionalHeader) == 24 &&
    offsetof(IMAGE_OPTIONAL_HEADER64, SizeOfImage) == 56);
static_assert(sizeof(IMAGE_SECTION_HEADER) == 40 && offsetof(IMAGE_SECTION_HEADER, Characteristics) == 36);

namespace rsf::skyrim {
namespace {
using Resolve = decltype(&GetProcAddress);
using Initialize = bool (*)(const void*);
using Configure = void (*)(void*, void*);
using HdrProcess = void (*)(void*, uint32_t, uint32_t, uintptr_t, uint8_t);
using LoaderCallout = BOOLEAN (NTAPI*)();
constexpr auto cs_module_name = L"CommunityShaders.dll";

extern const rsf_skyrim_cs_leaves leaves;

bool read(uintptr_t address, void* output, size_t size) noexcept
{
    SIZE_T count{};
    return ReadProcessMemory(GetCurrentProcess(), reinterpret_cast<const void*>(address), output, size, &count) && count == size;
}

struct File { HANDLE value{INVALID_HANDLE_VALUE}; ~File() { if (value != INVALID_HANDLE_VALUE) CloseHandle(value); } };
struct Mapping { HANDLE value{}; ~Mapping() { if (value) CloseHandle(value); } };
struct View { const void* value{}; ~View() { if (value) UnmapViewOfFile(value); } };

bool profile(const wchar_t* path, uintptr_t module) noexcept try {
    // A read-only handle spans hashing and mapping so the file cannot be replaced or opened for writing.
    File file{CreateFileW(path, GENERIC_READ, FILE_SHARE_READ, nullptr, OPEN_EXISTING, FILE_ATTRIBUTE_NORMAL, nullptr)};
    if (file.value == INVALID_HANDLE_VALUE) return false;
    std::string hash;
    if (!rsf::sha256_file(path, hash) || !rsf_skyrim_cs_hash_matches(hash.data(), hash.size())) return false;
    LARGE_INTEGER size{};
    if (!GetFileSizeEx(file.value, &size) || size.QuadPart <= 0 || size.QuadPart > 128 * 1024 * 1024) return false;
    Mapping mapping{CreateFileMappingW(file.value, nullptr, PAGE_READONLY, 0, 0, nullptr)};
    if (!mapping.value) return false;
    View view{MapViewOfFile(mapping.value, FILE_MAP_READ, 0, 0, 0)};
    return view.value && rsf_skyrim_cs_profile(static_cast<const unsigned char*>(view.value), size_t(size.QuadPart), module, read);
} catch (...) { return false; }

FARPROC WINAPI resolve(HMODULE module, LPCSTR name)
{
    const auto original = reinterpret_cast<Resolve>(rsf_skyrim_cs_original(0));
    if (!original) return nullptr;
    const auto found = original(module, name);
    const auto error = GetLastError();
    const auto resolved = rsf_skyrim_cs_resolved(&leaves, uintptr_t(module), name, uintptr_t(found));
    SetLastError(error);
    return reinterpret_cast<FARPROC>(resolved);
}

bool initialize(const void* skse) noexcept
{
    return rsf_skyrim_cs_initialize(creation_platform(), &leaves, skse);
}

void configure(void* feature, void* json)
{
    // The original keeps parsing and validating desired settings. The guarded byte affects only lifecycle participation.
    const auto original = reinterpret_cast<Configure>(rsf_skyrim_cs_original(1));
    struct Exclude {
        unsigned char* loaded;
        ~Exclude() { if (loaded) *loaded = 0; }
    } exclusion{reinterpret_cast<unsigned char*>(rsf_skyrim_cs_excluded_byte(uintptr_t(feature)))};
    if (original) original(feature, json);
}

void hdr_process(void* manager, uint32_t input, uint32_t output, uintptr_t argument4, uint8_t argument5)
{
#if defined(_MSC_VER)
    const auto caller = uintptr_t(_ReturnAddress());
#else
    const auto caller = uintptr_t(__builtin_return_address(0));
#endif
    const auto original = reinterpret_cast<HdrProcess>(rsf_skyrim_cs_original(2));
    const rsf_skyrim_cs_hdr_call call{caller, uintptr_t(manager), argument4, input, output, argument5};
    struct Restore {
        rsf_skyrim_cs_hdr_scope saved{};
        ~Restore() { rsf_skyrim_cs_hdr_leave(&saved); }
    } restore;
    rsf_skyrim_cs_hdr_enter(uintptr_t(GetModuleHandleW(nullptr)), &call, &restore.saved);
    // Preserve CS's RedirectFramebuffer -> ProcessMain -> RestoreFramebuffer sequence.
    if (original) original(manager, input, output, argument4, argument5);
}

const rsf_skyrim_cs_leaves leaves{
    read,
    []() noexcept { return uintptr_t(GetModuleHandleW(cs_module_name)); },
    [](uintptr_t address) noexcept -> uintptr_t {
        HMODULE hold{};
        const auto flags = address ? GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS : 0u;
        const auto name = address ? reinterpret_cast<LPCWSTR>(address) : cs_module_name;
        return GetModuleHandleExW(flags, name, &hold) ? uintptr_t(hold) : 0;
    },
    [](uintptr_t module) noexcept { FreeLibrary(reinterpret_cast<HMODULE>(module)); },
    [](uintptr_t module, wchar_t* path, uint32_t capacity) noexcept {
        return uint32_t(GetModuleFileNameW(reinterpret_cast<HMODULE>(module), path, capacity));
    },
    [](const wchar_t* path) noexcept -> uint32_t {
        const auto attributes = GetFileAttributesW(path);
        if (attributes != INVALID_FILE_ATTRIBUTES) return (attributes & FILE_ATTRIBUTE_DIRECTORY) ? 3u : 2u;
        const auto error = GetLastError();
        return (error == ERROR_FILE_NOT_FOUND || error == ERROR_PATH_NOT_FOUND) ? 1u : 0u;
    },
    profile,
    []() noexcept {
        const auto status = MH_Initialize();
        return status == MH_OK || status == MH_ERROR_ALREADY_INITIALIZED;
    },
    [](uintptr_t callout) noexcept { return reinterpret_cast<LoaderCallout>(callout)() != 0; },
    [](uintptr_t original, const void* skse) noexcept -> uint32_t {
        try { return original && reinterpret_cast<Initialize>(original)(skse) ? 1u : 0u; } catch (...) { return 2u; }
    },
    reinterpret_cast<const void*>(&resolve),
    reinterpret_cast<uintptr_t>(&initialize),
    reinterpret_cast<const void*>(&configure),
    reinterpret_cast<const void*>(&hdr_process),
};
}

bool compatibility_checkpoint(const char*& refusal, void (*withdraw)() noexcept) noexcept
{
    return rsf_skyrim_cs_checkpoint(creation_platform(), &leaves, withdraw, &refusal);
}

bool compatibility_render_allowed() noexcept { return rsf_skyrim_cs_render_allowed(&leaves); }
const char* compatibility_status() noexcept { return rsf_skyrim_cs_status(&leaves); }

bool compatibility_postprocess_call(uintptr_t caller, void* manager, uint32_t input, uint32_t output,
    uintptr_t argument4, uint8_t argument5) noexcept
{
    const rsf_skyrim_cs_hdr_call call{caller, uintptr_t(manager), argument4, input, output, argument5};
    return rsf_skyrim_cs_postprocess_allowed(uintptr_t(GetModuleHandleW(nullptr)), &call, read);
}
}
