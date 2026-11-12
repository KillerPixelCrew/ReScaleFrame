// SPDX-License-Identifier: GPL-3.0-only
#pragma once
#include <rescaleframe/game_api.h>
#include <cstdint>

namespace rsf::anomaly {
rsf_result prepare(const rsf_game_prepare_args*) noexcept;
rsf_result start(const rsf_game_start_args*) noexcept;
rsf_result quiesce(const rsf_game_control_args*) noexcept;
rsf_result stop(const rsf_game_control_args*) noexcept;
rsf_result status(rsf_game_renderer_status*) noexcept;
struct NativeObservation {
    uint32_t struct_size, abi_version;
    uint32_t active, width, height, generation;
    uint64_t render_frames, scene_passes, hud_passes, pda_copies, scope_copies, camera_updates, resets;
    uint32_t engine_frame, camera_finite;
    float view[16], projection[16];
};
inline constexpr uint32_t observation_abi = 1;
}
extern "C" __declspec(dllexport) rsf_result rsf_anomaly_observation(
    rsf::anomaly::NativeObservation*) noexcept;
