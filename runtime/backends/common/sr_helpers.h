// SPDX-License-Identifier: GPL-3.0-only
#pragma once
#include <rescaleframe/backend.h>
#include <rescaleframe/log.h>
#include <windows.h>
#include <cmath>
#include <cstdio>
#include <cwchar>
#include <d3d12.h>

namespace rsf {
// True when `object` (a queue, list or resource) was created by `device`. GetDevice adds a
// reference, so the owner is released again before comparing the pointers.
template<class T> bool owned_by(T* object, const void* device)
{
    ID3D12Device* owner = nullptr;
    if (!object || FAILED(object->GetDevice(IID_PPV_ARGS(&owner)))) return false;
    const bool same = owner == device; owner->Release();
    return same;
}
// Every element of a fixed array or container is a finite float.
template<class Range> bool all_finite(const Range& values)
{
    for (const auto value : values) if (!std::isfinite(value)) return false;
    return true;
}
// A UTF-8 runtime directory as wide text, accepted only when it is absolute (drive or UNC), so a
// library beside the game's executable cannot answer instead.
inline bool runtime_directory(const char* utf8, wchar_t (&directory)[1024])
{
    return utf8 && MultiByteToWideChar(CP_UTF8, MB_ERR_INVALID_CHARS, utf8, -1, directory, 1024) &&
        ((directory[0] && directory[1] == L':' && (directory[2] == L'\\' || directory[2] == L'/')) ||
         (directory[0] == L'\\' && directory[1] == L'\\'));
}
// LoadLibraryExW of an absolute path. Its own dependencies come from its folder and System32 only.
inline HMODULE load_module(const wchar_t* path, void (*log)(void*, const char*), void* user)
{
    HMODULE module = LoadLibraryExW(path, nullptr,
        LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_SYSTEM32);
    if (!module) say(log, user, "could not load %ls (Windows error %lu)", path, GetLastError());
    return module;
}
inline HMODULE load_runtime(const rsf_sr_open_desc& desc, const wchar_t* name)
{
    wchar_t directory[1024]{};
    wchar_t path[1200]{};
    if (!runtime_directory(desc.runtime_directory_utf8, directory) ||
        swprintf_s(path, L"%s\\%s", directory, name) < 0) {
        return nullptr;
    }
    return load_module(path, desc.log, desc.log_user);
}
template<class T> T entry(HMODULE module, const char* name)
{
    return reinterpret_cast<T>(reinterpret_cast<void*>(GetProcAddress(module, name)));
}
inline void probe_say(const rsf_backend_probe_desc* desc, const char* message)
{
    if (desc && desc->log) desc->log(desc->log_user, message);
}
// The shared opening of a vendor probe(): validate the descriptor, then reset `caps` to an empty
// answer that names the vendor. The caller fills in what it can do.
inline rsf_backend_result probe_begin(const rsf_backend_probe_desc* desc, rsf_backend_caps* caps,
                                      rsf_vendor vendor, const char* name)
{
    if (!desc || !caps || desc->struct_size < sizeof(rsf_backend_probe_desc) ||
        caps->struct_size < sizeof(rsf_backend_caps)) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    if (desc->abi_version != RSF_BACKEND_ABI_VERSION) return RSF_BACKEND_ERROR_ABI_MISMATCH;
    const uint32_t size = caps->struct_size;
    *caps = rsf_backend_caps{};
    caps->struct_size = size; caps->vendor = vendor; caps->name = name;
    return RSF_BACKEND_OK;
}
inline rsf_backend_result sr_release(void* pointer)
{
    // No vendor has an independent release operation. Closing requires GPU completion by the owner.
    return pointer ? RSF_BACKEND_OK : RSF_BACKEND_ERROR_INVALID_ARGUMENT;
}
// get_version for a session type with `version` and `name` members.
template<class Session> rsf_backend_result sr_version(void* pointer, uint64_t* id, const char** name)
{
    if (!pointer || !id || !name) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    auto* session = static_cast<Session*>(pointer);
    *id = session->version; *name = session->name;
    return RSF_BACKEND_OK;
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
        const auto desc = texture->GetDesc();
        const uint32_t width = i == 3 ? frame.record->output_width : i == 4 ? 1 : frame.record->render_width;
        const uint32_t height = i == 3 ? frame.record->output_height : i == 4 ? 1 : frame.record->render_height;
        if (!owned_by(texture, device) || desc.Dimension != D3D12_RESOURCE_DIMENSION_TEXTURE2D || desc.Width < width ||
            desc.Height < height || desc.DepthOrArraySize != 1 || desc.SampleDesc.Count != 1 ||
            resource.state != uint32_t(i == 3 ? D3D12_RESOURCE_STATE_UNORDERED_ACCESS : D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE))
            return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    }
    if (frame.color.resource == frame.output.resource) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    return RSF_BACKEND_OK;
}
}
