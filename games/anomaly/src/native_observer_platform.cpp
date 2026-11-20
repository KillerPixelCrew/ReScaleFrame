// SPDX-License-Identifier: MIT
#include "native_observer.h"
#include "build_profile.h"
#include "fg_stage.h"
#include "input_hooks.h"
#include "motion_stage.h"
#include "native_geometry.h"
#include "native_hooks.h"
#include "sr_stage.h"
#include <rescaleframe/sha256_file.h>
#include <windows.h>
#include <cstdio>
#include <cstring>
#include <cwchar>
#include <string>

extern "C" {
rsf_result rsf_anomaly_observer_prepare(const rsf_game_prepare_args*, const rsf_anomaly_observer_platform*) noexcept;
rsf_result rsf_anomaly_observer_start(const rsf_game_start_args*, const rsf_anomaly_observer_platform*) noexcept;
rsf_result rsf_anomaly_observer_quiesce(const rsf_game_control_args*, const rsf_anomaly_observer_platform*) noexcept;
rsf_result rsf_anomaly_observer_stop(const rsf_game_control_args*, const rsf_anomaly_observer_platform*) noexcept;
rsf_result rsf_anomaly_observer_status(rsf_game_renderer_status*) noexcept;
rsf_result rsf_anomaly_observer_read(rsf_anomaly_native_observation*) noexcept;
bool rsf_anomaly_observer_enter() noexcept;
void rsf_anomaly_observer_leave() noexcept;
void rsf_anomaly_observer_render_begin(const rsf_anomaly_observer_platform*) noexcept;
void rsf_anomaly_observer_end_before(const rsf_anomaly_observer_platform*) noexcept;
void rsf_anomaly_observer_end_after(const rsf_anomaly_observer_platform*) noexcept;
void rsf_anomaly_observer_camera_sample(const rsf_anomaly_observer_platform*, const float*, const float*) noexcept;
void rsf_anomaly_observer_scene() noexcept;
void rsf_anomaly_observer_ui(const rsf_anomaly_observer_platform*) noexcept;
void rsf_anomaly_observer_copy_target(const rsf_anomaly_observer_platform*, uint32_t) noexcept;
void rsf_anomaly_observer_reset(const rsf_anomaly_observer_platform*) noexcept;
}

namespace rsf::anomaly {
namespace {
extern const rsf_anomaly_observer_platform platform;
HookSet hooks;
std::array<void*, native_site_count> originals{};

// Native invocations and their trampolines stay on the C++ side. The scope also
// retires the Rust invocation when the native function unwinds through C++.
struct Invocation {
    const bool observe = rsf_anomaly_observer_enter();
    ~Invocation() { rsf_anomaly_observer_leave(); }
};
using Unary = void(*)(void*);
using Camera = void(*)(void*, const float*, const float*);
using CopyTarget = void(*)(void*, uint32_t);
void render_begin(void* self) {
    const Invocation call;
    reinterpret_cast<Unary>(originals[0])(self);
    if (call.observe) rsf_anomaly_observer_render_begin(&platform);
}
void render_end(void* self) {
    const Invocation call;
    if (call.observe) rsf_anomaly_observer_end_before(&platform);
    reinterpret_cast<Unary>(originals[1])(self);
    if (call.observe) rsf_anomaly_observer_end_after(&platform);
}
void camera(void* self, const float* view, const float* projection) {
    const Invocation call;
    reinterpret_cast<Camera>(originals[2])(self, view, projection);
    if (call.observe) rsf_anomaly_observer_camera_sample(&platform, view, projection);
}
void scene(void* self) {
    const Invocation call;
    if (call.observe) rsf_anomaly_observer_scene();
    const geometry::RenderViewScope view;
    reinterpret_cast<Unary>(originals[3])(self);
}
void screen_ui(void* self) {
    const Invocation call;
    if (call.observe) rsf_anomaly_observer_ui(&platform);
    reinterpret_cast<Unary>(originals[4])(self);
}
void target_copy(void* self, uint32_t target) {
    const Invocation call;
    reinterpret_cast<CopyTarget>(originals[5])(self, target);
    if (call.observe) rsf_anomaly_observer_copy_target(&platform, target);
}
void reset_begin(void* self) {
    const Invocation call;
    if (call.observe) rsf_anomaly_observer_reset(&platform);
    reinterpret_cast<Unary>(originals[6])(self);
}
void reset_end(void* self) {
    const Invocation call;
    reinterpret_cast<Unary>(originals[7])(self);
}

uint32_t image_size(const std::byte* image) noexcept {
    if (!image) return 0;
#ifdef _MSC_VER
    __try {
#endif
        const auto* dos = reinterpret_cast<const IMAGE_DOS_HEADER*>(image);
        if (dos->e_magic != IMAGE_DOS_SIGNATURE || dos->e_lfanew < 0) return 0;
        const auto* pe = reinterpret_cast<const IMAGE_NT_HEADERS64*>(image + dos->e_lfanew);
        if (pe->Signature != IMAGE_NT_SIGNATURE || pe->FileHeader.Machine != IMAGE_FILE_MACHINE_AMD64 ||
            pe->OptionalHeader.Magic != IMAGE_NT_OPTIONAL_HDR64_MAGIC) return 0;
        return pe->OptionalHeader.SizeOfImage;
#ifdef _MSC_VER
    } __except(EXCEPTION_EXECUTE_HANDLER) { return 0; }
#endif
}
bool guarded_sites(const std::byte* image, const BuildProfile& build) noexcept {
#ifdef _MSC_VER
    __try {
#endif
        for (const auto& site : build.native->native_sites) {
            if (site.rva > build.image_size() || site.expected.size() > build.image_size() - site.rva ||
                std::memcmp(image + site.rva, site.expected.data(), site.expected.size()) ||
                !unique_guards_match(image, build.image_size(), build.native->native_unique_guards, site.rva))
                return false;
        }
        return true;
#ifdef _MSC_VER
    } __except(EXCEPTION_EXECUTE_HANDLER) { return false; }
#endif
}
const char* identify(rsf_anomaly_observer_engine* out) noexcept try {
    wchar_t path[32768]{};
    const auto length = GetModuleFileNameW(nullptr, path, 32768);
    if (!length || length >= 32768) return "Could not read the running executable path.";
    const auto* separator = std::wcsrchr(path, L'\\');
    const auto* name = separator ? separator + 1 : path;
    bool profiled_name = false;
    for (const auto& candidate : build_profiles) profiled_name |= !_wcsicmp(name, candidate.executable_wide);
    if (!profiled_name)
        return "Native sites belong to the profiled DX11 executables (standard or AVX), not this process.";
    std::string hash;
    const BuildProfile* build = nullptr;
    if (!rsf::sha256_file(path, hash) || !(build = find_build(std::wstring_view{name}, hash)))
        return "Running executable SHA-256 does not match a guarded native profile.";
    auto* image = reinterpret_cast<std::byte*>(GetModuleHandleW(nullptr));
    if (image_size(image) != build->image_size()) return "Running image size differs from the verified build.";
    if (!guarded_sites(image, *build))
        return "A native hook's expected instructions do not match; no hook was written.";
    *out = {image, static_cast<size_t>(build - build_profiles.data()), build->label, build->executable};
    return nullptr;
} catch (...) { return "Could not verify the running native profile."; }

bool enable_hooks(const rsf_anomaly_observer_engine* engine) noexcept {
    const auto& build = build_profiles[engine->build];
    auto* image = static_cast<std::byte*>(engine->image);
    if (hooks.created()) return hooks.enable();
    const std::array<void*, native_site_count> replacements{
        reinterpret_cast<void*>(&render_begin), reinterpret_cast<void*>(&render_end),
        reinterpret_cast<void*>(&camera), reinterpret_cast<void*>(&scene),
        reinterpret_cast<void*>(&screen_ui), reinterpret_cast<void*>(&target_copy),
        reinterpret_cast<void*>(&reset_begin), reinterpret_cast<void*>(&reset_end)};
    std::array<HookSite, native_site_count> sites{};
    for (size_t i = 0; i < sites.size(); ++i) {
        const auto& site = build.native->native_sites[i];
        sites[i] = {site.name, site.rva, site.expected, replacements[i], &originals[i]};
    }
    return hooks.install(image, build.image_size(), sites);
}

bool guarded_copy(void* output, const void* input, size_t size) noexcept {
#ifdef _MSC_VER
    __try {
#endif
        std::memcpy(output, input, size);
        return true;
#ifdef _MSC_VER
    } __except(EXCEPTION_EXECUTE_HANDLER) { return false; }
#endif
}
bool read_frame(const rsf_anomaly_observer_engine* engine, rsf_anomaly_observer_frame* out) noexcept {
    const auto* image = static_cast<const std::byte*>(engine->image);
    const auto& profile = *build_profiles[engine->build].native;
    return guarded_copy(&out->id, image + profile.device_frame_rva, sizeof(out->id)) &&
        guarded_copy(&out->width, image + profile.device_width_rva, sizeof(out->width)) &&
        guarded_copy(&out->height, image + profile.device_height_rva, sizeof(out->height));
}
bool read_camera(const float* view, const float* projection, rsf_anomaly_observer_camera* out) noexcept {
    return guarded_copy(out->view, view, sizeof(out->view)) &&
        guarded_copy(out->projection, projection, sizeof(out->projection));
}

void log(rsf_game_log_fn emit, void* user, const char* text) noexcept {
    if (emit) try { emit(user, text); } catch (...) {}
}
void report_stages(rsf_game_log_fn emit, void* user) noexcept {
    if (!emit) return;
    struct Sink { rsf_game_log_fn emit; void* user; } sink{emit, user};
    const auto forward = [](void* context, const char* text) {
        const auto& s = *static_cast<Sink*>(context);
        log(s.emit, s.user, text);
    };
    if (geometry::geometry_hooks_active()) {
        const auto stats = geometry::geometry_stats();
        char text[384]{};
        using ull = unsigned long long;
        std::snprintf(text, sizeof(text),
            "Anomaly geometry identity registered=%llu retired=%llu draws_identified=%llu draws_unidentified=%llu faults=%llu; no history consumer, motion not established",
            static_cast<ull>(stats.models_registered), static_cast<ull>(stats.models_retired),
            static_cast<ull>(stats.draws_identified), static_cast<ull>(stats.draws_unidentified),
            static_cast<ull>(stats.capture_faults));
        forward(&sink, text);
        geometry::geometry_report_coverage(forward, &sink);
    }
    sr::report(forward, &sink);
    motion::report(forward, &sink);
    fg::report(forward, &sink);
}

const rsf_anomaly_observer_platform platform{
    identify, enable_hooks, [] { return hooks.disable(); },
    [](const rsf_anomaly_observer_engine* engine) {
        const auto& build = build_profiles[engine->build];
        return geometry::install_native_geometry(static_cast<std::byte*>(engine->image), build.image_size(), build);
    },
    geometry::disable_native_geometry,
    [] { return sr::options().selected(); },
    [](const rsf_anomaly_observer_engine* engine) {
        const auto& build = build_profiles[engine->build];
        return sr::install(static_cast<std::byte*>(engine->image), build.image_size(), build);
    },
    sr::quiesce,
    [](const rsf_anomaly_observer_engine* engine) {
        const auto& build = build_profiles[engine->build];
        return motion::install(static_cast<std::byte*>(engine->image), build.image_size(), build);
    },
    motion::quiesce, input::enable, input::disable, input::idle, read_frame, read_camera,
    fg::render_begin, sr::frame_end, fg::end_before, fg::end_after,
    fg::camera, fg::ui_boundary, fg::scope_frame, fg::device_reset, log, report_stages,
};
}

rsf_result prepare(const rsf_game_prepare_args* args) noexcept { return rsf_anomaly_observer_prepare(args, &platform); }
rsf_result start(const rsf_game_start_args* args) noexcept { return rsf_anomaly_observer_start(args, &platform); }
rsf_result quiesce(const rsf_game_control_args* args) noexcept { return rsf_anomaly_observer_quiesce(args, &platform); }
rsf_result stop(const rsf_game_control_args* args) noexcept { return rsf_anomaly_observer_stop(args, &platform); }
rsf_result status(rsf_game_renderer_status* out) noexcept { return rsf_anomaly_observer_status(out); }
}

extern "C" __declspec(dllexport) rsf_result rsf_anomaly_observation(rsf::anomaly::NativeObservation* out) noexcept {
    return rsf_anomaly_observer_read(out);
}
