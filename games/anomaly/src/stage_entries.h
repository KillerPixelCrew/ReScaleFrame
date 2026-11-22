// SPDX-License-Identifier: MIT
#pragma once
// C entries of the native SR, motion and geometry stage owners, called by the bootstrap and observer leaves.
// `build` is the exact build index the Rust profile owner admitted (native_profile.rs order).
#include <rescaleframe/game_api.h>
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
#define RSF_STAGE_NOEXCEPT noexcept
extern "C" {
#else
#define RSF_STAGE_NOEXCEPT
#endif

// sr_stage_platform.c.
bool rsf_anomaly_sr_configure_native(const wchar_t* ini, bool apply, rsf_game_log_fn) RSF_STAGE_NOEXCEPT;
bool rsf_anomaly_sr_install_native(void* image, size_t build) RSF_STAGE_NOEXCEPT;
// sr_stage.rs. HW.pBaseZB of the build (sr_profile.rs), or 0 for an unknown index.
uint32_t rsf_anomaly_sr_depth_rva(size_t build) RSF_STAGE_NOEXCEPT;
bool rsf_anomaly_sr_stage_quiesce(void) RSF_STAGE_NOEXCEPT;
void rsf_anomaly_sr_stage_frame_end(void) RSF_STAGE_NOEXCEPT;

// motion_stage.cpp.
void rsf_anomaly_motion_configure_native(const wchar_t* ini, rsf_game_log_fn) RSF_STAGE_NOEXCEPT;
bool rsf_anomaly_motion_install_native(void* image, size_t build) RSF_STAGE_NOEXCEPT;
// motion/stage.rs.
bool rsf_anomaly_motion_stage_quiesce(void) RSF_STAGE_NOEXCEPT;

// native_geometry_platform.c (declared with its other entries in native_geometry.h).
bool rsf_anomaly_geometry_install_native(void* image, size_t build) RSF_STAGE_NOEXCEPT;
bool rsf_anomaly_geometry_disable_native(void) RSF_STAGE_NOEXCEPT;
// Calls `original(self)` inside the render-view scope of the CRender::Render detour. The scope is left on return and
// during a C++ unwind, which then continues to the caller.
void rsf_anomaly_geometry_render_scene(void (*original)(void*), void* self);

#ifdef __cplusplus
}
#endif
#undef RSF_STAGE_NOEXCEPT
