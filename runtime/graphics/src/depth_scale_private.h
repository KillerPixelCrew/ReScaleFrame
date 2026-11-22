// SPDX-License-Identifier: MIT
#pragma once
#include <rescaleframe/depth_scale.h>
// Internal allocation-only readiness; same primitive used by public prepare.
extern "C" uint32_t rsf_depth_scale_reserve(rsf_depth_scale*, void* texture);
