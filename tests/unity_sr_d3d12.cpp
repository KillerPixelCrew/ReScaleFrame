// SPDX-License-Identifier: MIT
// The Streamline SDK headers are C++ only, so this leaf makes the one SDK call the Rust fixture checks.
#include "unity_sr_d3d12_abi.h"
#include <windows.h>
#include <cstring>
#if defined(RSF_HAVE_STREAMLINE) && RSF_HAVE_STREAMLINE
#include <sl.h>
#endif

extern "C" uint32_t rsf_fixture_unity_shared_constants(
    rsf_streamline_host* shared, const rsf_game_render_pass* pass)
{
#if defined(RSF_HAVE_STREAMLINE) && RSF_HAVE_STREAMLINE
    auto set_constants = reinterpret_cast<PFun_slSetConstants*>(reinterpret_cast<void*>(
        GetProcAddress(static_cast<HMODULE>(rsf_streamline_host_module(shared)), "slSetConstants")));
    auto* token = static_cast<sl::FrameToken*>(rsf_streamline_host_token(shared, 1));
    sl::Constants constants{};
    std::memcpy(&constants.cameraViewToClip, pass->camera.view_to_clip, 64);
    std::memcpy(&constants.clipToCameraView, pass->camera.clip_to_view, 64);
    std::memcpy(&constants.clipToPrevClip, pass->camera.clip_to_previous_clip, 64);
    std::memcpy(&constants.prevClipToClip, pass->previous_clip_to_clip, 64);
    constants.cameraNear = 0.2f;
    constants.cameraFar = 1000;
    constants.cameraFOV = pass->camera.vertical_fov_radians;
    constants.cameraAspectRatio = 640.0f / 360.0f;
    constants.depthInverted = sl::eTrue;
    constants.reset = sl::eTrue;
    return set_constants && token
        && set_constants(constants, *token, sl::ViewportHandle(0u)) == sl::Result::eOk;
#else
    (void)shared;
    (void)pass;
    return 2;
#endif
}
