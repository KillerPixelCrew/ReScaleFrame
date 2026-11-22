// SPDX-License-Identifier: MIT
#pragma once
#include "native_observer_platform.h"

#ifdef __cplusplus
namespace rsf::anomaly {
using NativeObservation = rsf_anomaly_native_observation;
inline constexpr uint32_t observation_abi = RSF_ANOMALY_OBSERVATION_ABI;
}
extern "C"
#endif
rsf_result rsf_anomaly_observation(rsf_anomaly_native_observation*)
#ifdef __cplusplus
    noexcept
#endif
    ;
