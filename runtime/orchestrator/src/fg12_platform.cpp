// SPDX-License-Identifier: MIT
#include "fg12_platform.h"

extern "C" void rsf_fg12_capture_attach();
extern "C" void rsf_fg12_capture_detach();

namespace {
// Preserve the original module lifetime for reading capture preferences and
// relinquishing capture tickets and resources at unload.
struct CaptureLifetime {
    CaptureLifetime() { rsf_fg12_capture_attach(); }
    ~CaptureLifetime() { rsf_fg12_capture_detach(); }
};
const CaptureLifetime capture_lifetime;
}

extern "C" int rsf_fg12_platform_install(const rsf_d3d11_present_setup* setup, int explicit_creation) try
{
    return explicit_creation ? rsf_d3d11_present_prepare(setup) : rsf_d3d11_present_install(setup);
} catch (...) { return 0; }
