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

extern "C" {
const char* rsf_anomaly_observer_identify(rsf_anomaly_observer_engine*, const rsf::anomaly::ImageHeaders*,
    decltype(&rsf::anomaly::guarded_copy)) noexcept;
bool rsf_anomaly_observer_before(const rsf_anomaly_observer_platform*, uint32_t boundary) noexcept;
void rsf_anomaly_observer_after(const rsf_anomaly_observer_platform*, uint32_t boundary,
    const float* view, const float* projection, uint32_t target) noexcept;
void rsf_anomaly_observer_leave() noexcept;
using rsf_anomaly_observer_install_fn = bool (*)(void*, uint32_t, const rsf::anomaly::HookSite*, size_t) noexcept;
bool rsf_anomaly_observer_install(const rsf_anomaly_observer_engine*, const rsf_anomaly_observer_binding*,
    rsf_anomaly_observer_install_fn) noexcept;
bool rsf_anomaly_observer_read_frame(const rsf_anomaly_observer_engine*, decltype(&rsf::anomaly::guarded_copy),
    rsf_anomaly_observer_frame*) noexcept;
}

namespace rsf::anomaly {
namespace {
extern const rsf_anomaly_observer_platform platform;
HookSet hooks;

static_assert(sizeof(rsf_anomaly_observer_binding) == 16 && offsetof(rsf_anomaly_observer_binding, original) == 8);

// Native invocations and their trampolines stay on the C++ side. The scope also
// retires the Rust invocation when the native function unwinds through C++.
struct Invocation {
    const uint32_t boundary;
    const bool completes;
    explicit Invocation(uint32_t kind) noexcept
        : boundary(kind), completes(rsf_anomaly_observer_before(&platform, kind)) {}
    ~Invocation() { rsf_anomaly_observer_leave(); }
    void complete(const float* view = nullptr, const float* projection = nullptr, uint32_t target = 0) const noexcept {
        if (completes) rsf_anomaly_observer_after(&platform, boundary, view, projection, target);
    }
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
    const Invocation call{RSF_ANOMALY_OBSERVER_BEGIN};
    begin_original.get()(self);
    call.complete();
}
void render_end(void* self) {
    const Invocation call{RSF_ANOMALY_OBSERVER_END};
    end_original.get()(self);
    call.complete();
}
void camera(void* self, const float* view, const float* projection) {
    const Invocation call{RSF_ANOMALY_OBSERVER_CAMERA};
    camera_original.get()(self, view, projection);
    call.complete(view, projection);
}
void scene(void* self) {
    const Invocation call{RSF_ANOMALY_OBSERVER_SCENE};
    const geometry::RenderViewScope view;
    scene_original.get()(self);
}
void screen_ui(void* self) {
    const Invocation call{RSF_ANOMALY_OBSERVER_UI};
    ui_original.get()(self);
}
void target_copy(void* self, uint32_t target) {
    const Invocation call{RSF_ANOMALY_OBSERVER_TARGET_COPY};
    copy_original.get()(self, target);
    call.complete(nullptr, nullptr, target);
}
void reset_begin(void* self) {
    const Invocation call{RSF_ANOMALY_OBSERVER_RESET_BEGIN};
    reset_begin_original.get()(self);
}
void reset_end(void* self) {
    const Invocation call{RSF_ANOMALY_OBSERVER_RESET_END};
    reset_end_original.get()(self);
}

const char* identify(rsf_anomaly_observer_engine* out) noexcept {
    return rsf_anomaly_observer_identify(out, &guarded_image_headers, guarded_copy);
}

bool install_hooks(void* image, uint32_t image_size, const HookSite* sites, size_t count) noexcept {
    return hooks.install(static_cast<std::byte*>(image), image_size, {sites, count});
}

bool enable_hooks(const rsf_anomaly_observer_engine* engine) noexcept {
    if (hooks.created()) return hooks.enable();
    // Same order as native_sites in both exact-build records.
    const std::array bindings{
        bind_original(begin_original, &render_begin), bind_original(end_original, &render_end),
        bind_original(camera_original, &camera), bind_original(scene_original, &scene),
        bind_original(ui_original, &screen_ui), bind_original(copy_original, &target_copy),
        bind_original(reset_begin_original, &reset_begin), bind_original(reset_end_original, &reset_end),
    };
    static_assert(decltype(bindings){}.size() == native_site_count);
    return rsf_anomaly_observer_install(engine, bindings.data(), install_hooks);
}

bool read_frame(const rsf_anomaly_observer_engine* engine, rsf_anomaly_observer_frame* out) noexcept {
    return rsf_anomaly_observer_read_frame(engine, guarded_copy, out);
}

void log(rsf_game_log_fn emit, void* user, const char* text) noexcept {
    if (emit) try { emit(user, text); } catch (...) {}
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
    rsf_anomaly_fg_stage_render_begin, sr::frame_end, rsf_anomaly_fg_stage_end_before, rsf_anomaly_fg_stage_end_after,
    rsf_anomaly_fg_stage_camera, rsf_anomaly_fg_stage_ui_boundary, rsf_anomaly_fg_stage_scope, rsf_anomaly_fg_stage_device_reset, log,
};
}
}

extern "C" const rsf_anomaly_observer_platform* rsf_anomaly_observer_native() noexcept {
    return &rsf::anomaly::platform;
}
