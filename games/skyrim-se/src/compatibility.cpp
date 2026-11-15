// SPDX-License-Identifier: GPL-3.0-only
#include "compatibility.h"
#include <rescaleframe/sha256_file.h>
#include <MinHook.h>
#include <windows.h>
#include <array>
#include <atomic>
#include <cstdint>
#include <cstring>
#include <cwchar>
#include <iterator>
#include <string>
#if defined(_MSC_VER)
#include <intrin.h>
#endif

namespace rsf::skyrim {
namespace {
constexpr char release_hash[] = "10ba95e6b1cd6b675ce74d1d681fae41e667bdd8b7b5ba8c362e657511097827";
constexpr DWORD image_size = 0x144b000;
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
constexpr uintptr_t upscaling = 0x13dcf40;
constexpr uintptr_t hdr_display = 0x13dcda0;
constexpr uintptr_t streamline = 0xfd9160;
struct HdrScope {
    void* manager;
    uint32_t input, output;
    uintptr_t argument4;
    uint8_t argument5;
    bool primary;
};
thread_local HdrScope* hdr_scope{};
bool initialize(const void*);
FARPROC WINAPI resolve(HMODULE, LPCSTR);
void hdr_process(void*, uint32_t, uint32_t, uintptr_t, uint8_t);
bool read(uintptr_t address, void* output, size_t size) noexcept {
    SIZE_T count{};
    return ReadProcessMemory(GetCurrentProcess(), reinterpret_cast<const void*>(address), output, size, &count) && count == size;
}
bool dormant(uintptr_t base) noexcept {
    unsigned char flags[3]{};
    return read(base + streamline, flags, sizeof(flags)) && !flags[0] && !flags[1] && !flags[2];
}
bool fail(const char* reason) noexcept {
    detail.store(reason, std::memory_order_release);
    state.store(State::refused, std::memory_order_release); return false;
}
struct Guard { DWORD rva; std::array<unsigned char, 19> bytes; };
constexpr std::array<Guard, 6> guards{{
    {0x3a4b30, {0x48,0x89,0x54,0x24,0x10,0x48,0x89,0x4c,0x24,0x08,0x53,0x56,0x57,0x41,0x54,0x41,0x55,0x41,0x56}},
    {0x4f9380, {0x48,0x83,0xec,0x48,0x48,0x8d,0x05,0xad,0x61,0x21,0x00,0x48,0xc7,0x44,0x24,0x28,0x1d,0x00,0x00}},
    {0x4f8ed0, {0x40,0x55,0x53,0x56,0x57,0x41,0x56,0x48,0x8b,0xec,0x48,0x81,0xec,0x80,0x00,0x00,0x00,0x48,0x8d}},
    {0x4f7030, {0x48,0x89,0x6c,0x24,0x10,0x48,0x89,0x74,0x24,0x18,0x57,0x48,0x83,0xec,0x60,0x80,0x3d,0x1d,0x21}},
    {0x631800, {0x48,0x89,0x5c,0x24,0x08,0x55,0x56,0x57,0x41,0x54,0x41,0x55,0x41,0x56,0x41,0x57,0x48,0x8d,0xac}},
    {0x45cbd0, {0x40,0x53,0x55,0x56,0x57,0x48,0x83,0xec,0x38,0x48,0x8b,0xe9,0x49,0x8b,0xd9,0x48,0x8d,0x0d,0xba}}
}};
struct Module { HMODULE value{}; ~Module() { if (value) FreeLibrary(value); } };
struct File { HANDLE value{INVALID_HANDLE_VALUE}; ~File() { if (value != INVALID_HANDLE_VALUE) CloseHandle(value); } };
struct Mapping { HANDLE value{}; ~Mapping() { if (value) CloseHandle(value); } };
struct View { const void* value{}; ~View() { if (value) UnmapViewOfFile(value); } };
bool before_device() noexcept {
    uintptr_t owner{}, device{};
    const auto game = uintptr_t(GetModuleHandleW(nullptr));
    return read(game + 0x3330188, &owner, sizeof(owner)) &&
        (!owner || (read(owner + 56, &device, sizeof(device)) && !device));
}
bool retain_cs(const char* reason, Initialize original, const void* argument) {
    fail(reason);
    if (const auto withdraw = withdraw_source.load(std::memory_order_acquire)) withdraw();
    // Only pre-device failures can forward safely without an active RSF graphics
    // owner. Late loading remains refused; never start a second live owner.
    return original && before_device() ? original(argument) : false;
}
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
void configure(void* feature, void* json) {
    // Original loads desired settings, including CS's normal validation. Only
    // effective lifecycle participation changes, before its caller can dispatch.
    struct Exclude {
        unsigned char* loaded;
        ~Exclude() { if (loaded) *loaded = 0; }
    } exclusion{uintptr_t(feature) == uintptr_t(cs_module.load()) + upscaling ?
        static_cast<unsigned char*>(feature) + 8 : nullptr};
    configure_original(feature, json);
}
void hdr_process(void* manager, uint32_t input, uint32_t output, uintptr_t argument4, uint8_t argument5) {
#if defined(_MSC_VER)
    const auto caller = uintptr_t(_ReturnAddress());
#else
    const auto caller = uintptr_t(__builtin_return_address(0));
#endif
    HdrScope scope{manager, input, output, argument4, argument5,
        !hdr_scope && caller == uintptr_t(GetModuleHandleW(nullptr)) + 0x153adec};
    struct Restore {
        HdrScope* previous;
        ~Restore() { hdr_scope = previous; }
    } restore{hdr_scope};
    hdr_scope = &scope;
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
    unsigned char loaded{};
    // Refuse late takeover after the game already created its renderer/device.
    if (original != reinterpret_cast<Initialize>(base + 0x631800) ||
        !length || length >= std::size(path) || !profile(path, module) ||
        !read(base + upscaling + 8, &loaded, sizeof(loaded)) || loaded || !dormant(base) ||
        !before_device())
        return retain_cs("CS profile/dormancy/startup guard failed; RSF withdrawn, original CS retained only before device creation.", original, skse);
    HMODULE pinned{};
    if (!GetModuleHandleExW(GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_PIN,
            reinterpret_cast<LPCWSTR>(module), &pinned))
        return retain_cs("CS handover module lifetime unavailable; RSF withdrawn.", original, skse);
    auto* target = reinterpret_cast<void*>(base + 0x3a4b30);
    if (MH_CreateHook(target, reinterpret_cast<void*>(&configure), reinterpret_cast<void**>(&configure_original)) != MH_OK)
        return retain_cs("CS Feature configuration hook unavailable; RSF withdrawn.", original, skse);
    auto* hdr_target = reinterpret_cast<void*>(base + 0x45cbd0);
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
    if (!result || !read(base + upscaling + 8, &loaded, sizeof(loaded)) || loaded || !dormant(base)) {
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
        AcquireSRWLockExclusive(&capture_lock);
        if (state.load() == State::armed) {
            cs_module.store(module);
            initialize_original.store(reinterpret_cast<Initialize>(result));
            state.store(State::captured, std::memory_order_release);
        }
        ReleaseSRWLockExclusive(&capture_lock);
        SetLastError(error);
        return reinterpret_cast<FARPROC>(&initialize);
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
    unsigned char loaded{}, hdr{};
    const auto base = uintptr_t(module);
    return state.load(std::memory_order_acquire) == State::excluded && module == cs_module.load() &&
        read(base + upscaling + 8, &loaded, sizeof(loaded)) && !loaded && dormant(base) &&
        read(base + hdr_display + 8, &hdr, sizeof(hdr)) && !hdr;
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
    const auto game = uintptr_t(GetModuleHandleW(nullptr));
    if (!hdr_scope) return caller == game + 0x153adec;
    const auto base = uintptr_t(cs_module.load());
    uintptr_t original{};
    constexpr unsigned char expected_call[]{0xff,0x15,0x33,0x20,0xfa,0x00};
    unsigned char actual_call[sizeof(expected_call)]{};
    return state.load(std::memory_order_acquire) == State::excluded && hdr_scope->primary &&
        caller == base + 0x45cc0d && manager == hdr_scope->manager && input == hdr_scope->input &&
        output == hdr_scope->output && argument4 == hdr_scope->argument4 && argument5 == hdr_scope->argument5 &&
        read(base + 0x45cc07, actual_call, sizeof(actual_call)) &&
        !std::memcmp(actual_call, expected_call, sizeof(actual_call)) &&
        read(base + 0x13fec40, &original, sizeof(original)) && original == game + 0x14edbf0;
}
}
