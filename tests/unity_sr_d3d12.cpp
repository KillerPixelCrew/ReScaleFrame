// SPDX-License-Identifier: MIT
// Vendor C++ and DirectXMath calls used by the Rust D3D12 fixture.
#include "unity_sr_d3d12_abi.h"
#include <windows.h>
#include <DirectXMath.h>
#include <DirectXPackedVector.h>
#include <cstring>
#if defined(RSF_HAVE_STREAMLINE) && RSF_HAVE_STREAMLINE
#include <sl.h>
#endif

extern "C" int rsf_fixture_unity_dlss_preset_valid(uint32_t preset)
{
    return rsf_dlss_preset_valid(preset);
}

extern "C" void rsf_fixture_unity_camera(rsf_game_render_pass* pass)
{
    DirectX::XMFLOAT4X4 identity{}, projection{}, inverse{};
    DirectX::XMStoreFloat4x4(&identity, DirectX::XMMatrixIdentity());
    const auto aspect = static_cast<float>(pass->camera.output_width)
        / static_cast<float>(pass->camera.output_height);
    const auto proj = DirectX::XMMatrixPerspectiveFovRH(
        pass->camera.vertical_fov_radians, aspect, pass->camera.far_plane, pass->camera.near_plane);
    DirectX::XMStoreFloat4x4(&projection, proj);
    DirectX::XMStoreFloat4x4(&inverse, DirectX::XMMatrixInverse(nullptr, proj));
    std::memcpy(pass->camera.view_to_clip, &projection, sizeof(projection));
    std::memcpy(pass->camera.clip_to_view, &inverse, sizeof(inverse));
    std::memcpy(pass->camera.view_to_world, &identity, sizeof(identity));
    std::memcpy(pass->camera.world_to_view, &identity, sizeof(identity));
    std::memcpy(pass->camera.clip_to_previous_clip, &identity, sizeof(identity));
    std::memcpy(pass->previous_clip_to_clip, &identity, sizeof(identity));
}

extern "C" float rsf_fixture_unity_half_to_float(uint16_t half)
{
    return DirectX::PackedVector::XMConvertHalfToFloat(half);
}

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
