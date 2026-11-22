// SPDX-License-Identifier: MIT
#include "native_observer.h"
#include "build_profile.h"
#include "fg_stage.h"
#include "input_hooks.h"
#include "motion_stage.h"
#include "native_geometry.h"
#include "native_hooks.h"
#include "native_memory.h"
#include "sr_stage.h"
#include <cstdio>

extern "C" {
const char* rsf_anomaly_observer_identify(rsf_anomaly_observer_engine*, const rsf_anomaly_build_identity*,
    const rsf_anomaly_observer_profile*, size_t, const rsf::anomaly::ImageHeaders*,
    decltype(&rsf::anomaly::guarded_copy)) noexcept;
bool rsf_anomaly_observer_enter() noexcept;
void rsf_anomaly_observer_leave() noexcept;
using rsf_anomaly_observer_install_fn = bool (*)(void*, uint32_t, const rsf::anomaly::HookSite*, size_t) noexcept;
bool rsf_anomaly_observer_install(const rsf_anomaly_observer_engine*, const rsf_anomaly_observer_binding*,
    rsf_anomaly_observer_install_fn) noexcept;
bool rsf_anomaly_observer_read_frame(const rsf_anomaly_observer_engine*, decltype(&rsf::anomaly::guarded_copy),
    rsf_anomaly_observer_frame*) noexcept;
void rsf_anomaly_observer_render_begin(const rsf_anomaly_observer_platform*) noexcept;
bool rsf_anomaly_observer_end_before(const rsf_anomaly_observer_platform*, bool observing) noexcept;
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

static_assert(sizeof(NativeSite) == 32 && offsetof(NativeSite, rva) == 8 && offsetof(NativeSite, expected) == 12);
static_assert(sizeof(UniqueGuard) == 24 && offsetof(UniqueGuard, rva) == 8 && offsetof(UniqueGuard, expected_hex) == 16);
static_assert(sizeof(rsf_anomaly_observer_profile) == 56 && offsetof(rsf_anomaly_observer_profile, sites) == 24);
static_assert(sizeof(rsf_anomaly_observer_engine) == 40 && offsetof(rsf_anomaly_observer_engine, profile) == 32);
static_assert(sizeof(rsf_anomaly_observer_binding) == 16 && offsetof(rsf_anomaly_observer_binding, original) == 8);
constexpr auto observer_profiles = [] {
    std::array<rsf_anomaly_observer_profile, build_profiles.size()> profiles{};
    for (size_t i = 0; i < profiles.size(); ++i) {
        const auto& build = build_profiles[i];
        const auto& native = *build.native;
        profiles[i] = {build.label, build.image_size(), native.device_frame_rva, native.device_width_rva,
            native.device_height_rva, native.native_sites.data(), native.native_sites.size(),
            native.native_unique_guards.data(), native.native_unique_guards.size()};
    }
    return profiles;
}();

// Native invocations and their trampolines stay on the C++ side. The scope also
// retires the Rust invocation when the native function unwinds through C++.
struct Invocation {
    const bool observe = rsf_anomaly_observer_enter();
    ~Invocation() { rsf_anomaly_observer_leave(); }
};
using Unary = void(*)(void*);
using Camera = void(*)(void*, const float*, const float*);
using CopyTarget = void(*)(void*, uint32_t);
// MinHook writes to untyped storage. Cast only at the native call site, and
// bind each replacement to a trampoline slot of the same function type.
template<class Function>
struct Original {
    void* trampoline{};
    Function get() const noexcept { return reinterpret_cast<Function>(trampoline); }
};
template<class Function>
rsf_anomaly_observer_binding bind_original(Original<Function>& original, Function replacement) noexcept {
    return {reinterpret_cast<void*>(replacement), &original.trampoline};
}
Original<Unary> begin_original, end_original, scene_original, ui_original, reset_begin_original, reset_end_original;
Original<Camera> camera_original;
Original<CopyTarget> copy_original;

void render_begin(void* self) {
    const Invocation call;
    begin_original.get()(self);
    if (call.observe) rsf_anomaly_observer_render_begin(&platform);
}
void render_end(void* self) {
    const Invocation call;
    const bool finish = rsf_anomaly_observer_end_before(&platform, call.observe);
    end_original.get()(self);
    if (finish) rsf_anomaly_observer_end_after(&platform);
}
void camera(void* self, const float* view, const float* projection) {
    const Invocation call;
    camera_original.get()(self, view, projection);
    if (call.observe) rsf_anomaly_observer_camera_sample(&platform, view, projection);
}
void scene(void* self) {
    const Invocation call;
    if (call.observe) rsf_anomaly_observer_scene();
    const geometry::RenderViewScope view;
    scene_original.get()(self);
}
void screen_ui(void* self) {
    const Invocation call;
    if (call.observe) rsf_anomaly_observer_ui(&platform);
    ui_original.get()(self);
}
void target_copy(void* self, uint32_t target) {
    const Invocation call;
    copy_original.get()(self, target);
    if (call.observe) rsf_anomaly_observer_copy_target(&platform, target);
}
void reset_begin(void* self) {
    const Invocation call;
    if (call.observe) rsf_anomaly_observer_reset(&platform);
    reset_begin_original.get()(self);
}
void reset_end(void* self) {
    const Invocation call;
    reset_end_original.get()(self);
}

const char* identify(rsf_anomaly_observer_engine* out) noexcept {
    return rsf_anomaly_observer_identify(out, build_identities.data(), observer_profiles.data(),
        observer_profiles.size(), &guarded_image_headers, guarded_copy);
}

bool install_hooks(void* image, uint32_t image_size, const HookSite* sites, size_t count) noexcept {
    return hooks.install(static_cast<std::byte*>(image), image_size, {sites, count});
}

bool enable_hooks(const rsf_anomaly_observer_engine* engine) noexcept {
    if (hooks.created()) return hooks.enable();
    // Same order as native_sites in both exact-build records.
    static_assert(native_site_count == RSF_ANOMALY_OBSERVER_HOOK_COUNT);
    const std::array bindings{
        bind_original(begin_original, &render_begin), bind_original(end_original, &render_end),
        bind_original(camera_original, &camera), bind_original(scene_original, &scene),
        bind_original(ui_original, &screen_ui), bind_original(copy_original, &target_copy),
        bind_original(reset_begin_original, &reset_begin), bind_original(reset_end_original, &reset_end),
    };
    static_assert(decltype(bindings){}.size() == RSF_ANOMALY_OBSERVER_HOOK_COUNT);
    return rsf_anomaly_observer_install(engine, bindings.data(), install_hooks);
}

bool read_frame(const rsf_anomaly_observer_engine* engine, rsf_anomaly_observer_frame* out) noexcept {
    return rsf_anomaly_observer_read_frame(engine, guarded_copy, out);
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
    [](const rsf_anomaly_observer_engine* engine) {
        const auto& build = build_profiles[engine->build];
        return sr::install(static_cast<std::byte*>(engine->image), build.image_size(), build);
    },
    sr::quiesce,
    [](const rsf_anomaly_observer_engine* engine) {
        const auto& build = build_profiles[engine->build];
        return motion::install(static_cast<std::byte*>(engine->image), build.image_size(), build);
    },
    motion::quiesce, input::enable, input::disable, input::idle, read_frame, guarded_copy,
    fg::render_begin, sr::frame_end, fg::end_before, fg::end_after,
    fg::camera, fg::ui_boundary, fg::scope_frame, fg::device_reset, log, report_stages,
};
}
}

extern "C" const rsf_anomaly_observer_platform* rsf_anomaly_observer_native() noexcept {
    return &rsf::anomaly::platform;
}
