// SPDX-License-Identifier: GPL-3.0-only
#pragma once
#include <rescaleframe/frame_generation.h>
#include "sr_helpers.h"
#include <dxgi1_4.h>
namespace rsf {
inline rsf_backend_result fg_setup_header(const rsf_generation_setup* setup, void** out, void** chain)
{
    if (out) *out = nullptr;
    if (chain) *chain = nullptr;
    if (!setup || !out || !chain || setup->struct_size < sizeof(*setup) ||
        setup->chain.struct_size < sizeof(rsf_fg_swapchain_desc)) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    if (setup->abi_version != RSF_FG_ABI_VERSION || setup->chain.abi_version != RSF_BACKEND_ABI_VERSION)
        return RSF_BACKEND_ERROR_ABI_MISMATCH;
    return RSF_BACKEND_OK;
}
inline rsf_backend_result fg_setup(const rsf_generation_setup* setup, void** out, void** chain)
{
    const auto result = fg_setup_header(setup, out, chain); if (result != 0) return result;
    if (!setup->chain.d3d12_device || !setup->chain.d3d12_queue || !setup->chain.width ||
        !setup->chain.height || setup->chain.buffer_count < 2 || !setup->runtime_directory_utf8 ||
        !setup->chain.max_generated_frames || !std::isfinite(setup->view_space_to_meters) ||
        setup->view_space_to_meters <= 0 || setup->chain.width > INT32_MAX || setup->chain.height > INT32_MAX ||
        (setup->chain.ui_mode != RSF_UI_MODE_NONE && setup->chain.ui_mode != RSF_UI_MODE_UI_LAYER &&
         setup->chain.ui_mode != RSF_UI_MODE_BACKBUFFER_HUDLESS && setup->chain.ui_mode != RSF_UI_MODE_BACKBUFFER_HUDLESS_UI))
        return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    auto* queue = static_cast<ID3D12CommandQueue*>(setup->chain.d3d12_queue);
    return queue->GetDesc().Type == D3D12_COMMAND_LIST_TYPE_DIRECT && owned_by(queue, setup->chain.d3d12_device) ?
        RSF_BACKEND_OK : RSF_BACKEND_ERROR_INVALID_ARGUMENT;
}
inline HMODULE fg_library(const rsf_generation_setup& setup, const wchar_t* name)
{
    rsf_sr_open_desc desc{};
    desc.runtime_directory_utf8 = setup.runtime_directory_utf8;
    desc.log = setup.chain.log; desc.log_user = setup.chain.log_user;
    return load_runtime(desc, name);
}
inline bool fg_options(const rsf_fg_options* options)
{
    return options && options->struct_size >= sizeof(*options) && options->abi_version == RSF_FG_ABI_VERSION &&
        options->mode <= RSF_FG_DYNAMIC && options->reflex_mode <= RSF_REFLEX_BOOST &&
        std::isfinite(options->dynamic_target_fps) && options->dynamic_target_fps >= 0;
}
inline rsf_backend_result fg_options_result(const rsf_fg_options* options)
{
    if (!options || options->struct_size < sizeof(*options)) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    if (options->abi_version != RSF_FG_ABI_VERSION) return RSF_BACKEND_ERROR_ABI_MISMATCH;
    return fg_options(options) ? RSF_BACKEND_OK : RSF_BACKEND_ERROR_INVALID_ARGUMENT;
}
inline DXGI_SWAP_CHAIN_DESC1 chain_description(const rsf_fg_swapchain_desc& desc)
{
    DXGI_SWAP_CHAIN_DESC1 chain{};
    chain.Width = desc.width; chain.Height = desc.height; chain.Format = static_cast<DXGI_FORMAT>(desc.format);
    chain.SampleDesc.Count = 1; chain.BufferUsage = DXGI_USAGE_RENDER_TARGET_OUTPUT;
    chain.BufferCount = desc.buffer_count; chain.SwapEffect = DXGI_SWAP_EFFECT_FLIP_DISCARD;
    chain.Flags = desc.allow_tearing ? DXGI_SWAP_CHAIN_FLAG_ALLOW_TEARING : 0;
    return chain;
}
inline rsf_backend_result fg_command(void* command, void* device)
{
    if (!command) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    auto* list = static_cast<ID3D12GraphicsCommandList*>(command);
    return list->GetType() == D3D12_COMMAND_LIST_TYPE_DIRECT && owned_by(list, device) ?
        RSF_BACKEND_OK : RSF_BACKEND_ERROR_INVALID_ARGUMENT;
}
inline rsf_backend_result fg_frame(const rsf_fg_frame* frame, void* device)
{
    if (!frame || frame->struct_size < sizeof(*frame) || !frame->record ||
        frame->record->struct_size < sizeof(rsf_frame_record)) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    const auto& record = *frame->record;
    if (record.abi_version != RSF_GAME_FRAME_ABI_VERSION || record.camera.abi_version != RSF_GAME_FRAME_ABI_VERSION)
        return RSF_BACKEND_ERROR_ABI_MISMATCH;
    if (!record.frame_id || record.camera.struct_size < sizeof(rsf_camera_frame) ||
        !record.output_width || !record.output_height || !record.render_width || !record.render_height ||
        record.output_width > INT32_MAX || record.output_height > INT32_MAX)
        return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    if (!frame->interpolate) return RSF_BACKEND_OK;
    if (!record.session_id || !record.resource_generation || !record.input_qpc || record.phase != RSF_PHASE_UI_COMPLETE)
        return RSF_BACKEND_ERROR_NOT_READY;
    if (!rsf_frame_allows_fg(&record) || record.flags & (RSF_FRAME_FLAG_RESET | RSF_FRAME_FLAG_AMBIGUOUS_ID))
        return RSF_BACKEND_ERROR_NOT_READY;
    if (!std::isfinite(frame->motion_scale_x) || !std::isfinite(frame->motion_scale_y) ||
        !frame->motion_scale_x || !frame->motion_scale_y || !std::isfinite(record.frame_time_ms) ||
        record.frame_time_ms <= 0 || !std::isfinite(record.camera.near_plane) || record.camera.near_plane <= 0 ||
        !std::isfinite(record.camera.vertical_fov_radians) || record.camera.vertical_fov_radians <= 0 ||
        record.camera.vertical_fov_radians >= 3.141593f || !std::isfinite(record.camera.far_plane) || record.camera.far_plane < 0)
        return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    if (!all_finite(record.camera.view_to_clip) || !all_finite(record.camera.clip_to_previous_clip) ||
        !all_finite(record.camera.view_to_world) || !all_finite(record.camera.world_to_view) ||
        !all_finite(record.camera.jitter_pixels) || !all_finite(record.camera.clip_to_view))
        return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    const rsf_backend_resource* resources[] = {&frame->backbuffer, &frame->depth, &frame->motion, &frame->hudless, &frame->ui};
    for (uint32_t i = 0; i < 5; ++i) {
        const auto& resource = *resources[i];
        if (i > 2 && !resource.resource) continue;
        if (resource.struct_size < sizeof(resource) || !resource.resource) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
        if (resource.generation != record.resource_generation) return RSF_BACKEND_ERROR_STALE_RESOURCES;
        auto* texture = static_cast<ID3D12Resource*>(resource.resource);
        const auto desc = texture->GetDesc();
        if (!owned_by(texture, device) || desc.Dimension != D3D12_RESOURCE_DIMENSION_TEXTURE2D || desc.SampleDesc.Count != 1 ||
            resource.x > desc.Width || resource.width > desc.Width - resource.x ||
            resource.y > desc.Height || resource.height > desc.Height - resource.y ||
            !resource.width || !resource.height) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
        const bool display = i == 0 || i > 2;
        if (display && (resource.width != record.output_width || resource.height != record.output_height))
            return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
        if (i == 1 && (resource.width != record.render_width || resource.height != record.render_height))
            return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    }
    return RSF_BACKEND_OK;
}
// provider.status for a session type whose `state` member is the rsf_fg_status to report.
template<class Session> rsf_backend_result session_status(void* pointer, rsf_fg_status* out)
{
    if (!pointer || !out || out->struct_size < sizeof(*out)) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    *out = static_cast<Session*>(pointer)->state; return RSF_BACKEND_OK;
}
// The provider a vendor backend exposes when its SDK was not compiled in: it validates the setup
// header like the real one, then reports NOT_COMPILED, which is a different thing from failing.
inline const rsf_generation_provider* not_compiled_provider()
{
    struct Stub {
        static rsf_backend_result create(const rsf_generation_setup* setup, void** out, void** chain)
        {
            const auto result = fg_setup_header(setup, out, chain);
            return result != 0 ? result : RSF_BACKEND_ERROR_NOT_COMPILED;
        }
        static rsf_backend_result configure(void*, const rsf_fg_options*) { return RSF_BACKEND_ERROR_NOT_COMPILED; }
        static rsf_backend_result begin(void*, uint64_t) { return RSF_BACKEND_ERROR_NOT_COMPILED; }
        static rsf_backend_result marker(void*, rsf_latency_marker, uint64_t, uint32_t) { return RSF_BACKEND_ERROR_NOT_COMPILED; }
        static rsf_backend_result prepare(void*, void*, const rsf_fg_frame*) { return RSF_BACKEND_ERROR_NOT_COMPILED; }
        static rsf_backend_result after(void*) { return RSF_BACKEND_ERROR_NOT_COMPILED; }
        static rsf_backend_result status(void*, rsf_fg_status*) { return RSF_BACKEND_ERROR_NOT_COMPILED; }
        static rsf_backend_result retirement(void*, rsf_fg_retirement*) { return RSF_BACKEND_ERROR_NOT_COMPILED; }
        static void destroy(void*) {}
        static rsf_backend_result abort_frame(void*, uint64_t) { return RSF_BACKEND_ERROR_NOT_COMPILED; }
    };
    static const rsf_generation_provider provider{sizeof(provider), Stub::create, Stub::configure, Stub::begin,
        Stub::marker, Stub::prepare, Stub::after, Stub::status, Stub::retirement, Stub::destroy, Stub::abort_frame};
    return &provider;
}
}
