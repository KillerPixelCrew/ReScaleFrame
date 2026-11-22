// SPDX-License-Identifier: MIT
#include <rescaleframe/native_composition.h>
#include <rescaleframe/native_translucency.h>
#include "native_regions.h"
#include "native_reinsert.h"

extern "C" void rsf_core_native_exposure_release();

namespace {
// Covers DLL and embedded carrier unload if explicit graphics teardown was skipped.
struct ReleaseAtUnload {
    ~ReleaseAtUnload()
    {
        rsf_native_composition_release();
        rsf_native_reinsert_release();
        rsf_core_native_exposure_release();
        rsf_native_regions_release();
        rsf_native_translucency_release();
    }
} release_at_unload;
}
