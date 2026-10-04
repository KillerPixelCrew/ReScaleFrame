// SPDX-License-Identifier: GPL-3.0-only
#include <rescaleframe/native_sr.h>
#include <rescaleframe/native_composition.h>
#include <rescaleframe/native_cpu.h>
#include <rescaleframe/native_fg.h>
#include <rescaleframe/native_window.h>
#include <rescaleframe/native_scene.h>
#include <rescaleframe/native_translucency.h>
#include <rescaleframe/d3d11_state.h>
#include <rescaleframe/fullscreen_pass.h>
#include "native_regions.h"
#include <d3d11.h>
#include <cstring>
#include <wrl/client.h>
#include <array>
#include <atomic>
#include <mutex>

namespace {
struct History {
    uint64_t session = 0, owner = 0, frame = 0;
    uint32_t width = 0, height = 0;
    int32_t render_rect[4]{}, output_rect[4]{};
};
// The existing pipeline has one active immediate context. This record shares that ownership.
History history;
std::atomic<uint32_t> native_enabled{0};
std::mutex surface_guard;
rsf_game_render_config surface_config{};
struct ExposureResources {
    Microsoft::WRL::ComPtr<ID3D11Device> device;
    Microsoft::WRL::ComPtr<ID3D11Texture2D> scalar;
    Microsoft::WRL::ComPtr<ID3D11RenderTargetView> target;
    struct Source {
        Microsoft::WRL::ComPtr<ID3D11Texture2D> texture;
        Microsoft::WRL::ComPtr<ID3D11ShaderResourceView> view;
    };
    std::array<Source, 2> sources;
    uint32_t next = 0;
    rsf_fullscreen_pass* blit = nullptr;
    void release() {
        rsf_fullscreen_pass_destroy(blit); blit = nullptr;
        for (auto& source : sources) { source.view.Reset(); source.texture.Reset(); }
        target.Reset(); scalar.Reset(); device.Reset(); next = 0;
    }
};
ExposureResources& exposure_resources() { static ExposureResources instance; return instance; }
ID3D11Texture2D* scalar_exposure(ID3D11DeviceContext* context, ID3D11Texture2D* source)
{
    if (!source) return nullptr;
    D3D11_TEXTURE2D_DESC descriptor{}; source->GetDesc(&descriptor);
    if (descriptor.Width != 1 || descriptor.Height != 1 || descriptor.ArraySize != 1 || descriptor.SampleDesc.Count != 1)
        return nullptr;
    if (descriptor.Format == DXGI_FORMAT_R32_FLOAT) return source;
    DXGI_FORMAT format = descriptor.Format;
    if (format == DXGI_FORMAT_R32G32B32A32_TYPELESS) format = DXGI_FORMAT_R32G32B32A32_FLOAT;
    if (format == DXGI_FORMAT_R16G16B16A16_TYPELESS) format = DXGI_FORMAT_R16G16B16A16_FLOAT;
    if (format == DXGI_FORMAT_R32G32_TYPELESS) format = DXGI_FORMAT_R32G32_FLOAT;
    if (format == DXGI_FORMAT_R16G16_TYPELESS) format = DXGI_FORMAT_R16G16_FLOAT;
    if (format != DXGI_FORMAT_R32G32B32A32_FLOAT && format != DXGI_FORMAT_R16G16B16A16_FLOAT &&
        format != DXGI_FORMAT_R32G32_FLOAT && format != DXGI_FORMAT_R16G16_FLOAT) return nullptr;
    auto& resources = exposure_resources(); Microsoft::WRL::ComPtr<ID3D11Device> device;
    context->GetDevice(&device);
    if (resources.device.Get() != device.Get()) resources.release();
    if (!resources.scalar) {
        resources.device = device;
        D3D11_TEXTURE2D_DESC scalar{}; scalar.Width = scalar.Height = scalar.MipLevels = scalar.ArraySize = 1;
        scalar.Format = DXGI_FORMAT_R32_FLOAT; scalar.SampleDesc.Count = 1;
        scalar.BindFlags = D3D11_BIND_RENDER_TARGET | D3D11_BIND_SHADER_RESOURCE;
        rsf_fullscreen_setup setup{sizeof(setup), RSF_FULLSCREEN_PASS_ABI_VERSION, nullptr, nullptr};
        if (FAILED(device->CreateTexture2D(&scalar, nullptr, &resources.scalar)) ||
            FAILED(device->CreateRenderTargetView(resources.scalar.Get(), nullptr, &resources.target)) ||
            rsf_fullscreen_pass_create(device.Get(), &setup, &resources.blit) != RSF_FULLSCREEN_OK) {
            resources.release(); return nullptr;
        }
    }
    ExposureResources::Source* selected = nullptr;
    for (auto& entry : resources.sources) if (entry.texture.Get() == source) { selected = &entry; break; }
    if (!selected) {
        auto& entry = resources.sources[resources.next++ % resources.sources.size()];
        entry.view.Reset(); entry.texture.Reset();
        D3D11_SHADER_RESOURCE_VIEW_DESC view{}; view.Format = format;
        view.ViewDimension = D3D11_SRV_DIMENSION_TEXTURE2D; view.Texture2D.MipLevels = 1;
        if (FAILED(device->CreateShaderResourceView(source, &view, &entry.view))) return nullptr;
        entry.texture = source; selected = &entry;
    }
    const rsf_fullscreen_draw draw{sizeof(draw), RSF_FULLSCREEN_COPY, 1, 1, 1};
    return rsf_fullscreen_pass_draw(resources.blit, context, resources.target.Get(), selected->view.Get(), &draw) ==
        RSF_FULLSCREEN_OK ? resources.scalar.Get() : nullptr;
}
struct Bindings {
    void* context;
    rsf_d3d11_state saved{};
    bool valid;
    explicit Bindings(void* c) : context(c), valid(rsf_d3d11_state_save(c, &saved) != 0) {}
    ~Bindings() { if (valid) rsf_d3d11_state_restore(context, &saved); }
};
}
extern "C" RSF_RUNTIME_API rsf_dlss_pipeline_result rsf_native_sr_evaluate(void* context,
    const rsf_game_render_pass* pass) try
{
    if (!context || !pass || pass->struct_size < sizeof(*pass) || pass->role != RSF_GAME_RENDER_SR ||
        !pass->camera_valid || !pass->color_input || !pass->color_output || !pass->depth || !pass->motion ||
        pass->render_rect[0] < 0 || pass->render_rect[1] < 0 || pass->output_rect[0] < 0 || pass->output_rect[1] < 0)
        return RSF_DLSS_PIPELINE_ERROR_INVALID_ARGUMENT;
    const auto& source = pass->camera;
    if (source.struct_size < sizeof(source) || source.abi_version != RSF_GAME_FRAME_ABI_VERSION ||
        !source.render_width || !source.render_height) return RSF_DLSS_PIPELINE_ERROR_ABI_MISMATCH;
    const uint32_t output_width = uint32_t(pass->output_rect[2] - pass->output_rect[0]);
    const uint32_t output_height = uint32_t(pass->output_rect[3] - pass->output_rect[1]);
    if (pass->render_rect[2] <= pass->render_rect[0] || pass->render_rect[3] <= pass->render_rect[1] ||
        pass->output_rect[2] <= pass->output_rect[0] || pass->output_rect[3] <= pass->output_rect[1] ||
        source.render_width != uint32_t(pass->render_rect[2] - pass->render_rect[0]) ||
        source.render_height != uint32_t(pass->render_rect[3] - pass->render_rect[1]))
        return RSF_DLSS_PIPELINE_ERROR_INVALID_ARGUMENT;
    rsf_pipeline_camera_frame camera{};
    camera.struct_size = sizeof(camera); camera.abi_version = RSF_FRAME_ASSEMBLY_ABI_VERSION;
    std::memcpy(camera.view_to_clip, source.view_to_clip, 64);
    std::memcpy(camera.clip_to_view, source.clip_to_view, 64);
    std::memcpy(camera.clip_to_prev_clip, source.clip_to_previous_clip, 64);
    std::memcpy(camera.prev_clip_to_clip, pass->previous_clip_to_clip, 64);
    std::memcpy(camera.camera_position, source.view_to_world + 12, 12);
    std::memcpy(camera.camera_right, source.view_to_world, 12);
    std::memcpy(camera.camera_up, source.view_to_world + 4, 12);
    std::memcpy(camera.camera_forward, source.view_to_world + 8, 12);
    std::memcpy(camera.jitter_pixels, source.jitter_pixels, 8);
    camera.has_jitter = 1; camera.depth_inverted = source.depth_inverted;
    camera.near_plane = source.near_plane; camera.far_plane = source.far_plane;
    camera.vertical_fov = source.vertical_fov_radians;
    camera.aspect_ratio = source.view_to_clip[5] / source.view_to_clip[0];
    camera.camera_motion_included = pass->motion_camera_included;
    camera.motion_scale[0] = pass->motion_to_uv[0]; camera.motion_scale[1] = pass->motion_to_uv[1];
    rsf_dlss_pipeline_status status{}; status.struct_size = sizeof(status);
    if (rsf_dlss_pipeline_get_status(&status) == RSF_DLSS_PIPELINE_OK && status.backend != 1) {
        // The legacy alternate-backend adapter accepts NDC-basis multipliers. Translate the
        // producer's canonical UV convention here; new native plugins never learn vendor units.
        camera.motion_scale[0] *= 2.0f; camera.motion_scale[1] *= -2.0f;
    }
    camera.reset = (pass->flags & RSF_GAME_RENDER_RESET) || history.session != pass->session_id ||
        history.owner != pass->history_key || pass->native_frame != uint32_t(history.frame + 1) ||
        history.width != source.render_width || history.height != source.render_height ||
        std::memcmp(history.render_rect, pass->render_rect, sizeof(history.render_rect)) ||
        std::memcmp(history.output_rect, pass->output_rect, sizeof(history.output_rect));
    rsf_dlss_pipeline_frame frame{};
    frame.struct_size = sizeof(frame); frame.abi_version = RSF_DLSS_PIPELINE_ABI_VERSION;
    frame.scene_color = pass->color_input; frame.depth = pass->depth; frame.game_motion = pass->motion;
    frame.exposure = nullptr; frame.render_width = source.render_width;
    frame.render_height = source.render_height; frame.camera = &camera;
    Bindings bindings(context); if (!bindings.valid) return RSF_DLSS_PIPELINE_ERROR_RESOURCE_FAILED;
    static_cast<ID3D11DeviceContext*>(context)->SetPredication(nullptr, FALSE);
    const auto resized = rsf_dlss_pipeline_resize_output(output_width, output_height, nullptr, nullptr);
    if (resized != RSF_DLSS_PIPELINE_OK) { history = {}; return resized; }
    D3D11_TEXTURE2D_DESC color_desc{}, depth_desc{}, motion_desc{};
    static_cast<ID3D11Texture2D*>(pass->color_input)->GetDesc(&color_desc);
    static_cast<ID3D11Texture2D*>(pass->depth)->GetDesc(&depth_desc);
    static_cast<ID3D11Texture2D*>(pass->motion)->GetDesc(&motion_desc);
    const bool local_inputs = !pass->render_rect[0] && !pass->render_rect[1] &&
        color_desc.Format == DXGI_FORMAT_R16G16B16A16_FLOAT &&
        color_desc.Width == source.render_width && color_desc.Height == source.render_height &&
        depth_desc.Width == source.render_width && depth_desc.Height == source.render_height &&
        motion_desc.Width == source.render_width && motion_desc.Height == source.render_height;
    if (!local_inputs) {
        ID3D11Texture2D *color = nullptr, *depth = nullptr, *motion = nullptr;
        if (!rsf_native_regions_prepare(static_cast<ID3D11DeviceContext*>(context),
            static_cast<ID3D11Texture2D*>(pass->color_input), static_cast<ID3D11Texture2D*>(pass->depth),
            static_cast<ID3D11Texture2D*>(pass->motion), pass->render_rect, &color, &depth, &motion))
            return RSF_DLSS_PIPELINE_ERROR_RESOURCE_FAILED;
        frame.scene_color = color; frame.depth = depth; frame.game_motion = motion;
    }
    frame.exposure = scalar_exposure(static_cast<ID3D11DeviceContext*>(context), static_cast<ID3D11Texture2D*>(pass->exposure));
    // Rect-local like the cropped inputs, and only for this view's own frame and rectangle.
    rsf_native_translucency_masks masks{}; masks.struct_size = sizeof(masks);
    if (rsf_native_translucency_take(pass, &masks)) {
        frame.color_before_transparency = masks.color_before_transparency;
        frame.transparency_layer = masks.transparency_layer;
        frame.reactive_mask = masks.reactive; frame.transparency_mask = masks.coverage; frame.bias_mask = masks.bias;
        frame.motion_depth_layer = masks.motion_depth;
    }
    rsf_native_fg_scene(pass);
    const auto result = rsf_dlss_pipeline_on_frame(context, &frame);
    rsf_native_fg_scene(nullptr);
    if (result != RSF_DLSS_PIPELINE_OK) { history = {}; return result; }
    auto* output = static_cast<ID3D11Texture2D*>(rsf_dlss_pipeline_output_texture());
    auto* target = static_cast<ID3D11Texture2D*>(pass->color_output);
    auto* readable = static_cast<ID3D11Texture2D*>(pass->color_output_readable);
    auto fits = [&](ID3D11Texture2D* destination) {
        if (!output || !destination) return false;
        D3D11_TEXTURE2D_DESC src{}, dst{}; output->GetDesc(&src); destination->GetDesc(&dst);
        return src.Format == dst.Format && src.SampleDesc.Count == 1 && dst.SampleDesc.Count == 1 &&
            src.Width == output_width && src.Height == output_height &&
            uint32_t(pass->output_rect[2]) <= dst.Width && uint32_t(pass->output_rect[3]) <= dst.Height;
    };
    if (!fits(target) || (readable && !fits(readable))) { history = {}; return RSF_DLSS_PIPELINE_ERROR_RESOURCE_FAILED; }
    auto* immediate = static_cast<ID3D11DeviceContext*>(context);
    immediate->OMSetRenderTargets(0, nullptr, nullptr);
    const D3D11_BOX box{0,0,0,output_width,output_height,1};
    immediate->CopySubresourceRegion(target,0,UINT(pass->output_rect[0]),UINT(pass->output_rect[1]),0,output,0,&box);
    if (readable && readable != target)
        immediate->CopySubresourceRegion(readable,0,UINT(pass->output_rect[0]),UINT(pass->output_rect[1]),0,output,0,&box);
    history.session = pass->session_id; history.owner = pass->history_key; history.frame = pass->native_frame;
    history.width = source.render_width; history.height = source.render_height;
    std::memcpy(history.render_rect, pass->render_rect, sizeof(history.render_rect));
    std::memcpy(history.output_rect, pass->output_rect, sizeof(history.output_rect));
    return RSF_DLSS_PIPELINE_OK;
}
catch (...) { history = {}; return RSF_DLSS_PIPELINE_ERROR_RESOURCE_FAILED; }

extern "C" RSF_RUNTIME_API void rsf_native_sr_release_resources(void)
{
    rsf_native_scene_reset(); rsf_native_window_reset(); rsf_native_cpu_release(); rsf_native_composition_release();
    native_enabled.store(0, std::memory_order_release); exposure_resources().release(); rsf_native_regions_release(); history = {};
    rsf_native_translucency_release();
    std::lock_guard<std::mutex> lock(surface_guard); surface_config = {};
}

extern "C" RSF_RUNTIME_API void rsf_native_sr_set_enabled(uint32_t enabled)
{ native_enabled.store(enabled != 0, std::memory_order_release); }
extern "C" RSF_RUNTIME_API int rsf_native_sr_set_surface(uint32_t width, uint32_t height) try
{
    rsf_dlss_pipeline_status status{}; status.struct_size = sizeof(status);
    if (!width || !height || rsf_dlss_pipeline_get_status(&status) != RSF_DLSS_PIPELINE_OK || !status.running ||
        status.output_width != width || status.output_height != height) return 0;
    std::lock_guard<std::mutex> lock(surface_guard);
    surface_config = {sizeof(surface_config), 0, width, height, status.render_width, status.render_height};
    return 1;
}
catch (...) { return 0; }
extern "C" RSF_RUNTIME_API int rsf_native_sr_render_config(void* user, rsf_game_render_config* config) try
{
    (void)user;
    if (!config || config->struct_size < sizeof(*config)) return 0;
    std::lock_guard<std::mutex> lock(surface_guard);
    *config = surface_config; config->struct_size = sizeof(*config);
    config->enabled = native_enabled.load(std::memory_order_acquire);
    return 1;
}
catch (...) { return 0; }
