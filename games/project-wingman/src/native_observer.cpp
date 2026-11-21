// SPDX-License-Identifier: MIT
#include "native_observer.h"
#include "native_execution.h"
#include "native_read.h"
#include <rescaleframe/sha256_file.h>
#include <MinHook.h>
#include <cwchar>

extern "C" {
bool rsf_wingman_matches_executable(const wchar_t*) noexcept;
bool rsf_wingman_matches_sha256(const char*) noexcept;
void rsf_wingman_observer_enter() noexcept;
void rsf_wingman_observer_leave() noexcept;
void rsf_wingman_observe(const void* graph, const void* view, const void* input,
    const void* output, const void* output_rect, const void* half, const void* half_rect) noexcept;
}

namespace rsf::wingman {
namespace {
struct Rect { int32_t min_x, min_y, max_x, max_y; };
struct Inputs;
using AddPasses = void (*)(const void*, void*, const void*, const Inputs*,
    void**, Rect*, void**, Rect*);
AddPasses original = nullptr;

void add_passes(const void* self, void* graph, const void* view, const Inputs* input,
    void** output, Rect* output_rect, void** half, Rect* half_rect)
{
    rsf_wingman_observer_enter();
    struct Call { ~Call() { rsf_wingman_observer_leave(); } } call;
    original(self, graph, view, input, output, output_rect, half, half_rect);
    rsf_wingman_observe(graph, view, input, output, output_rect, half, half_rect);
}

bool verify_executable() noexcept try
{
    wchar_t executable[32768]{};
    const auto length = GetModuleFileNameW(nullptr, executable, 32768);
    if (!length || length >= 32768) return false;
    const auto* leaf = wcsrchr(executable, L'\\');
    if (!rsf_wingman_matches_executable(leaf ? leaf + 1 : executable)) return false;
    std::string digest;
    return rsf::sha256_file(executable, digest) && rsf_wingman_matches_sha256(digest.c_str());
}
catch (...) { return false; }

bool create_hook(uintptr_t target) noexcept
{
    if (original) return true;
    const auto initialized = MH_Initialize();
    if (initialized != MH_OK && initialized != MH_ERROR_ALREADY_INITIALIZED) return false;
    // This trampoline remains process-lived even after disable. Pin before installing it.
    HMODULE pinned = nullptr;
    if (!GetModuleHandleExW(GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_PIN,
        reinterpret_cast<LPCWSTR>(&add_passes), &pinned)) return false;
    return MH_CreateHook(reinterpret_cast<void*>(target), reinterpret_cast<void*>(&add_passes),
        reinterpret_cast<void**>(&original)) == MH_OK;
}

constexpr ObserverPlatform platform{
    verify_executable,
    []() noexcept { return reinterpret_cast<uintptr_t>(GetModuleHandleW(nullptr)); },
    copy,
    []() noexcept -> uint64_t { return GetTickCount64(); },
    create_hook,
    [](uintptr_t target) noexcept { return MH_EnableHook(reinterpret_cast<void*>(target)) == MH_OK; },
    [](uintptr_t target) noexcept {
        const auto result = MH_DisableHook(reinterpret_cast<void*>(target));
        return result == MH_OK || result == MH_ERROR_DISABLED;
    },
    create_execution_hook,
    report_execution,
    [](rsf_game_log_fn log, void* user, const char* message) noexcept {
        try { if (log) log(user, message); } catch (...) {}
    },
};
static_assert(sizeof(ObserverPlatform) == 10 * sizeof(void*));
}

const ObserverPlatform& observer_platform() noexcept { return platform; }
}
