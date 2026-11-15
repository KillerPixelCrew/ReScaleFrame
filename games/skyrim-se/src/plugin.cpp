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
#include "cpu_events.h"
#include "render_inputs.h"
#include "resolution.h"
#include "auxiliary.h"
#include <rescaleframe/d3d11_host.h>
#include <string>
#if defined(_MSC_VER)
#include <intrin.h>
#endif

namespace {
constexpr char fingerprint[] =
    "846efccf0c1374d71f892907f46549560f2fcb0a75cb87a3eed438baa0f1402f";
using UpdateJitter = void (*)(void*);
UpdateJitter original = nullptr;
void* target = nullptr;
using ProcessMain = uintptr_t (*)(void*, uint32_t, uint32_t, uintptr_t);
ProcessMain original_process = nullptr;
void* process_target = nullptr;
UpdateJitter original_resolution = nullptr;
void* resolution_target = nullptr;
using RendererEnd = uintptr_t (*)();
RendererEnd original_end = nullptr;
void* end_target = nullptr;
using UploadFrame = void (*)(void*, uint8_t);
UploadFrame original_upload = nullptr;
void* upload_target = nullptr;
std::mutex lifecycle;
std::atomic<bool> accepting{false};
std::atomic<uint32_t> calls{0};
std::atomic<uint64_t> observations{0};
std::atomic<uint64_t> pass_observations{0};
rsf_game_host_services services{};
rsf_game_host_services bound_services{}, bound_upstream{};
using SetHostActive = int32_t (*)(void*, uint32_t);
SetHostActive host_active = nullptr;
using BindPresentation = int32_t (*)(void*, const rsf_d3d11_presentation_binding*);
BindPresentation bind_presentation = nullptr;
bool prepared = false;
bool enabled = false;
const char* reason = "Native hooks not prepared; native-resolution SR is experimental; FG and latency remain incomplete.";

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
#if defined(_MSC_VER)
    const auto caller = uintptr_t(_ReturnAddress());
#else
    const auto caller = uintptr_t(__builtin_return_address(0));
#endif
    calls.fetch_add(1, std::memory_order_acq_rel);
    bool reconstructed = false;
    if (accepting.load(std::memory_order_acquire) && input == 1 && output == 0 &&
        caller == uintptr_t(GetModuleHandleW(nullptr)) + 0x153adec) {
        rsf_game_host_services host{};
        { std::lock_guard<std::mutex> lock(lifecycle); if (accepting.load(std::memory_order_relaxed)) host = services; }
        try { reconstructed = rsf::skyrim::reconstruct(manager, host); } catch (...) {}
    }
    if (accepting.load(std::memory_order_acquire)) {
        const auto number = pass_observations.fetch_add(1, std::memory_order_relaxed);
        if (number < 16) {
            char colour[160]{}, motion[160]{}, message[512]{};
            describe_target(input, colour, sizeof(colour));
            describe_target(7, motion, sizeof(motion));
            std::snprintf(message, sizeof(message),
                "Skyrim native post-process observation=%llu source=%llu thread=%lu manager=%p input=%u output=%u; %s; motion candidate %s",
                static_cast<unsigned long long>(number), static_cast<unsigned long long>(rsf::skyrim::cpu_source()),
                GetCurrentThreadId(), manager, input, output, colour, motion);
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
    // Retain native bloom, exposure and HDR. Only its temporal evaluation is redundant
    // after a complete replacement. This byte is consumed on this native render owner.
    uintptr_t taa = 0; unsigned char temporal_enabled = 0; uint32_t clear_mask = 0;
    const bool bypass = reconstructed && copy(&taa, static_cast<unsigned char*>(manager) + 0x1f0, sizeof(taa)) && taa &&
        copy(&temporal_enabled, reinterpret_cast<void*>(taa + 0x18), sizeof(temporal_enabled)) &&
        copy(&clear_mask, reinterpret_cast<void*>(taa + 0x30), sizeof(clear_mask));
    if (bypass) {
        // Native 0x1544DF4 consumes this mask before evaluating TAA. Bit 0 clears
        // main history targets 81/82, preventing stale history on an SR fallback.
        *reinterpret_cast<uint32_t*>(taa + 0x30) = clear_mask | 1u;
        *reinterpret_cast<unsigned char*>(taa + 0x18) = 0;
    }
    const auto result = original_process(manager, input, output, argument4);
    if (bypass) *reinterpret_cast<unsigned char*>(taa + 0x18) = temporal_enabled;
    calls.fetch_sub(1, std::memory_order_acq_rel);
    return result;
}

void observe_jitter(void* state) noexcept
{
    calls.fetch_add(1, std::memory_order_acq_rel);
    // The trampoline is process-lived. Never hold lifecycle across native execution.
    original(state);
    rsf_game_host_services resolution_host{};
    { std::lock_guard<std::mutex> lock(lifecycle); if (accepting.load(std::memory_order_relaxed)) resolution_host = services; }
    try { rsf::skyrim::configure_resolution(state, resolution_host); } catch (...) {}
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
                    "Skyrim native jitter observation=%llu state=%p readable=%u size=%ux%u projection=%g,%g",
                    static_cast<unsigned long long>(number), state, readable ? 1u : 0u,
                    dimensions.width, dimensions.height, double(projection.x), double(projection.y));
                try { host.log(host.user, message); } catch (...) {}
            }
        }
    }
    calls.fetch_sub(1, std::memory_order_acq_rel);
}

void observe_resolution(void* state) noexcept {
    calls.fetch_add(1, std::memory_order_acq_rel);
    if (uintptr_t(state) != uintptr_t(GetModuleHandleW(nullptr)) + 0x33363a0 || !rsf::skyrim::resolution_owned()) original_resolution(state);
    calls.fetch_sub(1, std::memory_order_acq_rel);
}

uintptr_t observe_end() noexcept {
    calls.fetch_add(1, std::memory_order_acq_rel);
    if (accepting.load(std::memory_order_acquire) && bind_presentation) {
        uintptr_t window{}; void* chain{};
        const auto base = reinterpret_cast<const unsigned char*>(GetModuleHandleW(nullptr));
        if (copy(&window, base + 0x3330198, sizeof(window)) && window &&
            copy(&chain, reinterpret_cast<const void*>(window + 0x18), sizeof(chain)) && chain) {
            const rsf_d3d11_presentation_binding binding{sizeof(binding), RSF_D3D11_HOST_ABI_VERSION, chain};
            try { (void)bind_presentation(services.user, &binding); } catch (...) {}
        }
    }
    const auto result = original_end();
    rsf::skyrim::finish_auxiliaries(services);
    calls.fetch_sub(1, std::memory_order_acq_rel);
    return result;
}

void observe_upload(void* renderer, uint8_t flags) noexcept {
    calls.fetch_add(1, std::memory_order_acq_rel);
    const auto base = uintptr_t(GetModuleHandleW(nullptr));
    const bool uploading = uintptr_t(renderer) == base + 0x3331f40 && *reinterpret_cast<const unsigned char*>(base + 0x3336400) != 0;
    original_upload(renderer, flags);
    if (uploading) rsf::skyrim::record_per_frame_flags(flags);
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
    constexpr unsigned char reset_expected[] = {0x44,0x39,0x71,0x30,0x74,0x09,0xe8,0x61,0x05,0x00,0x00,0x44,0x89,0x73,0x30};
    unsigned char reset_actual[sizeof(reset_expected)]{};
    const auto* reset_site = reinterpret_cast<unsigned char*>(GetModuleHandleW(nullptr)) + 0x1544df4;
    constexpr unsigned char resolution_expected[]{0x83,0xb9,0x28,0x01,0x00,0x00,0x00,0x48,0x8b,0xd1,0x0f,0x85,0x7f,0x01,0x00,0x00};
    unsigned char resolution_actual[sizeof(resolution_expected)]{};
    resolution_target = reinterpret_cast<unsigned char*>(GetModuleHandleW(nullptr)) + 0x101e130;
    constexpr unsigned char end_expected[]{0x48,0x83,0xec,0x48,0xb9,0x01,0x00,0x00,0x00,0xe8,0x22,0xa9,0x58,0x00};
    unsigned char end_actual[sizeof(end_expected)]{};
    end_target = reinterpret_cast<unsigned char*>(GetModuleHandleW(nullptr)) + 0x1009f70;
    constexpr unsigned char viewport_expected[]{0x48,0x83,0xec,0x68,0x0f,0x29,0x74,0x24,0x50,0xf3,0x0f,0x10,0x35,0xb7,0x24,0xb5,0x00};
    unsigned char viewport_actual[sizeof(viewport_expected)]{};
    constexpr unsigned char upload_expected[]{0x48,0x89,0x5c,0x24,0x08,0x55,0x48,0x8d,0xac,0x24,0xe0,0xfd,0xff,0xff,0x48,0x81,0xec,0x20,0x03,0x00,0x00};
    unsigned char upload_actual[sizeof(upload_expected)]{};
    upload_target = reinterpret_cast<unsigned char*>(GetModuleHandleW(nullptr)) + 0x100ae80;
    return copy(process_actual, process_target, sizeof(process_actual)) &&
        !std::memcmp(process_actual, process_expected, sizeof(process_actual)) &&
        copy(call_actual, site, sizeof(call_actual)) &&
        !std::memcmp(call_actual, call_expected, sizeof(call_actual)) &&
        copy(reset_actual, reset_site, sizeof(reset_actual)) && !std::memcmp(reset_actual, reset_expected, sizeof(reset_actual)) &&
        copy(resolution_actual, resolution_target, sizeof(resolution_actual)) && !std::memcmp(resolution_actual, resolution_expected, sizeof(resolution_actual)) &&
        copy(end_actual, end_target, sizeof(end_actual)) && !std::memcmp(end_actual, end_expected, sizeof(end_actual)) &&
        copy(viewport_actual, reinterpret_cast<const unsigned char*>(GetModuleHandleW(nullptr)) + 0x1009960, sizeof(viewport_actual)) &&
        !std::memcmp(viewport_actual, viewport_expected, sizeof(viewport_actual)) &&
        copy(upload_actual, upload_target, sizeof(upload_actual)) && !std::memcmp(upload_actual, upload_expected, sizeof(upload_actual));
}

rsf_result prepare(const rsf_game_prepare_args* args) noexcept try
{
    auto valid = rsf_game::validate(args); if (valid != RSF_OK) return valid;
    valid = rsf_game::validate(args->host); if (valid != RSF_OK) return valid;
    if (!args->host->session_id) return RSF_ERROR_INVALID_ARGUMENT;
    std::lock_guard<std::mutex> lock(lifecycle);
    if (prepared || calls.load(std::memory_order_acquire) || rsf::skyrim::cpu_busy()) return RSF_ERROR_BUSY;
    if (!match_target()) {
        reason = "Skyrim native observer refused: executable identity or native hook bytes differ.";
        return RSF_ERROR_NATIVE_REFUSED;
    }
    static bool dpi_requested = false;
    if (!dpi_requested) {
        dpi_requested = true;
        using SetDpi = BOOL (WINAPI*)(HANDLE);
        const auto set_dpi = reinterpret_cast<SetDpi>(GetProcAddress(GetModuleHandleW(L"user32.dll"), "SetProcessDpiAwarenessContext"));
        SetLastError(0);
        const bool accepted = set_dpi && set_dpi(reinterpret_cast<HANDLE>(INT_PTR(-4)));
        const auto error = accepted ? 0ul : static_cast<unsigned long>(GetLastError());
        if (args->host->log) {
            char message[180]{}; std::snprintf(message, sizeof(message),
                "Skyrim DPI startup per-monitor-v2 accepted=%u error=%lu; window creation must use physical pixels.",
                accepted ? 1u : 0u, error);
            args->host->log(args->host->user, message);
        }
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
        if (MH_CreateHook(resolution_target, reinterpret_cast<void*>(&observe_resolution), reinterpret_cast<void**>(&original_resolution)) != MH_OK) {
            MH_RemoveHook(process_target); original_process = nullptr;
            MH_RemoveHook(target); original = nullptr;
            return RSF_ERROR_NATIVE_REFUSED;
        }
        if (MH_CreateHook(end_target, reinterpret_cast<void*>(&observe_end), reinterpret_cast<void**>(&original_end)) != MH_OK) {
            MH_RemoveHook(resolution_target); original_resolution = nullptr;
            MH_RemoveHook(process_target); original_process = nullptr;
            MH_RemoveHook(target); original = nullptr;
            return RSF_ERROR_NATIVE_REFUSED;
        }
        if (MH_CreateHook(upload_target, reinterpret_cast<void*>(&observe_upload), reinterpret_cast<void**>(&original_upload)) != MH_OK) {
            MH_RemoveHook(end_target); original_end = nullptr;
            MH_RemoveHook(resolution_target); original_resolution = nullptr;
            MH_RemoveHook(process_target); original_process = nullptr;
            MH_RemoveHook(target); original = nullptr;
            return RSF_ERROR_NATIVE_REFUSED;
        }
        HMODULE pinned = nullptr;
        if (!GetModuleHandleExW(GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_PIN,
                              reinterpret_cast<LPCWSTR>(&observe_jitter), &pinned)) {
            MH_RemoveHook(upload_target); original_upload = nullptr;
            MH_RemoveHook(end_target); original_end = nullptr;
            MH_RemoveHook(resolution_target); original_resolution = nullptr;
            MH_RemoveHook(process_target); original_process = nullptr;
            MH_RemoveHook(target); original = nullptr;
            return RSF_ERROR_NATIVE_REFUSED;
        }
    }
    if (!rsf::skyrim::cpu_prepare(*args->host)) {
        reason = "Skyrim CPU event hooks refused: expected bytes, hook ownership or clock unavailable.";
        return RSF_ERROR_NATIVE_REFUSED;
    }
    services = *args->host;
    using CreateHost = int32_t (*)(const rsf_d3d11_host_setup*, rsf_game_host_services*);
    const auto runtime = GetModuleHandleW(L"ReScaleFrame.Runtime.dll");
    const auto create_host = runtime ? reinterpret_cast<CreateHost>(GetProcAddress(runtime, "rsf_d3d11_host_create")) : nullptr;
    host_active = runtime ? reinterpret_cast<SetHostActive>(GetProcAddress(runtime, "rsf_d3d11_host_set_active")) : nullptr;
    bind_presentation = runtime ? reinterpret_cast<BindPresentation>(GetProcAddress(runtime, "rsf_d3d11_host_bind_presentation")) : nullptr;
    wchar_t runtime_path[32768]{};
    const auto path_length = runtime ? GetModuleFileNameW(runtime, runtime_path, 32768) : 0;
    if (!create_host || !host_active || !bind_presentation || !path_length || path_length >= 32768) return RSF_ERROR_NOT_READY;
    const auto separator = std::wcsrchr(runtime_path, L'\\');
    if (!separator) return RSF_ERROR_NOT_READY;
    const std::wstring directory(runtime_path, size_t(separator - runtime_path));
    const auto ini = directory + L"\\ReScaleFrame.ini";
    rsf_d3d11_host_setup setup{}; setup.struct_size = sizeof(setup); setup.abi_version = RSF_D3D11_HOST_ABI_VERSION;
    setup.configuration_path = ini.c_str(); setup.configuration_section = L"Rendering";
    setup.runtime_directory = directory.c_str(); setup.identity_utf8 = "skyrim-se"; setup.executable_sha256 = fingerprint;
    setup.engine_type = 0; setup.engine_version_utf8 = "1.7.104";
    setup.project_id_utf8 = "668392a0-ecfe-4d0c-9f58-c2838c5ca308";
    setup.units_to_meters = 0.0142875f; setup.quality_choices = 0x1f;
    setup.motion.struct_size = sizeof(setup.motion); setup.motion.scale_x = setup.motion.scale_y = 1;
    setup.motion.output_scale_x = setup.motion.output_scale_y = 1; setup.motion.invalid_value = -1000;
    if (bound_services.user) {
        if (bound_upstream.session_id != args->host->session_id || bound_upstream.user != args->host->user ||
            bound_upstream.log != args->host->log || bound_upstream.cpu_event != args->host->cpu_event) {
            reason = "Experimental D3D11 host is process-lived; a different session requires process restart.";
            return RSF_ERROR_NATIVE_REFUSED;
        }
        services = bound_services;
    } else {
        const auto host_result = create_host(&setup, &services);
        if (host_result != RSF_OK) return host_result;
        bound_services = services; bound_upstream = *args->host;
    }
    if (!rsf::skyrim::cpu_prepare(services)) return RSF_ERROR_NATIVE_REFUSED;
    observations.store(0, std::memory_order_relaxed);
    pass_observations.store(0, std::memory_order_relaxed);
    prepared = true;
    reason = "Native pre-HDR SR prepared at native resolution; FG and latency ownership remain incomplete.";
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
    if (MH_EnableHook(resolution_target) != MH_OK) {
        MH_DisableHook(process_target); MH_DisableHook(target);
        return RSF_ERROR_NATIVE_REFUSED;
    }
    if (MH_EnableHook(end_target) != MH_OK) {
        MH_DisableHook(resolution_target);
        MH_DisableHook(process_target); MH_DisableHook(target);
        return RSF_ERROR_NATIVE_REFUSED;
    }
    if (MH_EnableHook(upload_target) != MH_OK) {
        MH_DisableHook(end_target); MH_DisableHook(resolution_target);
        MH_DisableHook(process_target); MH_DisableHook(target);
        return RSF_ERROR_NATIVE_REFUSED;
    }
    if (!rsf::skyrim::cpu_start()) {
        MH_DisableHook(upload_target);
        MH_DisableHook(end_target);
        MH_DisableHook(resolution_target);
        MH_DisableHook(process_target); MH_DisableHook(target);
        return RSF_ERROR_NATIVE_REFUSED;
    }
    enabled = true;
    if (host_active) (void)host_active(services.user, 1);
    accepting.store(true, std::memory_order_release);
    reason = "Experimental native-resolution SR active with native temporal fallback; FG and latency ownership remain incomplete.";
    return RSF_OK;
}
catch (...) { return RSF_ERROR_NOT_READY; }

rsf_result quiesce(const rsf_game_control_args* args) noexcept
{
    const auto valid = rsf_game::validate(args); if (valid != RSF_OK) return valid;
    accepting.store(false, std::memory_order_release);
    if (host_active) (void)host_active(services.user, 0);
    rsf::skyrim::cpu_quiesce();
    return calls.load(std::memory_order_acquire) || rsf::skyrim::cpu_busy() || rsf::skyrim::resolution_owned() || rsf::skyrim::auxiliaries_pending() ? RSF_ERROR_BUSY : RSF_OK;
}

rsf_result stop(const rsf_game_control_args* args) noexcept try
{
    const auto valid = rsf_game::validate(args); if (valid != RSF_OK) return valid;
    std::lock_guard<std::mutex> lock(lifecycle);
    accepting.store(false, std::memory_order_release);
    if (host_active) (void)host_active(services.user, 0);
    if (rsf::skyrim::resolution_owned() || rsf::skyrim::auxiliaries_pending()) return RSF_ERROR_BUSY;
    const bool cpu_stopped = rsf::skyrim::cpu_stop();
    if (enabled) {
        const auto jitter_result = MH_DisableHook(target);
        const auto process_result = MH_DisableHook(process_target);
        const auto resolution_result = MH_DisableHook(resolution_target);
        const auto end_result = MH_DisableHook(end_target);
        const auto upload_result = MH_DisableHook(upload_target);
        if ((jitter_result != MH_OK && jitter_result != MH_ERROR_DISABLED) ||
            (process_result != MH_OK && process_result != MH_ERROR_DISABLED) ||
            (resolution_result != MH_OK && resolution_result != MH_ERROR_DISABLED) ||
            (end_result != MH_OK && end_result != MH_ERROR_DISABLED) ||
            (upload_result != MH_OK && upload_result != MH_ERROR_DISABLED)) return RSF_ERROR_BUSY;
        enabled = false;
    }
    if (!cpu_stopped || calls.load(std::memory_order_acquire)) return RSF_ERROR_BUSY;
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
         "Experimental native-resolution SR; reduced-resolution SR, FG and latency remain incomplete."},
        detect, {prepare, start, quiesce, stop, status}};
    return RSF_OK;
}
