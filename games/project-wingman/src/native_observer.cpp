// SPDX-License-Identifier: GPL-3.0-only
#include "native_observer.h"
#include <rescaleframe/game_plugin_util.h>
#include <rescaleframe/log.h>
#include <rescaleframe/sha256_file.h>
#include <MinHook.h>
#include <array>
#include <atomic>
#include <cstring>
#include <cwchar>
#include <mutex>

namespace rsf::wingman {
namespace {
struct Rect { int32_t min_x, min_y, max_x, max_y; };
struct Inputs {
    uint32_t allow_downsample;
    uint32_t downsample_format;
    void* colour;
    void* depth;
    void* velocity;
};
static_assert(sizeof(Inputs) == 32 && offsetof(Inputs, colour) == 8);
using AddPasses = void (*)(const void*, void*, const void*, const Inputs*,
                          void**, Rect*, void**, Rect*);
AddPasses original = nullptr;
void* target = nullptr;
std::mutex guard;
std::atomic<bool> accepting{false};
std::atomic<uint32_t> calls{0};
rsf_game_host_services services{};
bool prepared = false, installed = false;
const char* reason = "Native temporal pass observer has not been prepared.";
struct Sample { uintptr_t owner = 0; ULONGLONG last = 0; Rect input{}, output{}; };
std::array<Sample, 16> samples{};
uint64_t serial = 0;

bool copy(void* output, const void* input, size_t bytes) noexcept
{
    if (!input) return false;
#if defined(_MSC_VER)
    __try { std::memcpy(output, input, bytes); return true; }
    __except (EXCEPTION_EXECUTE_HANDLER) { return false; }
#else
    MEMORY_BASIC_INFORMATION memory{};
    if (!VirtualQuery(input, &memory, sizeof(memory)) || memory.State != MEM_COMMIT ||
        (memory.Protect & (PAGE_NOACCESS | PAGE_GUARD)) ||
        uintptr_t(input) + bytes > uintptr_t(memory.BaseAddress) + memory.RegionSize) return false;
    std::memcpy(output, input, bytes); return true;
#endif
}
template<class T> bool read(const void* base, size_t offset, T& value) noexcept
{ return base && copy(&value, static_cast<const unsigned char*>(base) + offset, sizeof(value)); }

void observe(void* graph, const void* view, const Inputs* input, void** output,
             Rect* output_rect, void** half, Rect* half_rect) noexcept try
{
    Inputs textures{};
    Rect render{}, result{}, half_result{};
    uintptr_t history = 0;
    float jitter[2]{};
    uint8_t camera_cut = 0;
    void* colour_output = nullptr;
    void* half_output = nullptr;
    // Only copy graph-construction facts. RDG references are opaque observations, never GPU leases.
    const bool readable = copy(&textures, input, sizeof(textures)) &&
        read(view, 0x1670, render) && read(view, 0x1680, history) &&
        read(view, 0x4fa4, jitter) && read(view, 0xd90, camera_cut) &&
        copy(&colour_output, output, sizeof(colour_output)) &&
        copy(&result, output_rect, sizeof(result)) && copy(&half_output, half, sizeof(half_output));
    if (half_output) copy(&half_result, half_rect, sizeof(half_result));
    rsf_game_host_services host{};
    uint64_t observation = 0;
    {
        std::lock_guard<std::mutex> lock(guard);
        if (!accepting.load(std::memory_order_relaxed)) return;
        const uintptr_t key = history ? history : uintptr_t(view);
        auto* selected = &samples[0];
        for (auto& sample : samples) {
            if (sample.owner == key) { selected = &sample; break; }
            if (sample.last < selected->last) selected = &sample;
        }
        const auto now = GetTickCount64();
        const bool changed = selected->owner != key ||
            std::memcmp(&selected->input, &render, sizeof(render)) ||
            std::memcmp(&selected->output, &result, sizeof(result));
        if (!changed && !camera_cut && now - selected->last < 1000) return;
        *selected = {key, now, render, result};
        host = services; observation = ++serial;
    }
    rsf::say(host.log, host.user,
        "PW temporal serial=%llu graph=%p view=%p history=%p readable=%u cut=%u "
        "render=(%d,%d,%d,%d) output=(%d,%d,%d,%d) jitter=(%.4f,%.4f) "
        "rdg=(%p,%p,%p)->%p half=%p half_rect=(%d,%d,%d,%d) downsample=%u format=%u",
        static_cast<unsigned long long>(observation), graph, view,
        reinterpret_cast<void*>(history), readable ? 1u : 0u, unsigned(camera_cut),
        render.min_x, render.min_y, render.max_x, render.max_y,
        result.min_x, result.min_y, result.max_x, result.max_y,
        double(jitter[0]), double(jitter[1]), textures.colour, textures.depth, textures.velocity,
        colour_output, half_output, half_result.min_x, half_result.min_y,
        half_result.max_x, half_result.max_y, textures.allow_downsample & 0xffu, textures.downsample_format);
}
catch (...) { /* Diagnostics never interrupt the engine graph. */ }

void add_passes(const void* self, void* graph, const void* view, const Inputs* input,
                void** output, Rect* output_rect, void** half, Rect* half_rect)
{
    calls.fetch_add(1, std::memory_order_acq_rel);
    original(self, graph, view, input, output, output_rect, half, half_rect);
    if (accepting.load(std::memory_order_acquire))
        observe(graph, view, input, output, output_rect, half, half_rect);
    calls.fetch_sub(1, std::memory_order_release);
}

bool match_target() try
{
    wchar_t executable[32768]{};
    const auto length = GetModuleFileNameW(nullptr, executable, 32768);
    if (!length || length >= 32768) return false;
    const auto* leaf = wcsrchr(executable, L'\\');
    if (_wcsicmp(leaf ? leaf + 1 : executable, L"ProjectWingman-Win64-Shipping.exe")) return false;
    std::string digest;
    if (!rsf::sha256_file(executable, digest) || digest != known_sha256) return false;
    const auto* base = reinterpret_cast<const unsigned char*>(GetModuleHandleW(nullptr));
    constexpr unsigned char expected[] = {
        0x48,0x89,0x5c,0x24,0x08,0x48,0x89,0x74,0x24,0x10,0x57,0x48,0x83,0xec,0x40,
        0x48,0x8b,0x05,0xda,0x50,0x8d,0x03,0x49,0x8b,0xf9,0x49,0x8b,0xd8,0x48,0x8b,0xf2,0x83};
    unsigned char observed[sizeof(expected)]{};
    target = const_cast<unsigned char*>(base + 0x5c442b0);
    return copy(observed, target, sizeof(observed)) && !std::memcmp(observed, expected, sizeof(expected));
}
catch (...) { return false; }
}

rsf_result prepare(const rsf_game_prepare_args* args) noexcept try
{
    auto valid = rsf_game::validate(args); if (valid != RSF_OK) return valid;
    valid = rsf_game::validate(args->host); if (valid != RSF_OK) return valid;
    if (!args->host->session_id) return RSF_ERROR_INVALID_ARGUMENT;
    std::lock_guard<std::mutex> lock(guard);
    if (prepared || calls.load(std::memory_order_acquire)) return RSF_ERROR_BUSY;
    if (!match_target()) {
        reason = "Native temporal observer refused: executable identity or entry bytes differ.";
        return RSF_ERROR_NATIVE_REFUSED;
    }
    if (!original) {
        const auto initialized = MH_Initialize();
        if (initialized != MH_OK && initialized != MH_ERROR_ALREADY_INITIALIZED) return RSF_ERROR_NATIVE_REFUSED;
        if (MH_CreateHook(target, reinterpret_cast<void*>(&add_passes), reinterpret_cast<void**>(&original)) != MH_OK)
            return RSF_ERROR_NATIVE_REFUSED;
        // Keep the detour code and trampoline valid for late native entries after disable.
        // One fixed hook is retained for process lifetime; stop retires the borrowed host services.
        HMODULE pinned = nullptr;
        if (!GetModuleHandleExW(GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_PIN,
            reinterpret_cast<LPCWSTR>(&add_passes), &pinned)) {
            MH_RemoveHook(target); original = nullptr; return RSF_ERROR_NATIVE_REFUSED;
        }
    }
    services = *args->host; samples = {}; serial = 0; prepared = true;
    reason = "Native temporal observer prepared; no graphics processing implemented.";
    return RSF_OK;
}
catch (...) { return RSF_ERROR_NOT_READY; }

rsf_result start(const rsf_game_start_args* args) noexcept try
{
    const auto valid = rsf_game::validate(args); if (valid != RSF_OK) return valid;
    std::lock_guard<std::mutex> lock(guard);
    if (!prepared) return RSF_ERROR_NOT_READY;
    if (!installed && MH_EnableHook(target) != MH_OK) return RSF_ERROR_NATIVE_REFUSED;
    installed = true; accepting.store(true, std::memory_order_release);
    reason = "Native temporal observer active; GPU leases, SR, FG and latency remain unimplemented.";
    return RSF_OK;
}
catch (...) { return RSF_ERROR_NOT_READY; }

rsf_result quiesce(const rsf_game_control_args* args) noexcept
{
    const auto valid = rsf_game::validate(args); if (valid != RSF_OK) return valid;
    accepting.store(false, std::memory_order_release);
    return RSF_OK;
}
rsf_result stop(const rsf_game_control_args* args) noexcept try
{
    const auto valid = rsf_game::validate(args); if (valid != RSF_OK) return valid;
    std::lock_guard<std::mutex> lock(guard);
    accepting.store(false, std::memory_order_release);
    if (installed) {
        if (MH_DisableHook(target) != MH_OK) return RSF_ERROR_BUSY;
        installed = false;
    }
    if (calls.load(std::memory_order_acquire)) return RSF_ERROR_BUSY;
    services = {}; prepared = false;
    reason = "Native temporal observer stopped; no host services retained.";
    return RSF_OK;
}
catch (...) { return RSF_ERROR_NOT_READY; }

rsf_result status(rsf_game_renderer_status* output) noexcept try
{
    const auto valid = rsf_game::validate(output); if (valid != RSF_OK) return valid;
    std::lock_guard<std::mutex> lock(guard);
    output->prepared = prepared ? 1u : 0u;
    output->active = accepting.load(std::memory_order_acquire) ? 1u : 0u;
    output->rendering_ready = 0;
    output->reason = reason;
    return RSF_OK;
}
catch (...) { return RSF_ERROR_NOT_READY; }
}
