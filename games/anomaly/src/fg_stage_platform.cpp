// SPDX-License-Identifier: MIT
#include "fg_stage.h"
#include "motion_stage.h"
#include "native_memory.h"
#include "sr_pass.h"
#include <rescaleframe/d3d11_state.h>
#include <cstddef>

extern "C" bool rsf_anomaly_sr_stage_owns_depth(uint32_t native_frame) noexcept;

namespace rsf::anomaly::fg {
namespace {
// The caller passes HW.pBaseZB from the exact executable profile the carrier selected.
void* engine_depth_view(uint32_t rva) noexcept {
    const auto* image = reinterpret_cast<const std::byte*>(GetModuleHandleW(nullptr));
    void* view = nullptr;
    if (!image || !rva || !guarded_copy(&view, image + rva, sizeof(view))) return nullptr;
    return view;
}

const rsf_anomaly_fg_platform& platform() noexcept {
    static const rsf_anomaly_fg_platform value{
        sizeof(rsf_anomaly_fg_platform),
        sr::engine_version, sr::project_id, sr::view_space_to_meters,
        &engine_depth_view,
        rsf_anomaly_sr_stage_owns_depth,
        [] {
            return motion::frame_target();
        },
        {rsf_d3d11_state_save, rsf_d3d11_state_restore},
    };
    return value;
}
}

bool configure_ini(const wchar_t* ini, bool apply, HMODULE runtime, const BuildProfile& build,
    rsf_game_log_fn log) noexcept {
    return rsf_anomaly_fg_configure_ini(ini, apply, runtime, build.sr->hw_base_zb_rva, log, &platform());
}

bool installed() noexcept { return rsf_anomaly_fg_installed(); }
void set_overlay_sink(rsf_anomaly_fg_overlay_present sink) noexcept {
    rsf_anomaly_fg_set_overlay_sink(sink, nullptr);
}
void camera(const float* view, const float* projection) noexcept { rsf_anomaly_fg_stage_camera(view, projection); }
void input_begin(uint32_t frame) noexcept { rsf_anomaly_fg_stage_input_begin(frame); }
void input_received(uint32_t kinds, uint32_t message) noexcept { rsf_anomaly_fg_stage_input_received(kinds, message); }
void render_begin(uint32_t frame, uint32_t width, uint32_t height) noexcept {
    rsf_anomaly_fg_stage_render_begin(frame, width, height);
}
void scope_frame() noexcept { rsf_anomaly_fg_stage_scope(); }
void ui_boundary() noexcept { rsf_anomaly_fg_stage_ui_boundary(); }
void end_before() noexcept { rsf_anomaly_fg_stage_end_before(); }
void end_after() noexcept { rsf_anomaly_fg_stage_end_after(); }
void device_reset() noexcept { rsf_anomaly_fg_stage_device_reset(); }
void report(void (*emit)(void*, const char*), void* user) noexcept { rsf_anomaly_fg_stage_report(emit, user); }
}
