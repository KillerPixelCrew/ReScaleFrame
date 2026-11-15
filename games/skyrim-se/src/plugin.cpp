// SPDX-License-Identifier: GPL-3.0-only
#include <rescaleframe/game_api.h>
#include <rescaleframe/game_plugin_util.h>
#include <rescaleframe/sha256_file.h>
#include <rescaleframe/version.h>
#include <MinHook.h>
#include <atomic>
#include <cstdio>
#include <cstring>
#include <cwchar>
#include <mutex>

namespace {
constexpr char fingerprint[] =
    "846efccf0c1374d71f892907f46549560f2fcb0a75cb87a3eed438baa0f1402f";
using UpdateJitter = void (*)(void*);
UpdateJitter original = nullptr;
void* target = nullptr;
std::mutex lifecycle;
std::atomic<bool> accepting{false};
std::atomic<uint32_t> calls{0};
std::atomic<uint64_t> observations{0};
rsf_game_host_services services{};
bool prepared = false;
bool enabled = false;
const char* reason = "Native jitter observer not prepared; SR, FG and latency are unimplemented.";

bool copy(void* output, const void* input, size_t size) noexcept
{
    if (!input) return false;
#if defined(_MSC_VER)
    __try { std::memcpy(output, input, size); return true; }
    __except (EXCEPTION_EXECUTE_HANDLER) { return false; }
#else
    MEMORY_BASIC_INFORMATION memory{};
    if (!VirtualQuery(input, &memory, sizeof(memory)) || memory.State != MEM_COMMIT ||
        (memory.Protect & (PAGE_NOACCESS | PAGE_GUARD)) ||
        uintptr_t(input) + size > uintptr_t(memory.BaseAddress) + memory.RegionSize) return false;
    std::memcpy(output, input, size);
    return true;
#endif
}

void observe_jitter(void* state) noexcept
{
    calls.fetch_add(1, std::memory_order_acq_rel);
    // The trampoline is process-lived. Never hold lifecycle across native execution.
    original(state);
    if (accepting.load(std::memory_order_acquire)) {
        const auto number = observations.fetch_add(1, std::memory_order_relaxed);
        if (number < 16) {
            struct Dimensions { uint32_t width, height; } dimensions{};
            struct Projection { float x, y; } projection{};
            const auto* bytes = static_cast<const unsigned char*>(state);
            const bool readable = state && copy(&dimensions, bytes + 0x24, sizeof(dimensions)) &&
                copy(&projection, bytes + 0x44, sizeof(projection));
            rsf_game_host_services host{};
            {
                std::lock_guard<std::mutex> lock(lifecycle);
                if (accepting.load(std::memory_order_relaxed)) host = services;
            }
            if (host.log) {
                char message[256]{};
                std::snprintf(message, sizeof(message),
                    "Skyrim native jitter observation=%llu state=%p readable=%u size=%ux%u projection=%g,%g; no GPU processing",
                    static_cast<unsigned long long>(number), state, readable ? 1u : 0u,
                    dimensions.width, dimensions.height, double(projection.x), double(projection.y));
                try { host.log(host.user, message); } catch (...) {}
            }
        }
    }
    calls.fetch_sub(1, std::memory_order_acq_rel);
}

bool match_target()
{
    wchar_t path[32768]{};
    const auto length = GetModuleFileNameW(nullptr, path, 32768);
    if (!length || length >= 32768) return false;
    const auto* leaf = std::wcsrchr(path, L'\\');
    if (_wcsicmp(leaf ? leaf + 1 : path, L"SkyrimSE.exe")) return false;
    std::string hash;
    if (!rsf::sha256_file(path, hash) || hash != fingerprint) return false;
    constexpr unsigned char expected[] = {
        0x48,0x8b,0x05,0x39,0x5d,0x3b,0x02,0x0f,0x57,0xc0,
        0x48,0x8b,0x90,0xf0,0x01,0x00,0x00,0x80,0x7a,0x18,0x00};
    target = reinterpret_cast<unsigned char*>(GetModuleHandleW(nullptr)) + 0x101e360;
    unsigned char actual[sizeof(expected)]{};
    return copy(actual, target, sizeof(actual)) && !std::memcmp(actual, expected, sizeof(actual));
}

rsf_result prepare(const rsf_game_prepare_args* args) noexcept try
{
    auto valid = rsf_game::validate(args); if (valid != RSF_OK) return valid;
    valid = rsf_game::validate(args->host); if (valid != RSF_OK) return valid;
    if (!args->host->session_id) return RSF_ERROR_INVALID_ARGUMENT;
    std::lock_guard<std::mutex> lock(lifecycle);
    if (prepared || calls.load(std::memory_order_acquire)) return RSF_ERROR_BUSY;
    if (!match_target()) {
        reason = "Skyrim native observer refused: executable identity or jitter entry bytes differ.";
        return RSF_ERROR_NATIVE_REFUSED;
    }
    if (GetModuleHandleW(L"CommunityShaders.dll")) {
        reason = "Community Shaders ownership handover is not implemented; activation refused.";
        return RSF_ERROR_NATIVE_REFUSED;
    }
    if (!original) {
        const auto initialized = MH_Initialize();
        if (initialized != MH_OK && initialized != MH_ERROR_ALREADY_INITIALIZED) return RSF_ERROR_NATIVE_REFUSED;
        if (MH_CreateHook(target, reinterpret_cast<void*>(&observe_jitter), reinterpret_cast<void**>(&original)) != MH_OK)
            return RSF_ERROR_NATIVE_REFUSED;
        HMODULE pinned = nullptr;
        if (!GetModuleHandleExW(GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_PIN,
                              reinterpret_cast<LPCWSTR>(&observe_jitter), &pinned)) {
            MH_RemoveHook(target); original = nullptr;
            return RSF_ERROR_NATIVE_REFUSED;
        }
    }
    services = *args->host;
    observations.store(0, std::memory_order_relaxed);
    prepared = true;
    reason = "Native jitter observer prepared; no graphics processing or latency policy.";
    return RSF_OK;
}
catch (...) { return RSF_ERROR_NOT_READY; }

rsf_result start(const rsf_game_start_args* args) noexcept try
{
    const auto valid = rsf_game::validate(args); if (valid != RSF_OK) return valid;
    std::lock_guard<std::mutex> lock(lifecycle);
    if (!prepared) return RSF_ERROR_NOT_READY;
    if (enabled) return RSF_OK;
    if (GetModuleHandleW(L"CommunityShaders.dll")) return RSF_ERROR_NATIVE_REFUSED;
    if (MH_EnableHook(target) != MH_OK) return RSF_ERROR_NATIVE_REFUSED;
    enabled = true;
    accepting.store(true, std::memory_order_release);
    reason = "Pass-through native jitter observation active; SR, FG and latency remain unimplemented.";
    return RSF_OK;
}
catch (...) { return RSF_ERROR_NOT_READY; }

rsf_result quiesce(const rsf_game_control_args* args) noexcept
{
    const auto valid = rsf_game::validate(args); if (valid != RSF_OK) return valid;
    accepting.store(false, std::memory_order_release);
    return calls.load(std::memory_order_acquire) ? RSF_ERROR_BUSY : RSF_OK;
}

rsf_result stop(const rsf_game_control_args* args) noexcept try
{
    const auto valid = rsf_game::validate(args); if (valid != RSF_OK) return valid;
    std::lock_guard<std::mutex> lock(lifecycle);
    accepting.store(false, std::memory_order_release);
    if (enabled) {
        if (MH_DisableHook(target) != MH_OK) return RSF_ERROR_BUSY;
        enabled = false;
    }
    if (calls.load(std::memory_order_acquire)) return RSF_ERROR_BUSY;
    services = {}; prepared = false;
    reason = "Native observer stopped; no host services retained.";
    return RSF_OK;
}
catch (...) { return RSF_ERROR_NOT_READY; }

rsf_result status(rsf_game_renderer_status* output) noexcept try
{
    const auto valid = rsf_game::validate(output); if (valid != RSF_OK) return valid;
    std::lock_guard<std::mutex> lock(lifecycle);
    output->prepared = prepared ? 1u : 0u;
    output->active = accepting.load(std::memory_order_acquire) ? 1u : 0u;
    output->rendering_ready = 0;
    output->reason = reason;
    return RSF_OK;
}
catch (...) { return RSF_ERROR_NOT_READY; }

rsf_detection detect(const rsf_game_probe* probe) noexcept
{
    return probe && probe->struct_size >= sizeof(*probe) && probe->pe_machine == 0x8664 &&
        rsf_game::equal_ascii(probe->executable_name_utf8, "SkyrimSE.exe") &&
        rsf_game::equal_ascii(probe->sha256_hex, fingerprint) ? RSF_GAME_RECOGNIZED : RSF_GAME_UNKNOWN;
}
}

extern "C" __declspec(dllexport) rsf_result rsf_get_game_plugin_api(
    uint32_t requested_abi, rsf_game_plugin_api* api) noexcept
{
    if (!api || api->struct_size < sizeof(*api)) return RSF_ERROR_INVALID_ARGUMENT;
    if (requested_abi != RSF_GAME_ABI_VERSION) return RSF_ERROR_ABI_MISMATCH;
    *api = {sizeof(*api), RSF_GAME_ABI_VERSION,
        {"skyrim-se", "Skyrim Special Edition", RSF_VERSION_STRING, 0,
         "Native jitter observation only; SR, FG and latency are unimplemented."},
        detect, {prepare, start, quiesce, stop, status}};
    return RSF_OK;
}
