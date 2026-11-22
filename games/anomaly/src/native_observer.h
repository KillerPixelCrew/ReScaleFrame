// SPDX-License-Identifier: MIT
#pragma once
#include "native_observer_platform.h"

namespace rsf::anomaly {
using NativeObservation = rsf_anomaly_native_observation;
inline constexpr uint32_t observation_abi = RSF_ANOMALY_OBSERVATION_ABI;
}
extern "C" rsf_result rsf_anomaly_observation(
    rsf::anomaly::NativeObservation*) noexcept;
