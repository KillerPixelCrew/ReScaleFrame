// SPDX-License-Identifier: GPL-3.0-only
#include "native_observer.h"
#include "native_geometry.h"
#include "native_hooks.h"
#include "native_profile.h"
#include <rescaleframe/game_plugin_util.h>
#include <rescaleframe/sha256_file.h>
#include <windows.h>
#include <atomic>
#include <cmath>
#include <cstdio>
#include <cstring>
#include <cwchar>
#include <mutex>
#include <string>

namespace rsf::anomaly {
namespace {
std::mutex control_guard, observation_guard;
std::atomic<bool> active{};
std::atomic<uint32_t> in_flight{};
bool prepared{};
bool geometry_installed{};
const char* reason = "Native rendering has not been prepared.";
rsf_game_host_services host{};
HookSet hooks;
std::byte* image{};
std::array<void*, native_sites.size()> originals{};
NativeObservation observation{sizeof(observation), observation_abi};
struct Invocation {
    bool observe;
    Invocation() noexcept {
        in_flight.fetch_add(1, std::memory_order_acq_rel);
        observe = active.load(std::memory_order_acquire);
    }
    ~Invocation() { in_flight.fetch_sub(1, std::memory_order_release); }
};
using Unary = void(*)(void*);
using Camera = void(*)(void*, const float*, const float*);
using CopyTarget = void(*)(void*, uint32_t);
void report(const char* message) noexcept {
    if (host.log) try { host.log(host.user, message); } catch (...) {}
}
void render_begin(void* self) {
    Invocation call;
    reinterpret_cast<Unary>(originals[0])(self);
    if (call.observe) {
        std::lock_guard lock(observation_guard);
        ++observation.render_frames;
        observation.engine_frame = *reinterpret_cast<const uint32_t*>(image + device_frame_rva);
        observation.width = *reinterpret_cast<const uint32_t*>(image + device_width_rva);
        observation.height = *reinterpret_cast<const uint32_t*>(image + device_height_rva);
    }
}
void render_end(void* self) {
    Invocation call;
    reinterpret_cast<Unary>(originals[1])(self);
    if (!call.observe) return;
    NativeObservation current{};
    { std::lock_guard lock(observation_guard); current = observation; }
    if (current.render_frames == 1 || current.render_frames % 120 == 0) {
        char text[384]{};
        std::snprintf(text, sizeof(text),
            "Anomaly native boundaries engine_frame=%u render_frames=%llu scene=%llu hud=%llu pda=%llu scope=%llu camera=%llu output=%ux%u reset=%llu; SR/FG execution not established",
            current.engine_frame, static_cast<unsigned long long>(current.render_frames),
            static_cast<unsigned long long>(current.scene_passes), static_cast<unsigned long long>(current.hud_passes),
            static_cast<unsigned long long>(current.pda_copies), static_cast<unsigned long long>(current.scope_copies),
            static_cast<unsigned long long>(current.camera_updates), current.width, current.height,
            static_cast<unsigned long long>(current.resets));
        report(text);
        if (geometry::geometry_hooks_active()) {
            const auto stats = geometry::geometry_stats();
            std::snprintf(text, sizeof(text),
                "Anomaly geometry identity registered=%llu retired=%llu draws_identified=%llu draws_unidentified=%llu faults=%llu; no history consumer, motion not established",
                static_cast<unsigned long long>(stats.models_registered), static_cast<unsigned long long>(stats.models_retired),
                static_cast<unsigned long long>(stats.draws_identified), static_cast<unsigned long long>(stats.draws_unidentified),
                static_cast<unsigned long long>(stats.capture_faults));
            report(text);
            geometry::geometry_report_coverage([](void*, const char* line) { report(line); }, nullptr);
        }
    }
}
void camera(void* self, const float* view, const float* projection) {
    Invocation call;
    reinterpret_cast<Camera>(originals[2])(self, view, projection);
    if (call.observe && view && projection) {
        std::lock_guard lock(observation_guard);
        ++observation.camera_updates;
        std::memcpy(observation.view, view, sizeof(observation.view));
        std::memcpy(observation.projection, projection, sizeof(observation.projection));
        observation.camera_finite = 1;
        for (size_t i = 0; i < 16; ++i)
            if (!std::isfinite(view[i]) || !std::isfinite(projection[i])) observation.camera_finite = 0;
    }
}
void scene(void* self) {
    Invocation call;
    if (call.observe) { std::lock_guard lock(observation_guard); ++observation.scene_passes; }
    // Main or scope view, world projection, for every geometry draw this CRender::Render makes (shadow and Z-fill
    // passes included; they are classified by phase). Inert unless the geometry hooks are active.
    const geometry::RenderViewScope view;
    reinterpret_cast<Unary>(originals[3])(self);
}
void screen_ui(void* self) {
    Invocation call;
    if (call.observe) { std::lock_guard lock(observation_guard); ++observation.hud_passes; }
    reinterpret_cast<Unary>(originals[4])(self);
}
void target_copy(void* self, uint32_t target) {
    Invocation call;
    reinterpret_cast<CopyTarget>(originals[5])(self, target);
    if (call.observe) {
        std::lock_guard lock(observation_guard);
        if (target == 1) ++observation.pda_copies;
        if (target == 2) ++observation.scope_copies;
    }
}
void reset_begin(void* self) {
    Invocation call;
    if (call.observe) {
        std::lock_guard lock(observation_guard);
        ++observation.resets; ++observation.generation; observation.camera_finite = 0;
    }
    reinterpret_cast<Unary>(originals[6])(self);
}
void reset_end(void* self) {
    Invocation call;
    reinterpret_cast<Unary>(originals[7])(self);
}
std::array<HookSite, native_sites.size()> sites() noexcept {
    const std::array<void*, native_sites.size()> replacements{
        reinterpret_cast<void*>(&render_begin), reinterpret_cast<void*>(&render_end),
        reinterpret_cast<void*>(&camera), reinterpret_cast<void*>(&scene),
        reinterpret_cast<void*>(&screen_ui), reinterpret_cast<void*>(&target_copy),
        reinterpret_cast<void*>(&reset_begin), reinterpret_cast<void*>(&reset_end)};
    std::array<HookSite, native_sites.size()> result{};
    for (size_t i = 0; i < result.size(); ++i)
        result[i] = {native_sites[i].name, native_sites[i].rva, native_sites[i].expected,
            replacements[i], &originals[i]};
    return result;
}
}
rsf_result prepare(const rsf_game_prepare_args* args) noexcept try {
    if (const auto error = rsf_game::validate(args); error != RSF_OK) return error;
    if (const auto error = rsf_game::validate(args->host); error != RSF_OK) return error;
    std::lock_guard lock(control_guard);
    if (prepared || active.load() || in_flight.load()) return RSF_ERROR_BUSY;
    wchar_t path[32768]{};
    const auto length = GetModuleFileNameW(nullptr, path, 32768);
    if (!length || length >= 32768) return RSF_ERROR_NATIVE_REFUSED;
    const auto* separator = std::wcsrchr(path, L'\\');
    const auto* name = separator ? separator + 1 : path;
    if (_wcsicmp(name, L"AnomalyDX11.exe")) {
        reason = "Native sites belong to the standard DX11 executable, not this process.";
        return RSF_ERROR_NATIVE_REFUSED;
    }
    std::string hash;
    if (!rsf::sha256_file(path, hash) || hash != dx11_hash) {
        reason = "Running executable SHA-256 does not match the guarded native profile.";
        return RSF_ERROR_NATIVE_REFUSED;
    }
    image = reinterpret_cast<std::byte*>(GetModuleHandleW(nullptr));
    const auto* dos = reinterpret_cast<const IMAGE_DOS_HEADER*>(image);
    const auto* pe = reinterpret_cast<const IMAGE_NT_HEADERS64*>(image + dos->e_lfanew);
    if (pe->OptionalHeader.SizeOfImage != native_image_size) {
        reason = "Running image size differs from the verified build."; return RSF_ERROR_NATIVE_REFUSED;
    }
    for (const auto& site : native_sites) {
        if (std::memcmp(image + site.rva, site.expected.data(), site.expected.size())) {
            reason = "A native hook's expected instructions do not match; no hook was written.";
            return RSF_ERROR_NATIVE_REFUSED;
        }
    }
    host = *args->host;
    prepared = true;
    reason = "Exact native boundary sites prepared; temporal SR/FG is not validated.";
    return RSF_OK;
} catch (...) { return RSF_ERROR_NATIVE_REFUSED; }
rsf_result start(const rsf_game_start_args* args) noexcept {
    if (const auto error = rsf_game::validate(args); error != RSF_OK) return error;
    std::unique_lock lock(control_guard);
    if (!prepared) return RSF_ERROR_NOT_READY;
    if (active.load()) return RSF_OK;
    HMODULE pinned{};
    if (!GetModuleHandleExW(GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_PIN,
        reinterpret_cast<LPCWSTR>(reinterpret_cast<void*>(&start)), &pinned)) return RSF_ERROR_NATIVE_REFUSED;
    const bool installed = hooks.created() ? hooks.enable() : hooks.install(image, native_image_size, sites());
    if (!installed) { reason = "Guarded native detour installation refused."; return RSF_ERROR_NATIVE_REFUSED; }
    // Geometry lifetime/identity hooks are a separate guarded set. Refusal leaves boundary observation intact
    // and simply produces no geometry identities.
    geometry_installed = geometry::install_native_geometry(image, native_image_size, dx11_hash);
    active.store(true, std::memory_order_release);
    reason = geometry_installed
        ? "Native boundaries and geometry identity hooks active; observation does not establish temporal SR/FG execution."
        : "Native boundaries active; geometry identity hooks refused; observation does not establish temporal SR/FG execution.";
    lock.unlock();
    report(geometry_installed ? "Anomaly guarded native hooks installed, including geometry identity. No executable file was modified."
        : "Anomaly guarded native hooks installed; geometry identity hooks were refused. No executable file was modified.");
    return RSF_OK;
}
rsf_result quiesce(const rsf_game_control_args* args) noexcept {
    if (const auto error = rsf_game::validate(args); error != RSF_OK) return error;
    std::lock_guard lock(control_guard);
    active.store(false, std::memory_order_release);
    if (!hooks.disable()) return RSF_ERROR_NATIVE_REFUSED;
    if (geometry_installed) {
        if (!geometry::disable_native_geometry()) return RSF_ERROR_NATIVE_REFUSED;
        geometry_installed = false;
    }
    return in_flight.load(std::memory_order_acquire) ? RSF_ERROR_BUSY : RSF_OK;
}
rsf_result stop(const rsf_game_control_args* args) noexcept {
    const auto result = quiesce(args);
    if (result != RSF_OK) return result;
    std::lock_guard lock(control_guard);
    if (in_flight.load(std::memory_order_acquire)) return RSF_ERROR_BUSY;
    prepared = false;
    host = {};
    reason = "Native callbacks inactive; process-lifetime trampolines remain owned by the pinned adapter.";
    return RSF_OK;
}
rsf_result status(rsf_game_renderer_status* output) noexcept {
    if (const auto error = rsf_game::validate(output); error != RSF_OK) return error;
    std::lock_guard lock(control_guard);
    *output = {sizeof(*output), RSF_GAME_ABI_VERSION, uint32_t(prepared), uint32_t(active.load()), 0, reason};
    return RSF_OK;
}
}
extern "C" __declspec(dllexport) rsf_result rsf_anomaly_observation(
    rsf::anomaly::NativeObservation* output) noexcept {
    using namespace rsf::anomaly;
    if (!output || output->struct_size < sizeof(*output)) return RSF_ERROR_INVALID_ARGUMENT;
    if (output->abi_version != observation_abi) return RSF_ERROR_ABI_MISMATCH;
    std::lock_guard lock(observation_guard);
    *output = observation;
    output->active = uint32_t(active.load());
    return RSF_OK;
}
