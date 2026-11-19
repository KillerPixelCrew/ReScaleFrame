// SPDX-License-Identifier: MIT
#include <rescaleframe/native_sr.h>
#include <rescaleframe/game_api.h>
#include <rescaleframe/native_composition.h>
#include <rescaleframe/native_cpu.h>
#include <rescaleframe/native_fg.h>
#include <rescaleframe/native_window.h>
#include <rescaleframe/native_scene.h>
#include <rescaleframe/native_translucency.h>
#include <rescaleframe/d3d11_state.h>
#include <rescaleframe/motion_resolve.h>
#include "native_regions.h"
#include "native_reinsert.h"
#include "native_sr_completion.h"
#include <d3d11.h>
#include <cstring>
#include <wrl/client.h>

enum class Temporal : uint32_t { Sr, Fg };
extern "C" void rsf_core_native_sr_camera(Temporal, const rsf_game_render_pass*, rsf_pipeline_camera_frame*);
extern "C" void rsf_core_native_sr_commit(Temporal, const rsf_game_render_pass*);
extern "C" void rsf_core_native_sr_reset(Temporal);
extern "C" void rsf_core_native_sr_publish_surface(uint32_t width, uint32_t height,
    uint32_t render_width, uint32_t render_height);
extern "C" void rsf_core_native_sr_clear_surface();
extern "C" void rsf_core_native_exposure_release();
void* rsf_native_scalar_exposure(void* context, void* input);

namespace {
struct FgSceneScope {
    rsf_game_render_pass previous;
    explicit FgSceneScope(const rsf_game_render_pass& current) : previous(current) {
        (void)rsf_native_fg_scene_swap(&previous);
    }
    ~FgSceneScope() { (void)rsf_native_fg_scene_swap(&previous); }
};
bool same_device_inputs(void* raw_context, const rsf_game_render_pass& pass) {
    auto* context=static_cast<ID3D11DeviceContext*>(raw_context);
    if (context->GetType()!=D3D11_DEVICE_CONTEXT_IMMEDIATE) return false;
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
thread_local bool native_destinations_ordered = false;
struct Bindings {
    void* context;
    rsf_d3d11_state saved{};
    bool valid;
    explicit Bindings(void* c) : context(c), valid(rsf_d3d11_state_save(c, &saved) != 0) {}
    ~Bindings() { if (valid) rsf_d3d11_state_restore(context, &saved); }
};
}
namespace {
int sr_call_failed() noexcept
{
    native_destinations_ordered = false;
    rsf_core_native_sr_reset(Temporal::Sr);
    return RSF_DLSS_PIPELINE_ERROR_RESOURCE_FAILED;
}

int validate_device(void* context, const rsf_game_render_pass* pass) try
{
    return same_device_inputs(context, *pass);
}
catch (...) { (void)sr_call_failed(); return -1; }

int depth_layout(const rsf_game_render_pass* pass) try
{
    D3D11_TEXTURE2D_DESC depth{};
    static_cast<ID3D11Texture2D*>(pass->depth)->GetDesc(&depth);
    if (!rsf_native_regions_depth_valid(&pass->depth_domains, depth.Format, pass->render_rect))
        return -1;
    return rsf_game_depth_domains_requested(&pass->depth_domains);
}
catch (...) { (void)sr_call_failed(); return -2; }

void clear_predication(void* context)
{
    static_cast<ID3D11DeviceContext*>(context)->SetPredication(nullptr, FALSE);
}

int resize_output(uint32_t width, uint32_t height) try
{
    return rsf_dlss_pipeline_resize_output(width, height, nullptr, nullptr);
}
catch (...) { return sr_call_failed(); }

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
catch (...) { (void)sr_call_failed(); return 0; }

int take_translucency(const rsf_game_render_pass* pass, rsf_native_translucency_masks* masks) try
{
    return rsf_native_translucency_take(pass, masks);
}
catch (...) { (void)sr_call_failed(); return -1; }

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
catch (...) { (void)sr_call_failed(); return 0; }

int evaluate_pipeline(void* context, const rsf_dlss_pipeline_frame* frame) try
{
    return rsf_dlss_pipeline_on_frame(context, frame);
}
catch (...) { return sr_call_failed(); }

int publish_output(void* context, const rsf_game_render_pass* pass) try
{
    return rsf_native_reinsert(static_cast<ID3D11DeviceContext*>(context),
        static_cast<ID3D11Texture2D*>(rsf_dlss_pipeline_output_texture()),
        static_cast<ID3D11Texture2D*>(pass->color_output),
        static_cast<ID3D11Texture2D*>(pass->color_output_readable), pass->output_rect);
}
catch (...) { (void)sr_call_failed(); return 0; }

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
}

extern "C" rsf_dlss_pipeline_result rsf_core_native_sr_evaluate(void*, const rsf_game_render_pass*, const SrServices*);

extern "C" RSF_RUNTIME_API rsf_dlss_pipeline_result rsf_native_sr_evaluate(void* context,
    const rsf_game_render_pass* pass)
{
    static constexpr SrServices services{
        validate_device, depth_layout, rsf_d3d11_state_save, rsf_d3d11_state_restore, clear_predication,
        resize_output, prepare_regions, rsf_native_scalar_exposure, take_translucency, merge_mask,
        evaluate_pipeline, publish_output,
    };
    native_destinations_ordered = true;
    return rsf_core_native_sr_evaluate(context, pass, &services);
}

bool rsf_native_sr_native_destinations_ordered() { return native_destinations_ordered; }


namespace {
struct FgInputs {
    Microsoft::WRL::ComPtr<ID3D11Device> device;
    rsf_motion_decode* decode{};
    rsf_motion_resolve* resolve{};
    uint32_t width{}, height{};
    void release() {
        rsf_motion_decode_destroy(decode); decode = nullptr;
        rsf_motion_resolve_destroy(resolve); resolve = nullptr;
        device.Reset(); width = height = 0;
        rsf_core_native_sr_reset(Temporal::Fg);
    }
};
FgInputs& fg_inputs() { static FgInputs value; return value; }
}
extern "C" RSF_RUNTIME_API int32_t rsf_native_sr_capture_fg(void* raw_context,
    const rsf_game_render_pass* pass, const rsf_motion_decode_params* motion) try
{
    if (!raw_context || !pass || pass->struct_size < sizeof(*pass) ||
        pass->role != RSF_GAME_RENDER_FG_INPUTS || !motion || motion->struct_size < sizeof(*motion))
        return RSF_ERROR_INVALID_ARGUMENT;
    // A refused repeated explicit lease must not leave an earlier same-source capture eligible.
    struct Refusal {
        const rsf_game_render_pass* pass;
        bool accepted{};
        ~Refusal() {
            if (accepted || !pass->source_frame_id) return;
            auto refused=*pass; refused.flags|=RSF_GAME_RENDER_NO_FG;
            FgSceneScope scope(refused);
            rsf_native_fg_scene(&refused);
        }
    } refusal{pass};
    if (!pass->source_frame_id || !pass->camera_valid || !pass->color_input || !pass->depth || !pass->motion ||
        !pass->session_id || !pass->submission_id || !pass->motion_camera_included ||
        !pass->motion_to_uv[0] || !pass->motion_to_uv[1]) return RSF_ERROR_NOT_READY;
    const auto& source = pass->camera;
    if (source.struct_size < sizeof(source) || source.abi_version != RSF_GAME_CAMERA_ABI_VERSION)
        return RSF_ERROR_ABI_MISMATCH;
    if (pass->render_rect[0] < 0 || pass->render_rect[1] < 0 ||
        pass->render_rect[2] <= pass->render_rect[0] || pass->render_rect[3] <= pass->render_rect[1] ||
        source.render_width != uint32_t(pass->render_rect[2] - pass->render_rect[0]) ||
        source.render_height != uint32_t(pass->render_rect[3] - pass->render_rect[1]) ||
        !source.output_width || !source.output_height) return RSF_ERROR_INVALID_ARGUMENT;
    auto* context = static_cast<ID3D11DeviceContext*>(raw_context);
    if (context->GetType() != D3D11_DEVICE_CONTEXT_IMMEDIATE) return RSF_ERROR_INVALID_ARGUMENT;
    rsf_native_fg_stage("normalize-enter", pass->source_frame_id, pass->native_frame, 0);
    if (!same_device_inputs(context,*pass)) return RSF_ERROR_INVALID_ARGUMENT;
    D3D11_TEXTURE2D_DESC mapped_depth_desc{};
    static_cast<ID3D11Texture2D*>(pass->depth)->GetDesc(&mapped_depth_desc);
    if (!rsf_native_regions_depth_valid(&pass->depth_domains,mapped_depth_desc.Format,pass->render_rect))
        return RSF_ERROR_INVALID_ARGUMENT;
    Bindings bindings(context); if (!bindings.valid) return RSF_ERROR_NOT_READY;
    context->SetPredication(nullptr, FALSE);
    Microsoft::WRL::ComPtr<ID3D11Device> device; context->GetDevice(&device);
    auto& inputs = fg_inputs();
    if (inputs.device.Get() != device.Get() || inputs.width != source.render_width || inputs.height != source.render_height)
        inputs.release();
    if (!inputs.decode) {
        inputs.device = device; inputs.width = source.render_width; inputs.height = source.render_height;
        rsf_motion_decode_setup setup{sizeof(setup), RSF_MOTION_DECODE_ABI_VERSION,
            inputs.width, inputs.height, DXGI_FORMAT_R32G32_FLOAT, nullptr, nullptr};
        if (rsf_motion_decode_create(device.Get(), &setup, &inputs.decode) != RSF_MOTION_DECODE_OK ||
            !rsf_motion_resolve_create(device.Get(), inputs.width, inputs.height, &inputs.resolve)) {
            inputs.release(); return RSF_ERROR_NOT_READY;
        }
    }
    rsf_native_fg_stage("normalize-regions-decode", pass->source_frame_id, pass->native_frame, 0);
    ID3D11Texture2D *color{}, *depth{}, *vectors{};
    if (!rsf_native_regions_prepare(context, static_cast<ID3D11Texture2D*>(pass->color_input),
        static_cast<ID3D11Texture2D*>(pass->depth), static_cast<ID3D11Texture2D*>(pass->motion),
        pass->render_rect, &color, &depth, &vectors, &pass->depth_domains, source.depth_inverted != 0) ||
        rsf_motion_decode_run(inputs.decode, context, vectors, motion) != RSF_MOTION_DECODE_OK)
        return RSF_ERROR_NOT_READY;
    rsf_motion_resolve_params resolve{}; resolve.struct_size = sizeof(resolve);
    std::memcpy(resolve.clip_to_previous, source.clip_to_previous_clip, sizeof(resolve.clip_to_previous));
    resolve.decoded_to_pixels[0] = -float(inputs.width) * pass->motion_to_uv[0];
    resolve.decoded_to_pixels[1] = -float(inputs.height) * pass->motion_to_uv[1];
    resolve.has_sentinel = motion->zero_means_unwritten || motion->validity_enabled;
    resolve.sentinel = motion->invalid_value;
    std::memcpy(resolve.jitter_pixels, source.jitter_pixels, sizeof(resolve.jitter_pixels));
    rsf_native_fg_stage("normalize-resolve", pass->source_frame_id, pass->native_frame, 0);
    if (!rsf_motion_resolve_run(inputs.resolve, context, rsf_motion_decode_texture(inputs.decode), depth, &resolve))
        return RSF_ERROR_NOT_READY;
    rsf_pipeline_camera_frame camera{};
    rsf_core_native_sr_camera(Temporal::Fg, pass, &camera);

    camera.motion_decoded = 1; camera.has_motion_sentinel = 0; camera.camera_motion_included = 1;
    camera.motion_scale[0] = 1.0f / float(inputs.width); camera.motion_scale[1] = 1.0f / float(inputs.height);
    rsf_frame_resources resources{}; resources.struct_size = sizeof(resources);
    resources.color_in = color; resources.color_out = color;
    resources.depth = rsf_motion_resolve_depth(inputs.resolve); resources.motion = rsf_motion_resolve_motion(inputs.resolve);
    resources.render_width = inputs.width; resources.render_height = inputs.height;
    resources.output_width = source.output_width; resources.output_height = source.output_height;
    resources.quality = RSF_DLSS_QUALITY_NATIVE;
    rsf_dlss_frame frame{}; frame.struct_size = sizeof(frame);
    if (rsf_assemble_dlss_frame(&camera, &resources, &frame) != RSF_FRAME_ASSEMBLY_OK) return RSF_ERROR_NOT_READY;
    auto captured = *pass;
    if (camera.reset) captured.flags |= RSF_GAME_RENDER_RESET;
    FgSceneScope scope(captured);
    rsf_native_fg_stage("normalize-capture", pass->source_frame_id, pass->native_frame, 0);
    rsf_native_fg_capture(context, &frame);
    rsf_native_fg_stage("normalize-captured", pass->source_frame_id, pass->native_frame, 0);
    if (!rsf_native_fg_inputs_ready(pass->source_frame_id)) return RSF_ERROR_NOT_READY;
    rsf_core_native_sr_commit(Temporal::Fg, pass);
    refusal.accepted=true;
    return RSF_OK;
} catch (...) { return RSF_ERROR_NOT_READY; }

extern "C" RSF_RUNTIME_API void rsf_native_sr_release_resources(void)
{
    rsf_native_scene_reset(); rsf_native_window_reset(); rsf_native_cpu_release(); rsf_native_composition_release();
    rsf_native_sr_set_enabled(0);
    rsf_core_native_exposure_release(); rsf_native_regions_release();
    fg_inputs().release(); rsf_native_reinsert_release(); rsf_core_native_sr_reset(Temporal::Sr);
    rsf_native_translucency_release();
    rsf_core_native_sr_clear_surface();
}

extern "C" RSF_RUNTIME_API int rsf_native_sr_set_surface(uint32_t width, uint32_t height) try
{
    rsf_dlss_pipeline_status status{}; status.struct_size = sizeof(status);
    if (!width || !height || rsf_dlss_pipeline_get_status(&status) != RSF_DLSS_PIPELINE_OK || !status.running ||
        status.output_width != width || status.output_height != height) return 0;
    rsf_core_native_sr_publish_surface(width, height, status.render_width, status.render_height);
    return 1;
}
catch (...) { return 0; }
