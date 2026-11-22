// SPDX-License-Identifier: MIT
#include <rescaleframe/native_sr.h>
#include <rescaleframe/game_api.h>
#include <rescaleframe/native_fg.h>
#include <rescaleframe/native_translucency.h>
#include <rescaleframe/d3d11_state.h>
#include <rescaleframe/motion_resolve.h>
#include "native_regions.h"
#include "native_reinsert.h"
#include "native_sr_completion.h"
#include <d3d11.h>
#include <wrl/client.h>


namespace {
bool same_device_inputs(void* raw_context, const rsf_game_render_pass& pass) {
    auto* context=static_cast<ID3D11DeviceContext*>(raw_context);
    Microsoft::WRL::ComPtr<ID3D11Device> device; context->GetDevice(&device);
    Microsoft::WRL::ComPtr<IUnknown> identity;
    if (!device || FAILED(device.As(&identity))) return false;
    void* textures[]{pass.color_input,pass.color_output,pass.depth,pass.motion,pass.exposure,
        pass.color_output_readable,pass.temporal_mask};
    for (void* pointer:textures) if (pointer) {
        Microsoft::WRL::ComPtr<ID3D11Device> owner;
        static_cast<ID3D11Texture2D*>(pointer)->GetDevice(&owner);
        Microsoft::WRL::ComPtr<IUnknown> other;
        if (!owner || FAILED(owner.As(&other)) || other.Get()!=identity.Get()) return false;
    }
    return true;
}
// Private leaf result outside the public result enum.
constexpr int leaf_fault = INT32_MIN;

int validate_device(void* context, const rsf_game_render_pass* pass) try
{
    return same_device_inputs(context, *pass);
}
catch (...) { return leaf_fault; }

int depth_layout(const rsf_game_render_pass* pass) try
{
    D3D11_TEXTURE2D_DESC depth{};
    static_cast<ID3D11Texture2D*>(pass->depth)->GetDesc(&depth);
    if (!rsf_native_regions_depth_valid(&pass->depth_domains, depth.Format, pass->render_rect))
        return -1;
    return rsf_game_depth_domains_requested(&pass->depth_domains);
}
catch (...) { return leaf_fault; }

void clear_predication(void* context)
{
    static_cast<ID3D11DeviceContext*>(context)->SetPredication(nullptr, FALSE);
}

int resize_output(uint32_t width, uint32_t height) try
{
    return rsf_dlss_pipeline_resize_output(width, height, nullptr, nullptr);
}
catch (...) { return leaf_fault; }

int prepare_regions(void* context, const rsf_game_render_pass* pass, void** textures) try
{
    ID3D11Texture2D *color = nullptr, *depth = nullptr, *motion = nullptr;
    if (!rsf_native_regions_prepare(static_cast<ID3D11DeviceContext*>(context),
        static_cast<ID3D11Texture2D*>(pass->color_input), static_cast<ID3D11Texture2D*>(pass->depth),
        static_cast<ID3D11Texture2D*>(pass->motion), pass->render_rect, &color, &depth, &motion,
        &pass->depth_domains, pass->camera.depth_inverted != 0)) return 0;
    textures[0] = color; textures[1] = depth; textures[2] = motion;
    return 1;
}
catch (...) { return leaf_fault; }

int take_translucency(const rsf_game_render_pass* pass, rsf_native_translucency_masks* masks) try
{
    return rsf_native_translucency_take(pass, masks);
}
catch (...) { return leaf_fault; }

int merge_mask(void* context, const rsf_game_render_pass* pass, rsf_dlss_pipeline_frame* frame) try
{
    ID3D11Texture2D *reactive = nullptr, *bias = nullptr;
    if (!rsf_native_regions_temporal_mask(static_cast<ID3D11DeviceContext*>(context),
        static_cast<ID3D11Texture2D*>(pass->temporal_mask), pass->render_rect, pass->temporal_mask_weights,
        static_cast<ID3D11Texture2D*>(frame->reactive_mask), static_cast<ID3D11Texture2D*>(frame->bias_mask),
        &reactive, &bias)) return 0;
    frame->reactive_mask = reactive; frame->bias_mask = bias;
    return 1;
}
catch (...) { return leaf_fault; }

int evaluate_pipeline(void* context, const rsf_dlss_pipeline_frame* frame) try
{
    return rsf_dlss_pipeline_on_frame(context, frame);
}
catch (...) { return leaf_fault; }

int publish_output(void* context, const rsf_game_render_pass* pass) try
{
    return rsf_native_reinsert(static_cast<ID3D11DeviceContext*>(context),
        static_cast<ID3D11Texture2D*>(rsf_dlss_pipeline_output_texture()),
        static_cast<ID3D11Texture2D*>(pass->color_output),
        static_cast<ID3D11Texture2D*>(pass->color_output_readable), pass->output_rect);
}
catch (...) { return leaf_fault; }

struct SrServices {
    decltype(&validate_device) validate_device;
    decltype(&depth_layout) depth_layout;
    decltype(&rsf_d3d11_state_save) save;
    decltype(&rsf_d3d11_state_restore) restore;
    decltype(&clear_predication) clear_predication;
    decltype(&resize_output) resize_output;
    decltype(&prepare_regions) prepare_regions;
    decltype(&rsf_native_scalar_exposure) exposure;
    decltype(&take_translucency) take_translucency;
    decltype(&merge_mask) merge_mask;
    decltype(&evaluate_pipeline) evaluate_pipeline;
    decltype(&publish_output) publish_output;
};
constexpr SrServices sr_services{
    validate_device, depth_layout, rsf_d3d11_state_save, rsf_d3d11_state_restore, clear_predication,
    resize_output, prepare_regions, rsf_native_scalar_exposure, take_translucency, merge_mask,
    evaluate_pipeline, publish_output,
};
}

extern "C" rsf_dlss_pipeline_result rsf_core_native_sr_evaluate(
    void*, const rsf_game_render_pass*, const SrServices*, uint32_t*);

rsf_dlss_pipeline_result rsf_native_sr_evaluate_ordered(
    void* context, const rsf_game_render_pass* pass, uint32_t* ordered)
{
    return rsf_core_native_sr_evaluate(context, pass, &sr_services, ordered);
}

extern "C" RSF_RUNTIME_API rsf_dlss_pipeline_result rsf_native_sr_evaluate(void* context,
    const rsf_game_render_pass* pass)
{
    return rsf_native_sr_evaluate_ordered(context, pass, nullptr);
}

namespace {
rsf_motion_decode* create_decode(void* device, uint32_t width, uint32_t height) try
{
    const rsf_motion_decode_setup setup{sizeof(setup), RSF_MOTION_DECODE_ABI_VERSION,
        width, height, DXGI_FORMAT_R32G32_FLOAT, nullptr, nullptr};
    rsf_motion_decode* decode = nullptr;
    return rsf_motion_decode_create(device, &setup, &decode) == RSF_MOTION_DECODE_OK ? decode : nullptr;
}
catch (...) { return nullptr; }

rsf_motion_resolve* create_resolve(void* device, uint32_t width, uint32_t height) try
{
    rsf_motion_resolve* resolve = nullptr;
    return rsf_motion_resolve_create(device, width, height, &resolve) ? resolve : nullptr;
}
catch (...) { return nullptr; }

int decode_motion(rsf_motion_decode* decode, void* context, void* motion,
    const rsf_motion_decode_params* params) try
{
    return rsf_motion_decode_run(decode, context, motion, params) == RSF_MOTION_DECODE_OK;
}
catch (...) { return 0; }

int resolve_motion(rsf_motion_resolve* resolve, void* context, void* motion, void* depth,
    const rsf_motion_resolve_params* params) try
{
    return rsf_motion_resolve_run(resolve, context, motion, depth, params);
}
catch (...) { return 0; }

int capture_fg(void* context, const rsf_dlss_frame* frame, uint64_t source, uint64_t native) try
{
    rsf_native_fg_stage("normalize-capture", source, native, 0);
    rsf_native_fg_capture(context, frame);
    rsf_native_fg_stage("normalize-captured", source, native, 0);
    return rsf_native_fg_inputs_ready(source);
}
catch (...) { return 0; }

struct FgServices {
    decltype(&create_decode) create_decode;
    decltype(&create_resolve) create_resolve;
    decltype(&decode_motion) decode;
    decltype(&resolve_motion) resolve;
    decltype(&rsf_motion_decode_texture) decoded_texture;
    decltype(&rsf_motion_resolve_depth) depth;
    decltype(&rsf_motion_resolve_motion) motion;
    decltype(&rsf_motion_decode_destroy) destroy_decode;
    decltype(&rsf_motion_resolve_destroy) destroy_resolve;
    decltype(&rsf_native_fg_stage) stage;
    decltype(&capture_fg) capture;
    decltype(&rsf_native_fg_scene) refuse;
};
constexpr FgServices fg_services{
    create_decode, create_resolve, decode_motion, resolve_motion, rsf_motion_decode_texture,
    rsf_motion_resolve_depth, rsf_motion_resolve_motion, rsf_motion_decode_destroy, rsf_motion_resolve_destroy,
    rsf_native_fg_stage, capture_fg, rsf_native_fg_scene,
};
}

extern "C" int32_t rsf_core_native_sr_capture_fg(void* context, const rsf_game_render_pass* pass,
    const rsf_motion_decode_params* motion, const SrServices*, const FgServices*);
extern "C" RSF_RUNTIME_API int32_t rsf_native_sr_capture_fg(void* context, const rsf_game_render_pass* pass,
    const rsf_motion_decode_params* motion)
{
    return rsf_core_native_sr_capture_fg(context, pass, motion, &sr_services, &fg_services);
}
