// SPDX-License-Identifier: MIT
#pragma once
#include "bridge_platform.h"
#include <ffx_framegeneration.h>

namespace rsf::fsr4fg {
inline int32_t versions(PfnFfxQuery query, void* device, rsf_ffx_version* out, uint64_t& count) {
    if (!query || !device) return RSF_FSR4FG_INVALID_ARGUMENT;
    uint64_t ids[max_ffx_versions]{};
    const char* names[max_ffx_versions]{};
    const auto capacity = count;
    if (capacity > max_ffx_versions) return RSF_FSR4FG_INVALID_ARGUMENT;
    ffxQueryDescGetVersions versions{}; versions.header.type = FFX_API_QUERY_DESC_TYPE_GET_VERSIONS;
    versions.createDescType = FFX_API_CREATE_CONTEXT_DESC_TYPE_FRAMEGENERATION;
    versions.device = device; versions.outputCount = &count; versions.versionIds = ids; versions.versionNames = names;
    if (query(nullptr, &versions.header) || count > capacity) return RSF_FSR4FG_NOT_SUPPORTED;
    for (uint64_t i = 0; i < count; ++i) out[i] = ffx_version(ids[i], names[i]);
    return 0;
}
inline int32_t provider_version(PfnFfxQuery query, ffxContext* context, uint64_t& version) {
    ffxQueryGetProviderVersion actual{}; actual.header.type = FFX_API_QUERY_DESC_TYPE_GET_PROVIDER_VERSION;
    const auto result = query(context, &actual.header);
    if (!result) version = actual.versionId;
    return static_cast<int32_t>(result);
}
inline void prepare_constants(const rsf_fsr4fg_frame& frame, uint64_t sequence, bool reset,
        ffxDispatchDescFrameGenerationPrepareV2& out) {
    const auto& record = *frame.record; const auto& camera = record.camera;
    out.header.type = FFX_API_DISPATCH_DESC_TYPE_FRAMEGENERATION_PREPARE_V2;
    out.frameID = sequence; out.renderSize = {record.render_width, record.render_height};
    out.jitterOffset = {camera.jitter_pixels[0], camera.jitter_pixels[1]};
    out.motionVectorScale = {frame.motion_scale_x, frame.motion_scale_y};
    out.frameTimeDelta = record.frame_time_ms; out.cameraNear = camera.near_plane; out.cameraFar = camera.far_plane;
    out.cameraFovAngleVertical = camera.vertical_fov_radians; out.viewSpaceToMetersFactor = 1;
    out.reset = reset;
    // Godot's RH perspective looks down negative view Z. Follow the signed W/Z
    // projection term, as the existing D3D12 FSR preparation does.
    const float sign = camera.view_to_clip[11] < 0 ? -1.0f : 1.0f;
    for (uint32_t i = 0; i < 3; ++i) {
        out.cameraPosition[i] = camera.view_to_world[12 + i];
        out.cameraRight[i] = camera.view_to_world[i]; out.cameraUp[i] = camera.view_to_world[4 + i];
        out.cameraForward[i] = sign * camera.view_to_world[8 + i];
    }
}
}
