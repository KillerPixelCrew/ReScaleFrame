/* SPDX-License-Identifier: MIT */
#include <rescaleframe/dlss.h>
#include <rescaleframe/texture_dump.h>

/* The public cast macros are not integral macro values in bindgen. */
enum {
    RSF_PROBE_DLSS_ENGINE_UNREAL = RSF_DLSS_ENGINE_UNREAL,
    RSF_PROBE_DLSS_QUALITY_PERFORMANCE = RSF_DLSS_QUALITY_PERFORMANCE,
};
