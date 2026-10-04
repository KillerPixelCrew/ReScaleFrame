// SPDX-License-Identifier: GPL-3.0-only
#pragma once
#include <rescaleframe/backend.h>
#include <windows.h>
#include <cmath>
#include <cstdio>
#include <cwchar>
#include <d3d12.h>

namespace rsf {
inline HMODULE load_runtime(const rsf_sr_open_desc& desc, const wchar_t* name)
{
    wchar_t directory[1024]{};
    wchar_t path[1200]{};
    if (!desc.runtime_directory_utf8 ||
        !MultiByteToWideChar(CP_UTF8, MB_ERR_INVALID_CHARS, desc.runtime_directory_utf8, -1,
                            directory, 1024) ||
        !((directory[0] && directory[1] == L':' && (directory[2] == L'\\' || directory[2] == L'/')) ||
          (directory[0] == L'\\' && directory[1] == L'\\'))) {
        return nullptr;
    }
    if (swprintf_s(path, L"%s\\%s", directory, name) < 0) {
        return nullptr;
    }
    HMODULE module = LoadLibraryExW(path, nullptr,
        LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_SYSTEM32);
    if (!module && desc.log) {
        char message[1400]{};
        std::snprintf(message, sizeof(message), "SR: could not load %s (Windows error %lu)",
                      desc.runtime_directory_utf8, GetLastError());
        desc.log(desc.log_user, message);
    }
    return module;
}
template<class T> T entry(HMODULE module, const char* name)
{
    return reinterpret_cast<T>(reinterpret_cast<void*>(GetProcAddress(module, name)));
}
inline rsf_backend_result validate_open(const rsf_sr_open_desc* desc, void** out)
{
    if (!desc || !out || desc->struct_size < sizeof(*desc) || !desc->device ||
        !desc->output_width || !desc->output_height || desc->quality > RSF_QUALITY_ULTRA_QUALITY) {
        return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    }
    *out = nullptr;
    if (desc->abi_version != RSF_BACKEND_ABI_VERSION) {
        return RSF_BACKEND_ERROR_ABI_MISMATCH;
    }
    return desc->api == RSF_API_D3D12 ? RSF_BACKEND_OK : RSF_BACKEND_ERROR_WRONG_API;
}
inline rsf_backend_result validate_frame(const rsf_sr_frame* frame)
{
    if (!frame || frame->struct_size < sizeof(*frame) || !frame->record ||
        frame->record->struct_size < sizeof(rsf_frame_record)) {
        return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    }
    const auto& record = *frame->record;
    if (record.abi_version != RSF_GAME_FRAME_ABI_VERSION ||
        record.camera.abi_version != RSF_GAME_FRAME_ABI_VERSION) {
        return RSF_BACKEND_ERROR_ABI_MISMATCH;
    }
    if (record.camera.struct_size < sizeof(rsf_camera_frame) || !record.frame_id ||
        !record.render_width || !record.render_height || !record.output_width || !record.output_height ||
        record.render_width > record.output_width || record.render_height > record.output_height ||
        !std::isfinite(frame->jitter_x) || !std::isfinite(frame->jitter_y) ||
        !std::isfinite(frame->motion_scale_x) || !std::isfinite(frame->motion_scale_y) ||
        !frame->motion_scale_x || !frame->motion_scale_y ||
        !std::isfinite(frame->pre_exposure) || frame->pre_exposure <= 0 ||
        !std::isfinite(frame->view_space_to_meters) || frame->view_space_to_meters <= 0 ||
        !std::isfinite(record.frame_time_ms) || record.frame_time_ms <= 0 ||
        !std::isfinite(record.camera.near_plane) || record.camera.near_plane <= 0 ||
        !std::isfinite(record.camera.far_plane) || record.camera.far_plane < 0 ||
        !std::isfinite(record.camera.vertical_fov_radians) ||
        record.camera.vertical_fov_radians <= 0 || record.camera.vertical_fov_radians >= 3.141593f) {
        return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    }
    if (record.flags & RSF_FRAME_FLAG_NO_SR) return RSF_BACKEND_ERROR_NOT_READY;
    const rsf_backend_resource* resources[] = {&frame->color, &frame->depth, &frame->motion,
                                               &frame->output};
    for (auto* resource : resources) {
        const bool output = resource == &frame->output;
        if (resource->struct_size < sizeof(*resource) || !resource->resource ||
            resource->lifetime != RSF_LIFETIME_ONLY_NOW || resource->x || resource->y ||
            resource->width < (output ? record.output_width : record.render_width) ||
            resource->height < (output ? record.output_height : record.render_height)) {
            return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
        }
        if (resource->generation != record.resource_generation) {
            return RSF_BACKEND_ERROR_STALE_RESOURCES;
        }
    }
    if (frame->exposure.resource && (frame->exposure.struct_size < sizeof(rsf_backend_resource) ||
        frame->exposure.lifetime != RSF_LIFETIME_ONLY_NOW || frame->exposure.x || frame->exposure.y))
        return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    if (frame->exposure.resource && frame->exposure.generation != record.resource_generation)
        return RSF_BACKEND_ERROR_STALE_RESOURCES;
    return RSF_BACKEND_OK;
}
inline rsf_backend_result validate_d3d12_resources(const rsf_sr_frame& frame, void* device)
{
    const rsf_backend_resource* resources[] = {&frame.color, &frame.depth, &frame.motion,
                                               &frame.output, &frame.exposure, &frame.reactive,
                                               &frame.transparency};
    for (uint32_t i = 0; i < 7; ++i) {
        const auto& resource = *resources[i];
        if (!resource.resource) continue;
        auto* texture = static_cast<ID3D12Resource*>(resource.resource);
        ID3D12Device* owner = nullptr;
        if (FAILED(texture->GetDevice(IID_PPV_ARGS(&owner)))) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
        const bool same = owner == device; owner->Release();
        const auto desc = texture->GetDesc();
        const uint32_t width = i == 3 ? frame.record->output_width : i == 4 ? 1 : frame.record->render_width;
        const uint32_t height = i == 3 ? frame.record->output_height : i == 4 ? 1 : frame.record->render_height;
        if (!same || desc.Dimension != D3D12_RESOURCE_DIMENSION_TEXTURE2D || desc.Width < width ||
            desc.Height < height || desc.DepthOrArraySize != 1 || desc.SampleDesc.Count != 1 ||
            resource.state != uint32_t(i == 3 ? D3D12_RESOURCE_STATE_UNORDERED_ACCESS : D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE))
            return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    }
    if (frame.color.resource == frame.output.resource) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    return RSF_BACKEND_OK;
}
}
