// SPDX-License-Identifier: MIT
#pragma once
#include "native_observer_platform.h"

namespace rsf::anomaly {
rsf_result prepare(const rsf_game_prepare_args*) noexcept;
rsf_result start(const rsf_game_start_args*) noexcept;
rsf_result quiesce(const rsf_game_control_args*) noexcept;
rsf_result stop(const rsf_game_control_args*) noexcept;
rsf_result status(rsf_game_renderer_status*) noexcept;
using NativeObservation = rsf_anomaly_native_observation;
inline constexpr uint32_t observation_abi = RSF_ANOMALY_OBSERVATION_ABI;
}
extern "C" __declspec(dllexport) rsf_result rsf_anomaly_observation(
    rsf::anomaly::NativeObservation*) noexcept;
