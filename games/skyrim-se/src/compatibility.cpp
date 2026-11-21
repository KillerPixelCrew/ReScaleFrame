// SPDX-License-Identifier: MIT
#include "compatibility.h"
#include <rescaleframe/sha256_file.h>
#include <MinHook.h>
#include <windows.h>
#include <cstddef>
#include <atomic>
#include <cstdint>
#include <cstring>
#include <cwchar>
#include <iterator>
#include <string>
#if defined(_MSC_VER)
#include <intrin.h>
#endif

extern "C" {
using ProfileRead = bool (*)(uintptr_t, void*, size_t);
struct rsf_skyrim_cs_detour_sites { uintptr_t configure, initialize, hdr_process; };
struct rsf_skyrim_cs_hdr_call {
    uintptr_t caller, manager, argument4;
    uint32_t input, output;
    uint8_t argument5;
};
struct rsf_skyrim_cs_hdr_scope {
    rsf_skyrim_cs_hdr_call call;
    bool primary, active;
};

bool rsf_skyrim_cs_hash_matches(const char*, size_t) noexcept;
bool rsf_skyrim_cs_profile(const unsigned char*, size_t, uintptr_t, ProfileRead) noexcept;
rsf_skyrim_cs_detour_sites rsf_skyrim_cs_sites() noexcept;
bool rsf_skyrim_cs_before_device(uintptr_t game, ProfileRead) noexcept;
bool rsf_skyrim_cs_features_inactive(uintptr_t module, bool check_hdr, ProfileRead) noexcept;
uintptr_t rsf_skyrim_cs_excluded_byte(uintptr_t module, uintptr_t feature) noexcept;
void rsf_skyrim_cs_hdr_enter(uintptr_t game, const rsf_skyrim_cs_hdr_call*, rsf_skyrim_cs_hdr_scope*) noexcept;
void rsf_skyrim_cs_hdr_leave(const rsf_skyrim_cs_hdr_scope*) noexcept;
bool rsf_skyrim_cs_postprocess_allowed(uintptr_t game, uintptr_t module, bool excluded,
    const rsf_skyrim_cs_hdr_call*, ProfileRead) noexcept;
}

static_assert(sizeof(rsf_skyrim_cs_detour_sites) == 24);
static_assert(sizeof(rsf_skyrim_cs_hdr_call) == 40 && offsetof(rsf_skyrim_cs_hdr_call, argument5) == 32);
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
Resolve resolve_original{};
Configure configure_original{};
HdrProcess hdr_original{};
std::atomic<void (*)() noexcept> withdraw_source{};
SRWLOCK capture_lock = SRWLOCK_INIT;
LoaderCallout loader_callout{};
std::atomic<HMODULE> cs_module{};
std::atomic<Initialize> initialize_original{};
enum class State { unarmed, armed, captured, initializing, excluded, refused };
std::atomic<State> state{State::unarmed};
std::atomic<const char*> detail{};
bool initialize(const void*);
FARPROC WINAPI resolve(HMODULE, LPCSTR);
void hdr_process(void*, uint32_t, uint32_t, uintptr_t, uint8_t);
bool read(uintptr_t address, void* output, size_t size) noexcept {
    SIZE_T count{};
    return ReadProcessMemory(GetCurrentProcess(), reinterpret_cast<const void*>(address), output, size, &count) && count == size;
}
bool fail(const char* reason) noexcept {
    detail.store(reason, std::memory_order_release);
    state.store(State::refused, std::memory_order_release); return false;
}
struct Module { HMODULE value{}; ~Module() { if (value) FreeLibrary(value); } };
struct File { HANDLE value{INVALID_HANDLE_VALUE}; ~File() { if (value != INVALID_HANDLE_VALUE) CloseHandle(value); } };
struct Mapping { HANDLE value{}; ~Mapping() { if (value) CloseHandle(value); } };
struct View { const void* value{}; ~View() { if (value) UnmapViewOfFile(value); } };
bool before_device() noexcept {
    return rsf_skyrim_cs_before_device(uintptr_t(GetModuleHandleW(nullptr)), read);
}
bool retain_cs(const char* reason, Initialize original, const void* argument) {
    fail(reason);
    if (const auto withdraw = withdraw_source.load(std::memory_order_acquire)) withdraw();
    // Only pre-device failures can forward safely without an active RSF graphics
    // owner. Late loading remains refused; never start a second live owner.
    return original && before_device() ? original(argument) : false;
}
bool profile(const wchar_t* path, HMODULE module) {
    // Keep a read-only shared handle across hashing/mapping to refuse replacement
    // or a writer during inspection. This never maps the DLL as executable code.
    File file{CreateFileW(path, GENERIC_READ, FILE_SHARE_READ, nullptr, OPEN_EXISTING, FILE_ATTRIBUTE_NORMAL, nullptr)};
    if (file.value == INVALID_HANDLE_VALUE) return false;
    std::string hash;
    if (!rsf::sha256_file(path, hash) || !rsf_skyrim_cs_hash_matches(hash.data(), hash.size())) return false;
    LARGE_INTEGER size{};
    if (!GetFileSizeEx(file.value, &size) || size.QuadPart <= 0 || size.QuadPart > 128 * 1024 * 1024) return false;
    Mapping mapping{CreateFileMappingW(file.value, nullptr, PAGE_READONLY, 0, 0, nullptr)};
    if (!mapping.value) return false;
    View view{MapViewOfFile(mapping.value, FILE_MAP_READ, 0, 0, 0)};
    return view.value && rsf_skyrim_cs_profile(static_cast<const unsigned char*>(view.value),
        size_t(size.QuadPart), uintptr_t(module), read);
}
void configure(void* feature, void* json) {
    // Original loads desired settings, including CS's normal validation. Only
    // effective lifecycle participation changes, before its caller can dispatch.
    struct Exclude {
        unsigned char* loaded;
        ~Exclude() { if (loaded) *loaded = 0; }
    } exclusion{reinterpret_cast<unsigned char*>(rsf_skyrim_cs_excluded_byte(uintptr_t(cs_module.load()), uintptr_t(feature)))};
    configure_original(feature, json);
}
void hdr_process(void* manager, uint32_t input, uint32_t output, uintptr_t argument4, uint8_t argument5) {
#if defined(_MSC_VER)
    const auto caller = uintptr_t(_ReturnAddress());
#else
    const auto caller = uintptr_t(__builtin_return_address(0));
#endif
    const rsf_skyrim_cs_hdr_call call{caller, uintptr_t(manager), argument4, input, output, argument5};
    struct Restore {
        rsf_skyrim_cs_hdr_scope saved{};
        ~Restore() { rsf_skyrim_cs_hdr_leave(&saved); }
    } restore;
    rsf_skyrim_cs_hdr_enter(uintptr_t(GetModuleHandleW(nullptr)), &call, &restore.saved);
    // CS owns RedirectFramebuffer -> original ProcessMain -> RestoreFramebuffer.
    // Keep its fifth stack byte unchanged; do not implement either side ourselves.
    hdr_original(manager, input, output, argument4, argument5);
}
bool initialize(const void* skse) try {
    // The resolver does no patching. Reject an initializer called from DllMain
    // or a loader callout before any file I/O, pinning or MinHook operation.
    if (!loader_callout || loader_callout()) return fail("CS handover refused: initializer entered from a loader callout.");
    const auto module = cs_module.load();
    const auto base = uintptr_t(module);
    const auto original = initialize_original.load();
    Module borrowed{};
    if (!module || !GetModuleHandleExW(GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS,
            reinterpret_cast<LPCWSTR>(module), &borrowed.value) || borrowed.value != module)
        return fail("CS initializer module no longer available; ownership refused.");
    auto expected = State::captured;
    if (!state.compare_exchange_strong(expected, State::initializing)) {
        fail("CS handover refused: initializer reentry or uncertain prior initialization.");
        if (const auto withdraw = withdraw_source.load(std::memory_order_acquire)) withdraw();
        return false;
    }
    wchar_t path[32768]{};
    const auto length = GetModuleFileNameW(module, path, DWORD(std::size(path)));
    // Refuse late takeover after the game already created its renderer/device.
    const auto sites = rsf_skyrim_cs_sites();
    if (original != reinterpret_cast<Initialize>(base + sites.initialize) ||
        !length || length >= std::size(path) || !profile(path, module) ||
        !rsf_skyrim_cs_features_inactive(base, false, read) ||
        !before_device())
        return retain_cs("CS profile/dormancy/startup guard failed; RSF withdrawn, original CS retained only before device creation.", original, skse);
    HMODULE pinned{};
    if (!GetModuleHandleExW(GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_PIN,
            reinterpret_cast<LPCWSTR>(module), &pinned))
        return retain_cs("CS handover module lifetime unavailable; RSF withdrawn.", original, skse);
    auto* target = reinterpret_cast<void*>(base + sites.configure);
    if (MH_CreateHook(target, reinterpret_cast<void*>(&configure), reinterpret_cast<void**>(&configure_original)) != MH_OK)
        return retain_cs("CS Feature configuration hook unavailable; RSF withdrawn.", original, skse);
    auto* hdr_target = reinterpret_cast<void*>(base + sites.hdr_process);
    const bool hdr_created = MH_CreateHook(hdr_target, reinterpret_cast<void*>(&hdr_process), reinterpret_cast<void**>(&hdr_original)) == MH_OK;
    if (!hdr_created ||
        MH_EnableHook(target) != MH_OK || MH_EnableHook(hdr_target) != MH_OK) {
        MH_DisableHook(target); MH_RemoveHook(target);
        if (hdr_created) { MH_DisableHook(hdr_target); MH_RemoveHook(hdr_target); }
        configure_original = nullptr; hdr_original = nullptr;
        return retain_cs("CS configuration/HDR chain hook activation failed; RSF withdrawn.", original, skse);
    }
    // No adapter lock is held across the original initializer. Its other features
    // and core D3D fallback execute normally; no CS SDK entry is called by RSF.
    const bool result = original && original(skse);
    if (!result || !rsf_skyrim_cs_features_inactive(base, false, read)) {
        fail("CS handover refused: original initialization failed or Upscaling exclusion was lost.");
        if (const auto withdraw = withdraw_source.load(std::memory_order_acquire)) withdraw();
        return result; // Never make SKSE discard a CS initializer that already installed core hooks.
    }
    expected = State::initializing;
    if (!state.compare_exchange_strong(expected, State::excluded)) return result;
    detail.store("CS 1.9.1 Upscaling lifecycle excluded; original configuration and other feature initialization forwarded; runtime coexistence unvalidated.",
        std::memory_order_release);
    return result;
} catch (...) {
    fail("CS initializer exception; RSF coexistence refused for this process.");
    if (const auto withdraw = withdraw_source.load(std::memory_order_acquire)) withdraw();
    return false;
}
FARPROC WINAPI resolve(HMODULE module, LPCSTR name) {
    const auto result = resolve_original(module, name);
    const auto error = GetLastError();
    // The actual caller still invokes the initializer synchronously, after its
    // LoadLibrary has returned. No hash, MinHook, SDK, allocation or log here.
    if (result && uintptr_t(name) > 0xffff && !std::strcmp(name, "SKSEPlugin_Load") &&
        module == GetModuleHandleW(L"CommunityShaders.dll")) {
        // Publish the complete scalar record before any thread can enter the
        // wrapper. No native call, hashing or patching occurs under this lock.
        bool captured = false;
        AcquireSRWLockExclusive(&capture_lock);
        if (state.load() == State::armed) {
            cs_module.store(module);
            initialize_original.store(reinterpret_cast<Initialize>(result));
            state.store(State::captured, std::memory_order_release);
            captured = true;
        }
        ReleaseSRWLockExclusive(&capture_lock);
        SetLastError(error);
        return captured ? reinterpret_cast<FARPROC>(&initialize) : result;
    }
    SetLastError(error); return result;
}
}
bool compatibility_checkpoint(const char*& refusal, void (*withdraw)() noexcept) noexcept try {
    if (withdraw) withdraw_source.store(withdraw, std::memory_order_release);
    if (state.load(std::memory_order_acquire) != State::unarmed) {
        if (state.load(std::memory_order_acquire) != State::refused) return true;
        refusal = detail.load(std::memory_order_acquire); return false;
    }
    Module module{};
    wchar_t path[32768]{};
    if (GetModuleHandleExW(0, L"CommunityShaders.dll", &module.value)) {
        const auto length = GetModuleFileNameW(module.value, path, DWORD(std::size(path)));
        if (!length || length >= std::size(path) || !profile(path, module.value)) {
            refusal = "Community Shaders refused: loaded module differs from the guarded 1.9.1 profile or cannot be inspected.";
        } else {
            // loaded=false cannot prove Load/PostPostLoad or SDK initialization never ran.
            refusal = "Community Shaders 1.9.1 already present before interception; prior initialization cannot be excluded.";
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
        if (error != ERROR_FILE_NOT_FOUND && error != ERROR_PATH_NOT_FOUND) {
            refusal = "Community Shaders presence check failed: installed plugin path unreadable."; return false;
        }
    } else if ((attributes & FILE_ATTRIBUTE_DIRECTORY) || !profile(candidate.c_str(), nullptr)) {
        refusal = "Community Shaders installed profile unknown or unreadable; early handover refused."; return false;
    }
    const auto initialized = MH_Initialize();
    if (initialized != MH_OK && initialized != MH_ERROR_ALREADY_INITIALIZED) {
        refusal = "CS initializer interception could not initialize native hooks."; return false;
    }
    const auto kernel = GetModuleHandleW(L"kernel32.dll");
    auto* target = reinterpret_cast<void*>(GetProcAddress(kernel, "GetProcAddress"));
    loader_callout = reinterpret_cast<LoaderCallout>(GetProcAddress(GetModuleHandleW(L"ntdll.dll"), "RtlIsThreadWithinLoaderCallout"));
    HMODULE pinned{};
    if (!target || !loader_callout || loader_callout() ||
        !GetModuleHandleExW(GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_PIN,
            reinterpret_cast<LPCWSTR>(&resolve), &pinned) ||
        MH_CreateHook(target, reinterpret_cast<void*>(&resolve), reinterpret_cast<void**>(&resolve_original)) != MH_OK) {
        refusal = "CS initializer interception unavailable outside loader callout."; return false;
    }
    state.store(State::armed, std::memory_order_release);
    detail.store("CS initializer interception armed before module load; awaiting exact-profile initialization if present.");
    if (MH_EnableHook(target) != MH_OK) {
        MH_RemoveHook(target); resolve_original = nullptr;
        fail("CS initializer interception could not be enabled."); refusal = detail.load(); return false;
    }
    // If lookup escaped before the detour became active, do not infer freshness
    // from a currently-zero Feature.loaded byte.
    if (GetModuleHandleW(L"CommunityShaders.dll") && state.load() == State::armed) {
        fail("CS appeared before initializer interception completed; startup ordering ambiguous."); refusal = detail.load(); return false;
    }
    return true;
} catch (...) {
    fail("Community Shaders profile inspection failed; ownership remains refused."); refusal = detail.load(); return false;
}
bool compatibility_render_allowed() noexcept {
    const auto module = GetModuleHandleW(L"CommunityShaders.dll");
    if (!module) return state.load(std::memory_order_acquire) != State::refused;
    return state.load(std::memory_order_acquire) == State::excluded && module == cs_module.load() &&
        rsf_skyrim_cs_features_inactive(uintptr_t(module), true, read);
}
const char* compatibility_status() noexcept {
    if (state.load(std::memory_order_acquire) == State::excluded && !compatibility_render_allowed())
        return "CS HDR chain preserved; float16 scene readers, PQ/HDR setup metadata and post-composite HUD-less lease still required; RSF HDR processing refused.";
    if (state.load(std::memory_order_acquire) == State::refused || GetModuleHandleW(L"CommunityShaders.dll"))
        return detail.load(std::memory_order_acquire);
    return nullptr;
}
bool compatibility_postprocess_call(uintptr_t caller, void* manager, uint32_t input, uint32_t output,
    uintptr_t argument4, uint8_t argument5) noexcept {
    const rsf_skyrim_cs_hdr_call call{caller, uintptr_t(manager), argument4, input, output, argument5};
    return rsf_skyrim_cs_postprocess_allowed(uintptr_t(GetModuleHandleW(nullptr)), uintptr_t(cs_module.load()),
        state.load(std::memory_order_acquire) == State::excluded, &call, read);
}
}
