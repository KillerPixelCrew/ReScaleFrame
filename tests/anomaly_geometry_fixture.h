// SPDX-License-Identifier: MIT
#pragma once
#include "native_geometry/fixture_platform.h"

template<class F>
rsf_test_anomaly_geometry_scenario geometry_scenario(const F& run) noexcept
{
    return {&run, [](const void* context) noexcept { (*static_cast<const F*>(context))(); }};
}
