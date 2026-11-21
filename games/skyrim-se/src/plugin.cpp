// SPDX-License-Identifier: MIT
// GameSDK export, native detours and leaves of the Rust Skyrim SE owner (src/session.rs). Rust owns
// the lifecycle order, results and reasons, the Runtime host setup and its original-session reuse,
// DPI startup, the observer hook record and the borrowed services, which the detours lease under
// the session lock. This file keeps the five typed observers and their native facts, MinHook,
// exception guards and the C++ CPU, sampling, presentation-input and compatibility subsystems.
#include <rescaleframe/game_api.h>
#include <MinHook.h>
#include <d3d11.h>
#include <atomic>
#include <cstdio>
#include <cstring>
#include <cwchar>
#include "cpu_events.h"
#include "render_inputs.h"
#include "resolution.h"
#include "auxiliary.h"
#include "sampling.h"
#include "presentation_inputs.h"
#include "compatibility.h"
#include "session_platform.h"
#include <rescaleframe/d3d11_host.h>
#if defined(_MSC_VER)
#include <intrin.h>
#endif

namespace {
// Window class policy lent to the Runtime presentation bridge through the host setup.
int accept_presentation_window(void*, void* window) {
    wchar_t name[128]{};
    return window && GetClassNameW(static_cast<HWND>(window), name, 128) &&
        std::wcscmp(name, L"Skyrim Special Edition") == 0;
}
using UpdateJitter = void (*)(void*);
UpdateJitter original = nullptr;
void* target = nullptr;
// The matched game consumes three arguments. Carry the CS wrapper's two extra
// machine arguments unchanged, including its fifth stack byte (not a bool read).
using ProcessMain = uintptr_t (*)(void*, uint32_t, uint32_t, uintptr_t, uint8_t);
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
std::atomic<uint32_t> calls{0};
std::atomic<uint64_t> observations{0};
std::atomic<uint64_t> pass_observations{0};
using CreateHost = int32_t (*)(const rsf_d3d11_host_setup*, rsf_game_host_services*);
using SetHostActive = int32_t (*)(void*, uint32_t);
using BindPresentation = int32_t (*)(void*, const rsf_d3d11_presentation_binding*);
bool accepting() noexcept { return rsf_skyrim_session_accepting(); }

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

uintptr_t observe_process(void* manager, uint32_t input, uint32_t output, uintptr_t argument4, uint8_t argument5) noexcept
{
#if defined(_MSC_VER)
    const auto caller = uintptr_t(_ReturnAddress());
#else
    const auto caller = uintptr_t(__builtin_return_address(0));
#endif
    calls.fetch_add(1, std::memory_order_acq_rel);
    bool reconstructed = false;
    if (accepting() && rsf::skyrim::compatibility_render_allowed() && input == 1 && output == 0 &&
        rsf::skyrim::compatibility_postprocess_call(caller, manager, input, output, argument4, argument5)) {
        rsf_game_host_services host{};
        (void)rsf_skyrim_session_lease(true, &host);
        try { (void)rsf::skyrim::prepare_presentation_inputs(manager, host); } catch (...) {}
        try { reconstructed = rsf::skyrim::reconstruct(manager, host); } catch (...) {}
    }
    if (accepting()) {
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
            (void)rsf_skyrim_session_lease(true, &host);
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
    const auto result = original_process(manager, input, output, argument4, argument5);
    if (bypass) *reinterpret_cast<unsigned char*>(taa + 0x18) = temporal_enabled;
    calls.fetch_sub(1, std::memory_order_acq_rel);
    return result;
}

void observe_jitter(void* state) noexcept
{
    calls.fetch_add(1, std::memory_order_acq_rel);
    // The trampoline is process-lived. The session lease never spans native execution.
    original(state);
    rsf_game_host_services resolution_host{};
    (void)rsf_skyrim_session_lease(true, &resolution_host);
    if (!rsf::skyrim::compatibility_render_allowed()) resolution_host = {};
    try { rsf::skyrim::configure_resolution(state, resolution_host); } catch (...) {}
    if (accepting()) {
        const auto number = observations.fetch_add(1, std::memory_order_relaxed);
        if (number < 16) {
            struct Dimensions { uint32_t width, height; } dimensions{};
            struct Projection { float x, y; } projection{};
            const auto* bytes = static_cast<const unsigned char*>(state);
            const bool readable = state && copy(&dimensions, bytes + 0x24, sizeof(dimensions)) &&
                copy(&projection, bytes + 0x44, sizeof(projection));
            rsf_game_host_services host{};
            (void)rsf_skyrim_session_lease(true, &host);
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
    if (!rsf::skyrim::resolution_controls(state)) original_resolution(state);
    calls.fetch_sub(1, std::memory_order_acq_rel);
}

uintptr_t observe_end() noexcept {
    calls.fetch_add(1, std::memory_order_acq_rel);
    // The retained services, also between quiesce and stop, let pending auxiliaries finish.
    rsf_game_host_services host{};
    const auto bind_presentation = reinterpret_cast<BindPresentation>(rsf_skyrim_session_lease(false, &host));
    if (accepting() && rsf::skyrim::compatibility_render_allowed() && bind_presentation) {
        uintptr_t window{}; void* chain{};
        const auto base = reinterpret_cast<const unsigned char*>(GetModuleHandleW(nullptr));
        if (copy(&window, base + 0x3330198, sizeof(window)) && window &&
            copy(&chain, reinterpret_cast<const void*>(window + 0x18), sizeof(chain)) && chain) {
            const rsf_d3d11_presentation_binding binding{sizeof(binding), RSF_D3D11_HOST_ABI_VERSION, chain};
            try { (void)bind_presentation(host.user, &binding); } catch (...) {}
        }
    }
    rsf::skyrim::finish_sampling();
    const auto result = original_end();
    rsf::skyrim::finish_presentation_inputs();
    rsf::skyrim::finish_auxiliaries(host);
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

template<bool (*Guard)()>
int32_t observer_guard() noexcept try { return Guard() ? 1 : 0; }
catch (...) { return -1; }

int32_t admit_observers()
{
    wchar_t path[32768]{};
    const auto length = GetModuleFileNameW(nullptr, path, 32768);
    rsf_skyrim_observer_targets sites{};
    const auto result = rsf_skyrim_observer_admit(path, length, rsf::skyrim::creation_platform(),
        observer_guard<rsf::skyrim::sampling_guard>, observer_guard<rsf::skyrim::presentation_inputs_guard>, &sites);
    if (result == 1) {
        target = sites.jitter;
        process_target = sites.process;
        resolution_target = sites.resolution;
        end_target = sites.end;
        upload_target = sites.upload;
    }
    return result;
}

struct ObserverHook {
    void* const* address;
    void* detour;
    void** trampoline;
};
const ObserverHook& observer_hook(uint32_t index) noexcept
{
    static const ObserverHook hooks[]{
        {&target, reinterpret_cast<void*>(&observe_jitter), reinterpret_cast<void**>(&original)},
        {&process_target, reinterpret_cast<void*>(&observe_process), reinterpret_cast<void**>(&original_process)},
        {&resolution_target, reinterpret_cast<void*>(&observe_resolution), reinterpret_cast<void**>(&original_resolution)},
        {&end_target, reinterpret_cast<void*>(&observe_end), reinterpret_cast<void**>(&original_end)},
        {&upload_target, reinterpret_cast<void*>(&observe_upload), reinterpret_cast<void**>(&original_upload)},
    };
    return hooks[index];
}

constexpr rsf_skyrim_observer_leaves observer_leaves{
    []() noexcept {
        const auto result = MH_Initialize();
        return result == MH_OK || result == MH_ERROR_ALREADY_INITIALIZED;
    },
    [](uint32_t index) noexcept {
        const auto& hook = observer_hook(index);
        return MH_CreateHook(*hook.address, hook.detour, hook.trampoline) == MH_OK;
    },
    [](uint32_t index) noexcept {
        const auto& hook = observer_hook(index);
        const auto result = MH_RemoveHook(*hook.address);
        if (result != MH_OK && result != MH_ERROR_NOT_CREATED) return false;
        *hook.trampoline = nullptr;
        return true;
    },
    [](uint32_t index) noexcept -> int32_t {
        return static_cast<int32_t>(MH_EnableHook(*observer_hook(index).address));
    },
    [](uint32_t index) noexcept {
        const auto result = MH_DisableHook(*observer_hook(index).address);
        return result == MH_OK || result == MH_ERROR_DISABLED;
    },
    []() noexcept {
        HMODULE pinned = nullptr;
        return GetModuleHandleExW(GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_PIN,
            reinterpret_cast<LPCWSTR>(&observe_jitter), &pinned) != FALSE;
    },
    [](const rsf_game_host_services* host, const char* message) noexcept {
        if (host->log) try { host->log(host->user, message); } catch (...) {}
    },
};
static_assert(MH_OK == 0);

// A subsystem call that throws reports false; the Rust session then retires what it started and
// answers RSF_ERROR_NOT_READY as the former catch did.
bool subsystem(uint32_t operation, const rsf_game_host_services* services, int32_t* value) noexcept try
{
    using namespace rsf::skyrim;
    const auto flag = [value](bool result) { *value = result ? 1 : 0; };
    switch (operation) {
    case RSF_SKYRIM_ADMIT_OBSERVERS: *value = admit_observers(); break;
    case RSF_SKYRIM_OBSERVERS_BUSY: flag(calls.load(std::memory_order_acquire) != 0); break;
    case RSF_SKYRIM_RESET_OBSERVATIONS:
        observations.store(0, std::memory_order_relaxed);
        pass_observations.store(0, std::memory_order_relaxed);
        flag(true);
        break;
    case RSF_SKYRIM_CPU_PREPARE: flag(services && cpu_prepare(*services)); break;
    case RSF_SKYRIM_CPU_START: flag(cpu_start()); break;
    case RSF_SKYRIM_CPU_QUIESCE: cpu_quiesce(); flag(true); break;
    case RSF_SKYRIM_CPU_DISABLE: flag(cpu_disable()); break;
    case RSF_SKYRIM_CPU_STOP: flag(cpu_stop()); break;
    case RSF_SKYRIM_CPU_BUSY: flag(cpu_busy()); break;
    case RSF_SKYRIM_SAMPLING_START: flag(services && sampling_start(*services)); break;
    case RSF_SKYRIM_SAMPLING_QUIESCE: sampling_quiesce(); flag(true); break;
    case RSF_SKYRIM_SAMPLING_STOP: flag(sampling_stop()); break;
    case RSF_SKYRIM_SAMPLING_BUSY: flag(sampling_busy()); break;
    case RSF_SKYRIM_PRESENTATION_START: flag(services && presentation_inputs_start(*services)); break;
    case RSF_SKYRIM_PRESENTATION_QUIESCE: presentation_inputs_quiesce(); flag(true); break;
    case RSF_SKYRIM_PRESENTATION_STOP: flag(presentation_inputs_stop()); break;
    case RSF_SKYRIM_PRESENTATION_BUSY: flag(presentation_inputs_busy()); break;
    case RSF_SKYRIM_RESOLUTION_OWNED: flag(resolution_owned()); break;
    case RSF_SKYRIM_AUXILIARIES_PENDING: flag(auxiliaries_pending()); break;
    default: flag(false); break;
    }
    return true;
}
catch (...) { return false; }

const rsf_skyrim_session_leaves session_leaves{
    sizeof(rsf_skyrim_session_leaves), RSF_SKYRIM_SESSION_LEAVES_VERSION,
    rsf::skyrim::creation_platform(), &observer_leaves,
    subsystem,
    [](const char** refusal, bool arm) noexcept {
        return rsf::skyrim::compatibility_checkpoint(*refusal, arm ? &rsf_skyrim_session_withdraw : nullptr);
    },
    []() noexcept { return rsf::skyrim::compatibility_status(); },
    [](const rsf_game_host_services* host, const char* message) noexcept {
        try { if (host && host->log) host->log(host->user, message); return true; }
        catch (...) { return false; }
    },
    [](uintptr_t module, wchar_t* path, uint32_t capacity) noexcept {
        return uint32_t(GetModuleFileNameW(reinterpret_cast<HMODULE>(module), path, capacity));
    },
    [](uintptr_t create, const rsf_d3d11_host_setup* setup, rsf_game_host_services* services, int32_t* result) noexcept {
        try { *result = reinterpret_cast<CreateHost>(create)(setup, services); return true; }
        catch (...) { return false; }
    },
    [](uintptr_t activate, void* user, uint32_t active, int32_t* result) noexcept {
        try { *result = reinterpret_cast<SetHostActive>(activate)(user, active); return true; }
        catch (...) { return false; }
    },
    accept_presentation_window,
};
}

extern "C" __declspec(dllexport) rsf_result rsf_get_game_plugin_api(
    uint32_t requested_abi, rsf_game_plugin_api* api) noexcept
{
    return rsf_skyrim_publish_api(requested_abi, api, &session_leaves);
}
