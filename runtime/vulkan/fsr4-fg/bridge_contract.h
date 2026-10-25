// SPDX-License-Identifier: GPL-3.0-only
#pragma once
#include "bridge_api.h"
#include <ffx_framegeneration.h>
#include "../../backends/fsr/src/ffx_version.h"

namespace rsf::fsr4fg {
inline int32_t discover(PfnFfxQuery query, void* device, uint64_t& selected) {
    selected = 0;
    if (!query || !device) return RSF_FSR4FG_INVALID_ARGUMENT;
    uint64_t count = 32, ids[32]{}; const char* names[32]{};
    ffxQueryDescGetVersions versions{}; versions.header.type = FFX_API_QUERY_DESC_TYPE_GET_VERSIONS;
    versions.createDescType = FFX_API_CREATE_CONTEXT_DESC_TYPE_FRAMEGENERATION;
    versions.device = device; versions.outputCount = &count; versions.versionIds = ids; versions.versionNames = names;
    char name[128]{};
    if (query(nullptr, &versions.header) || count > 32 ||
        !select_ffx_version(ids, names, count, 4, 0, selected, name, sizeof(name)) || !selected) return RSF_FSR4FG_NOT_SUPPORTED;
    return 0;
}
inline bool verify(PfnFfxQuery query, ffxContext* context, uint64_t expected) {
    ffxQueryGetProviderVersion actual{}; actual.header.type = FFX_API_QUERY_DESC_TYPE_GET_PROVIDER_VERSION;
    return query && context && expected && !query(context, &actual.header) && actual.versionId == expected;
}
inline void prepare_constants(const rsf_fsr4fg_frame& frame, uint64_t sequence, bool history,
        uint64_t last_source, ffxDispatchDescFrameGenerationPrepareV2& out) {
    const auto& record = *frame.record; const auto& camera = record.camera;
    out.header.type = FFX_API_DISPATCH_DESC_TYPE_FRAMEGENERATION_PREPARE_V2;
    out.frameID = sequence; out.renderSize = {record.render_width, record.render_height};
    out.jitterOffset = {camera.jitter_pixels[0], camera.jitter_pixels[1]};
    out.motionVectorScale = {frame.motion_scale_x, frame.motion_scale_y};
    out.frameTimeDelta = record.frame_time_ms; out.cameraNear = camera.near_plane; out.cameraFar = camera.far_plane;
    out.cameraFovAngleVertical = camera.vertical_fov_radians; out.viewSpaceToMetersFactor = 1;
    out.reset = !history || record.frame_id != last_source + 1 || (record.flags & RSF_FRAME_FLAG_RESET);
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
