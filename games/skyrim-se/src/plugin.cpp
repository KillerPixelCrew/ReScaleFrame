// SPDX-License-Identifier: GPL-3.0-only
#include <rescaleframe/game_api.h>
#include <rescaleframe/game_plugin_util.h>
#include <rescaleframe/sha256_file.h>
#include <rescaleframe/version.h>
#include <MinHook.h>
#include <d3d11.h>
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
using ProcessMain = uintptr_t (*)(void*, uint32_t, uint32_t, uintptr_t);
ProcessMain original_process = nullptr;
void* process_target = nullptr;
std::mutex lifecycle;
std::atomic<bool> accepting{false};
std::atomic<uint32_t> calls{0};
std::atomic<uint64_t> observations{0};
std::atomic<uint64_t> pass_observations{0};
rsf_game_host_services services{};
bool prepared = false;
bool enabled = false;
const char* reason = "Native observers not prepared; SR, FG and latency are unimplemented.";

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

void describe_target(uint32_t index, char* message, size_t capacity) noexcept
{
    // Native CopyResource at RVA 0x10155FE uses renderer + index*0x30 + 0xA58.
    // Read at the consuming pass only. No native pointer survives this callback.
    ID3D11Texture2D* texture = nullptr;
    const auto* renderer = reinterpret_cast<const unsigned char*>(GetModuleHandleW(nullptr)) + 0x3331f40;
    D3D11_TEXTURE2D_DESC desc{};
    // Only the researched main-colour and motion-candidate slots are probed.
    if ((index != 1 && index != 7) ||
        !copy(&texture, renderer + 0xa58 + size_t(index) * 0x30, sizeof(texture)) || !texture) {
        std::snprintf(message, capacity, "target=%u unavailable", index);
        return;
    }
    texture->GetDesc(&desc);
    std::snprintf(message, capacity, "target=%u texture=%p size=%ux%u format=%u samples=%u bind=0x%x",
        index, static_cast<void*>(texture), desc.Width, desc.Height, unsigned(desc.Format),
        desc.SampleDesc.Count, desc.BindFlags);
}

void describe_native_inputs(char* message, size_t capacity) noexcept
{
    const auto* base = reinterpret_cast<const unsigned char*>(GetModuleHandleW(nullptr));
    const auto* state = base + 0x33363a0;
    uintptr_t camera = 0, cache = 0;
    uint32_t count = 0;
    // Native 0x101CB50: state + 0xB8 pointer, +0xC8 count, stride 0x290.
    bool readable = copy(&camera, base + 0x33d3d28, sizeof(camera)) && camera &&
        copy(&cache, state + 0xb8, sizeof(cache)) && cache &&
        copy(&count, state + 0xc8, sizeof(count)) && count && count <= 256;
    uintptr_t selected = 0;
    if (readable) {
        for (uint32_t i = 0; i < count; ++i) {
            uintptr_t owner = 0;
            unsigned char jittered = 0;
            const auto entry = cache + size_t(i) * 0x290;
            if (!copy(&owner, reinterpret_cast<const void*>(entry), sizeof(owner)) ||
                !copy(&jittered, reinterpret_cast<const void*>(entry + 0x284), sizeof(jittered))) {
                readable = false; break;
            }
            if (owner == camera && jittered == 1) { selected = entry; break; }
        }
    }
    float projection[16]{}, current_vp[16]{}, previous_vp[16]{};
    float planes[2]{}, origins[6]{}, jitter[2]{}, scale[2]{};
    readable = readable && selected &&
        copy(projection, reinterpret_cast<const void*>(selected + 0x1c0), sizeof(projection)) &&
        copy(current_vp, reinterpret_cast<const void*>(selected + 0x140), sizeof(current_vp)) &&
        copy(previous_vp, reinterpret_cast<const void*>(selected + 0x180), sizeof(previous_vp)) &&
        copy(origins, reinterpret_cast<const void*>(selected + 0x26c), sizeof(origins)) &&
        copy(planes, reinterpret_cast<const void*>(camera + 0x160), sizeof(planes)) &&
        copy(jitter, state + 0x44, sizeof(jitter)) && copy(scale, state + 0x114, sizeof(scale));
    ID3D11Texture2D* depth = nullptr;
    D3D11_TEXTURE2D_DESC desc{};
    // Native CopyResource at 0x15399DF reads main depth at renderer + 0x2018.
    const bool have_depth = copy(&depth, base + 0x3333f58, sizeof(depth)) && depth;
    if (have_depth) depth->GetDesc(&desc);
    std::snprintf(message, capacity,
        "Skyrim native inputs camera=%p cache=%p entries=%u readable=%u near=%g far=%g projection=%g,%g,%g,%g,%g jitter=%g,%g scale=%g,%g origin=%g,%g,%g previous_origin=%g,%g,%g VP=%g,%g previous_VP=%g,%g depth=%p size=%ux%u format=%u samples=%u; no input-frame identity or pixel validation",
        reinterpret_cast<void*>(camera), reinterpret_cast<void*>(selected), count, readable ? 1u : 0u,
        double(planes[0]), double(planes[1]), double(projection[0]), double(projection[5]),
        double(projection[10]), double(projection[11]), double(projection[14]), double(jitter[0]), double(jitter[1]),
        double(scale[0]), double(scale[1]), double(origins[0]), double(origins[1]), double(origins[2]),
        double(origins[3]), double(origins[4]), double(origins[5]), double(current_vp[0]), double(current_vp[5]),
        double(previous_vp[0]), double(previous_vp[5]), static_cast<void*>(depth), desc.Width, desc.Height,
        unsigned(desc.Format), desc.SampleDesc.Count);
}

uintptr_t observe_process(void* manager, uint32_t input, uint32_t output, uintptr_t argument4) noexcept
{
    calls.fetch_add(1, std::memory_order_acq_rel);
    if (accepting.load(std::memory_order_acquire)) {
        const auto number = pass_observations.fetch_add(1, std::memory_order_relaxed);
        if (number < 16) {
            char colour[160]{}, motion[160]{}, message[512]{};
            describe_target(input, colour, sizeof(colour));
            describe_target(7, motion, sizeof(motion));
            std::snprintf(message, sizeof(message),
                "Skyrim native post-process observation=%llu manager=%p input=%u output=%u; %s; motion candidate %s; no GPU processing",
                static_cast<unsigned long long>(number), manager, input, output, colour, motion);
            rsf_game_host_services host{};
            {
                std::lock_guard<std::mutex> lock(lifecycle);
                if (accepting.load(std::memory_order_relaxed)) host = services;
            }
            if (host.log) { try { host.log(host.user, message); } catch (...) {} }
            if (host.log && number < 4) {
                char inputs[1024]{};
                describe_native_inputs(inputs, sizeof(inputs));
                try { host.log(host.user, inputs); } catch (...) {}
            }
        }
    }
    const auto result = original_process(manager, input, output, argument4);
    calls.fetch_sub(1, std::memory_order_acq_rel);
    return result;
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
    if (!copy(actual, target, sizeof(actual)) || std::memcmp(actual, expected, sizeof(actual))) return false;
    constexpr unsigned char process_expected[] = {
        0x48,0x89,0x5c,0x24,0x08,0x48,0x89,0x6c,0x24,0x10,
        0x48,0x89,0x74,0x24,0x20,0x44,0x89,0x44,0x24,0x18};
    process_target = reinterpret_cast<unsigned char*>(GetModuleHandleW(nullptr)) + 0x14edbf0;
    unsigned char process_actual[sizeof(process_expected)]{};
    constexpr unsigned char call_expected[] = {0xe8,0x04,0x2e,0xfb,0xff};
    unsigned char call_actual[sizeof(call_expected)]{};
    const auto* site = reinterpret_cast<unsigned char*>(GetModuleHandleW(nullptr)) + 0x153ade7;
    return copy(process_actual, process_target, sizeof(process_actual)) &&
        !std::memcmp(process_actual, process_expected, sizeof(process_actual)) &&
        copy(call_actual, site, sizeof(call_actual)) &&
        !std::memcmp(call_actual, call_expected, sizeof(call_actual));
}

rsf_result prepare(const rsf_game_prepare_args* args) noexcept try
{
    auto valid = rsf_game::validate(args); if (valid != RSF_OK) return valid;
    valid = rsf_game::validate(args->host); if (valid != RSF_OK) return valid;
    if (!args->host->session_id) return RSF_ERROR_INVALID_ARGUMENT;
    std::lock_guard<std::mutex> lock(lifecycle);
    if (prepared || calls.load(std::memory_order_acquire)) return RSF_ERROR_BUSY;
    if (!match_target()) {
        reason = "Skyrim native observer refused: executable identity or native hook bytes differ.";
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
        if (MH_CreateHook(process_target, reinterpret_cast<void*>(&observe_process), reinterpret_cast<void**>(&original_process)) != MH_OK) {
            MH_RemoveHook(target); original = nullptr;
            return RSF_ERROR_NATIVE_REFUSED;
        }
        HMODULE pinned = nullptr;
        if (!GetModuleHandleExW(GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_PIN,
                              reinterpret_cast<LPCWSTR>(&observe_jitter), &pinned)) {
            MH_RemoveHook(process_target); original_process = nullptr;
            MH_RemoveHook(target); original = nullptr;
            return RSF_ERROR_NATIVE_REFUSED;
        }
    }
    services = *args->host;
    observations.store(0, std::memory_order_relaxed);
    pass_observations.store(0, std::memory_order_relaxed);
    prepared = true;
    reason = "Native jitter and pre-HDR observers prepared; no graphics processing or latency policy.";
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
    if (MH_EnableHook(process_target) != MH_OK) {
        MH_DisableHook(target);
        return RSF_ERROR_NATIVE_REFUSED;
    }
    enabled = true;
    accepting.store(true, std::memory_order_release);
    reason = "Pass-through native jitter and pre-HDR observations active; SR, FG and latency remain unimplemented.";
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
        const auto jitter_result = MH_DisableHook(target);
        const auto process_result = MH_DisableHook(process_target);
        if ((jitter_result != MH_OK && jitter_result != MH_ERROR_DISABLED) ||
            (process_result != MH_OK && process_result != MH_ERROR_DISABLED)) return RSF_ERROR_BUSY;
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
         "Native jitter and pre-HDR observation only; SR, FG and latency are unimplemented."},
        detect, {prepare, start, quiesce, stop, status}};
    return RSF_OK;
}
