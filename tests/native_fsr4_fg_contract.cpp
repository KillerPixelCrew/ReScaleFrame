// SPDX-License-Identifier: GPL-3.0-only
#include "../runtime/vulkan/fsr4-fg/bridge_contract.h"
#include <cstdio>
#include <cstring>

namespace {
int failures = 0, mode = 0;
void* expected_device = reinterpret_cast<void*>(123);
#define CHECK(value) do { if (!(value)) { std::printf("line %d: %s\n", __LINE__, #value); ++failures; } } while (false)
ffxReturnCode_t query(ffxContext* context, ffxQueryDescHeader* header) {
    if (header->type == FFX_API_QUERY_DESC_TYPE_GET_PROVIDER_VERSION) {
        CHECK(context != nullptr);
        reinterpret_cast<ffxQueryGetProviderVersion*>(header)->versionId = mode == 4 ? 3 : 42;
        return mode == 5 ? FFX_API_RETURN_ERROR : FFX_API_RETURN_OK;
    }
    CHECK(!context && header->type == FFX_API_QUERY_DESC_TYPE_GET_VERSIONS);
    auto& desc = *reinterpret_cast<ffxQueryDescGetVersions*>(header);
    CHECK(desc.device == expected_device && desc.createDescType == FFX_API_CREATE_CONTEXT_DESC_TYPE_FRAMEGENERATION);
    if (mode == 2) return FFX_API_RETURN_ERROR;
    if (mode == 3) { *desc.outputCount = 33; return FFX_API_RETURN_OK; }
    desc.versionIds[0] = 3; desc.versionNames[0] = "3.1.6";
    if (mode == 0) { *desc.outputCount = 1; return FFX_API_RETURN_OK; }
    desc.versionIds[1] = 41; desc.versionNames[1] = "4.0.0";
    desc.versionIds[2] = 42; desc.versionNames[2] = "4.0.1";
    desc.versionIds[3] = 50; desc.versionNames[3] = "5.0.0";
    *desc.outputCount = 4; return FFX_API_RETURN_OK;
}
}
int main() {
    uint64_t selected = 99;
    CHECK(rsf::fsr4fg::discover(query, expected_device, selected) == RSF_FSR4FG_NOT_SUPPORTED && selected == 0);
    mode = 1;
    CHECK(rsf::fsr4fg::discover(query, expected_device, selected) == 0 && selected == 42);
    ffxContext context = reinterpret_cast<void*>(1);
    CHECK(rsf::fsr4fg::verify(query, &context, selected));
    mode = 4; CHECK(!rsf::fsr4fg::verify(query, &context, selected));
    mode = 5; CHECK(!rsf::fsr4fg::verify(query, &context, selected));
    mode = 2; CHECK(rsf::fsr4fg::discover(query, expected_device, selected) == RSF_FSR4FG_NOT_SUPPORTED && !selected);
    mode = 3; CHECK(rsf::fsr4fg::discover(query, expected_device, selected) == RSF_FSR4FG_NOT_SUPPORTED && !selected);
    CHECK(rsf::fsr4fg::discover(nullptr, expected_device, selected) == RSF_FSR4FG_INVALID_ARGUMENT);
    CHECK(rsf::fsr4fg::discover(query, nullptr, selected) == RSF_FSR4FG_INVALID_ARGUMENT);
    rsf_frame_record record{}; record.frame_id = 12; record.render_width = 426; record.render_height = 240;
    record.frame_time_ms = 16.5f;
    auto& camera = record.camera;
    for (uint32_t i = 0; i < 16; i += 5) camera.view_to_world[i] = 1;
    camera.view_to_world[12] = 2; camera.view_to_world[13] = 3; camera.view_to_world[14] = 4;
    camera.view_to_clip[11] = -1;
    camera.near_plane = 0.05f; camera.far_plane = 4000; camera.vertical_fov_radians = 1.3f;
    camera.jitter_pixels[0] = 0.25f; camera.jitter_pixels[1] = -0.125f;
    rsf_fsr4fg_frame frame{}; frame.record = &record; frame.motion_scale_x = 426; frame.motion_scale_y = 240;
    ffxDispatchDescFrameGenerationPrepareV2 prepare{};
    rsf::fsr4fg::prepare_constants(frame, 70, true, 11, prepare);
    CHECK(prepare.header.type == FFX_API_DISPATCH_DESC_TYPE_FRAMEGENERATION_PREPARE_V2 && prepare.frameID == 70 && !prepare.reset);
    CHECK(prepare.cameraPosition[0] == 2 && prepare.cameraPosition[1] == 3 && prepare.cameraPosition[2] == 4);
    CHECK(prepare.cameraRight[0] == 1 && prepare.cameraUp[1] == 1 && prepare.cameraForward[2] == -1);
    CHECK(prepare.frameTimeDelta == 16.5f && prepare.motionVectorScale.x == 426 && prepare.motionVectorScale.y == 240);
    CHECK(prepare.jitterOffset.x == 0.25f && prepare.jitterOffset.y == -0.125f && prepare.viewSpaceToMetersFactor == 1);
    CHECK(prepare.cameraNear == 0.05f && prepare.cameraFar == 4000 && prepare.cameraFovAngleVertical == 1.3f);
    rsf::fsr4fg::prepare_constants(frame, 71, false, 11, prepare); CHECK(prepare.reset);
    rsf::fsr4fg::prepare_constants(frame, 72, true, 9, prepare); CHECK(prepare.reset);
    record.flags = RSF_FRAME_FLAG_RESET;
    rsf::fsr4fg::prepare_constants(frame, 73, true, 11, prepare); CHECK(prepare.reset);
    record.flags = 0; camera.view_to_clip[11] = 1;
    rsf::fsr4fg::prepare_constants(frame, 74, true, 11, prepare); CHECK(!prepare.reset && prepare.cameraForward[2] == 1);
    CHECK(rsf_fsr4fg_check(sizeof(frame) - 1, 99, sizeof(frame)) == RSF_FSR4FG_INVALID_ARGUMENT);
    CHECK(rsf_fsr4fg_check(sizeof(frame), 99, sizeof(frame)) == RSF_FSR4FG_ABI_MISMATCH);
    if (!failures) std::puts("FSR4 FG exact-provider and PrepareV2 constants passed (synthetic, no GPU).");
    return failures ? 1 : 0;
}
