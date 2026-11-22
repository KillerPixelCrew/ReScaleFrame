// SPDX-License-Identifier: MIT
#pragma once
#include <rescaleframe/overlay.h>
#include <rescaleframe/sharpening_settings.h>

#ifdef __cplusplus
extern "C" {
#endif
void rsf_overlay_fill_sharpening_stats(rsf_overlay_stats*, const rsf_sharpening_options*);
rsf_sharpening_options rsf_overlay_requested_sharpening(const rsf_overlay_intent*);
#ifdef __cplusplus
}
#endif
