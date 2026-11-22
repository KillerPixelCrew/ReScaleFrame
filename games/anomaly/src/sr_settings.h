// SPDX-License-Identifier: MIT
#pragma once
// The shared render-frontend SR settings (INI keys SR, SRBackend, SRQuality, DLSSPreset) as the Anomaly SR stages apply
// them. The stage contract supplies the adapter's capabilities without the Runtime headers.
#include "sr_stage_platform.h"
#include <cstdint>

namespace rsf::anomaly::sr {
inline constexpr uint32_t backend_dlss = 1, backend_fsr3 = 3, backend_fsr4 = 4, backend_xess = 5, backend_auto = 7;   // RSF_SR_*
using SrSettings = rsf_anomaly_sr_settings;
inline constexpr SrSettings default_sr_settings{true, backend_auto, 1, 0};
}
