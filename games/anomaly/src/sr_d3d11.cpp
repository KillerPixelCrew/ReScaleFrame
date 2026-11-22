// SPDX-License-Identifier: MIT
#include "sr_d3d11.h"
#include <rescaleframe/d3d11_state.h>
#include <bit>
#include <iterator>
#include <new>

namespace rsf::anomaly::sr {
namespace {
static_assert(sizeof(D3D11_TEXTURE2D_DESC) == texture2d_desc_size && sizeof(rsf_sr_texture) == texture2d_desc_size);
static_assert(offsetof(D3D11_TEXTURE2D_DESC, MipLevels) == offsetof(rsf_sr_texture, mips));
static_assert(offsetof(D3D11_TEXTURE2D_DESC, ArraySize) == offsetof(rsf_sr_texture, layers));
static_assert(offsetof(D3D11_TEXTURE2D_DESC, Format) == offsetof(rsf_sr_texture, format));
static_assert(offsetof(D3D11_TEXTURE2D_DESC, SampleDesc) == offsetof(rsf_sr_texture, samples));
static_assert(offsetof(D3D11_TEXTURE2D_DESC, Usage) == offsetof(rsf_sr_texture, usage));
static_assert(offsetof(D3D11_TEXTURE2D_DESC, BindFlags) == offsetof(rsf_sr_texture, bindings));
static_assert(offsetof(D3D11_TEXTURE2D_DESC, CPUAccessFlags) == offsetof(rsf_sr_texture, cpu_access));
static_assert(offsetof(D3D11_TEXTURE2D_DESC, MiscFlags) == offsetof(rsf_sr_texture, misc));
static_assert(DXGI_FORMAT_UNKNOWN == 0 && DXGI_FORMAT_R16G16B16A16_FLOAT == 10 && DXGI_FORMAT_R32_FLOAT == 41);
static_assert(DXGI_FORMAT_R24G8_TYPELESS == 44 && DXGI_FORMAT_D24_UNORM_S8_UINT == 45);
static_assert(DXGI_FORMAT_R8G8B8A8_TYPELESS == 27 && DXGI_FORMAT_R8G8B8A8_UNORM == 28 &&
    DXGI_FORMAT_B8G8R8A8_UNORM == 87 && DXGI_FORMAT_B8G8R8A8_TYPELESS == 90);
static_assert(D3D11_USAGE_DEFAULT == 0 && D3D11_USAGE_IMMUTABLE == 1);
static_assert(D3D11_BIND_SHADER_RESOURCE == 0x08 && D3D11_BIND_RENDER_TARGET == 0x20 &&
    D3D11_BIND_DEPTH_STENCIL == 0x40 && D3D11_BIND_UNORDERED_ACCESS == 0x80);
static_assert((D3D11_FORMAT_SUPPORT_TEXTURE2D | D3D11_FORMAT_SUPPORT_SHADER_LOAD | D3D11_FORMAT_SUPPORT_SHADER_SAMPLE |
    D3D11_FORMAT_SUPPORT_RENDER_TARGET | D3D11_FORMAT_SUPPORT_BLENDABLE) == 0xc320 &&
    D3D11_FORMAT_SUPPORT_TYPED_UNORDERED_ACCESS_VIEW == 0x02000000);
static_assert(d3dfmt_a16b16g16r16f == 113);

const rsf_sr_d3d11 platform{
    .retain = rsf_anomaly_sr_retain,
    .release = rsf_anomaly_sr_release,
    .as_texture = rsf_anomaly_sr_as_texture,
    .view_resource = rsf_anomaly_sr_view_resource,
    .texture_device = rsf_anomaly_sr_texture_device,
    .describe_texture = rsf_anomaly_sr_describe_texture,
    .describe_view = rsf_anomaly_sr_describe_view,
    .format_support = rsf_anomaly_sr_format_support,
    .texture = rsf_anomaly_sr_texture,
    .view = [](void* device, void* texture, uint32_t kind, const rsf_sr_view* source) {
        if (kind == RSF_SR_SRV && !source) return rsf_d3d11_create_shader_view(device, texture);
        return rsf_anomaly_sr_view(device, texture, kind, source);
    },
    .shader = [](void* raw, uint32_t kind, uint32_t vertex, const char* entry, rsf_sr_log log, void* user, void** out) -> int32_t {
        struct Sink { rsf_sr_log log; void* user; } sink{log, user};
        return rsf_anomaly_sr_compile_shader(raw, kind, vertex, entry, [](void* data, const char* message) noexcept {
            const auto& sink = *static_cast<Sink*>(data);
            try { if (sink.log) sink.log(sink.user, message); }
            catch (...) { return false; }
            return true;
        }, &sink, out);
    },
    .constant_buffer = rsf_anomaly_sr_constant_buffer,
    .raster_state = rsf_anomaly_sr_raster_state,
    .blend_state = rsf_anomaly_sr_blend_state,
    .depth_state = rsf_anomaly_sr_depth_state,
    .save_state = [](void* raw) -> void* {
        auto state = std::unique_ptr<rsf_d3d11_state>(new (std::nothrow) rsf_d3d11_state{});
        if (!state || !rsf_d3d11_state_save(raw, state.get())) return nullptr;
        return state.release();
    },
    .restore_state = [](void* raw, void* snapshot) {
        std::unique_ptr<rsf_d3d11_state> state(static_cast<rsf_d3d11_state*>(snapshot));
        rsf_d3d11_state_restore(raw, state.get());
    },
    .update_constants = rsf_anomaly_sr_update_constants,
    .ps_constant = rsf_anomaly_sr_ps_constant,
    .targets = rsf_anomaly_sr_targets,
    .ps_resources = rsf_anomaly_sr_ps_resources,
    .clear_stencil = rsf_anomaly_sr_clear_stencil,
    .draw = rsf_anomaly_sr_draw,
};

}

rsf_sr_draw_views round_trip_views(void* source, uint32_t sw, uint32_t sh, void* target, uint32_t width, uint32_t height) noexcept {
    return {{width, height}, {sw, sh}, {target}, nullptr, {source}, 0, 0};
}

rsf_sr_draw_views upscale_views(const UpscaleViews& v, void* reconstructed) noexcept {
    return {{v.width, v.height}, {v.source_width, v.source_height}, {v.colour_target, v.position_target, v.distort_target},
        v.depth_target, {v.colour, v.position, v.distort, v.depth, reconstructed}, 0, 0};
}

rsf_sr_draw_views prepare_views(const PrepareViews& v) noexcept {
    return {{v.width, v.height}, {}, {v.colour_target, v.motion_target}, nullptr,
        {v.colour, v.depth, v.motion}, std::bit_cast<uint32_t>(v.first_person_depth), uint32_t(v.first_person_zero)};
}

const rsf_sr_d3d11& d3d11_platform() noexcept { return platform; }

const char* twin_error_name(TwinError error) noexcept {
    static constexpr const char* names[] = {"none", "not_texture", "shape", "format", "support", "create", "depth"};
    const auto index = static_cast<std::size_t>(error);
    return index < std::size(names) ? names[index] : "unknown";
}

TwinError Twin::create(ID3D11Device* device, const rsf_sr_twin_input& input, Twin& out) noexcept {
    out = {};
    rsf_sr_target* owner{};
    rsf_sr_target_view view{};
    const auto result = TwinError(rsf_anomaly_sr_make_twin(&platform, device, &input, &owner, &view));
    if (result != TwinError::None) return result;
    out.owner_.reset(owner);
    out.texture = ResourceView<ID3D11Texture2D>(view.texture);
    out.rtv = ResourceView<ID3D11RenderTargetView>(view.rtv);
    out.srv = ResourceView<ID3D11ShaderResourceView>(view.srv);
    out.uav = ResourceView<ID3D11UnorderedAccessView>(view.uav);
    rsf_anomaly_sr_twin_fields(&view, &out.fields);
    return result;
}

TwinError make_twin(ID3D11Device* device, ID3D11Resource* engine, bool with_uav, Twin& out) noexcept {
    return Twin::create(device, {engine, nullptr, nullptr, nullptr, {}, 0, 1, uint32_t(with_uav), 0}, out);
}

TwinError make_world_twin(ID3D11Device* device, const TargetFields& engine, uint32_t width, uint32_t height, bool promote,
    Twin& out) noexcept {
    return Twin::create(device, {engine.surface, engine.rtv, engine.srv, engine.uav, {width, height}, engine.format,
        uint32_t(promote), uint32_t(engine.uav != nullptr), 1}, out);
}

TwinError make_depth_twin(ID3D11Device* device, ID3D11DepthStencilView* engine, uint32_t output_width, uint32_t output_height,
    uint32_t width, uint32_t height, DepthTwin& out) noexcept {
    out = {};
    rsf_sr_target* owner{};
    rsf_sr_target_view view{};
    const auto result = TwinError(rsf_anomaly_sr_make_depth(&platform, device, engine, {output_width, output_height},
        {width, height}, &owner, &view));
    if (result != TwinError::None) return result;
    out.owner_.reset(owner);
    out.texture = ResourceView<ID3D11Texture2D>(view.texture);
    out.dsv = ResourceView<ID3D11DepthStencilView>(view.dsv);
    out.srv = ResourceView<ID3D11ShaderResourceView>(view.srv);
    out.width = view.size.width;
    out.height = view.size.height;
    return result;
}

TwinError make_scratch_target(ID3D11Device* device, ID3D11RenderTargetView* engine, uint32_t width, uint32_t height,
    ScratchTarget& out) noexcept {
    out = {};
    rsf_sr_target* owner{};
    rsf_sr_target_view view{};
    const auto result = TwinError(rsf_anomaly_sr_make_scratch(&platform, device, engine, {width, height}, &owner, &view));
    if (result != TwinError::None) return result;
    out.owner_.reset(owner);
    out.texture = ResourceView<ID3D11Texture2D>(view.texture);
    out.rtv = ResourceView<ID3D11RenderTargetView>(view.rtv);
    return result;
}

TwinError make_temporal_targets(ID3D11Device* device, uint32_t rw, uint32_t rh, uint32_t ow, uint32_t oh, DXGI_FORMAT format,
    TemporalTargets& out) noexcept {
    out = {};
    rsf_sr_temporal_targets* owner{};
    rsf_sr_temporal_view view{};
    const auto result = TwinError(rsf_anomaly_sr_make_temporal(&platform, device, {rw, rh}, {ow, oh}, format, &owner, &view));
    if (result != TwinError::None) return result;
    out.owner_.reset(owner);
    out.colour = ResourceView<ID3D11Texture2D>(view.colour);
    out.motion = ResourceView<ID3D11Texture2D>(view.motion);
    out.output = ResourceView<ID3D11Texture2D>(view.output);
    out.exposure = ResourceView<ID3D11Texture2D>(view.exposure);
    out.empty_motion = ResourceView<ID3D11Texture2D>(view.empty_motion);
    out.colour_rtv = ResourceView<ID3D11RenderTargetView>(view.colour_rtv);
    out.motion_rtv = ResourceView<ID3D11RenderTargetView>(view.motion_rtv);
    out.colour_srv = ResourceView<ID3D11ShaderResourceView>(view.colour_srv);
    out.motion_srv = ResourceView<ID3D11ShaderResourceView>(view.motion_srv);
    out.output_srv = ResourceView<ID3D11ShaderResourceView>(view.output_srv);
    out.empty_motion_srv = ResourceView<ID3D11ShaderResourceView>(view.empty_motion_srv);
    return result;
}

void set_full_viewport(ID3D11DeviceContext* context, uint32_t width, uint32_t height) noexcept {
    rsf_anomaly_sr_set_full_viewport(context, width, height);
}

bool RoundTripPass::create(ID3D11Device* device, rsf_sr_log log, void* user) noexcept {
    reset();
    owner_.reset(rsf_anomaly_sr_create_pass(&platform, device, RSF_SR_ROUND_TRIP, log, user));
    return ready();
}

void RoundTripPass::reset() noexcept { owner_.reset(); }

bool RoundTripPass::run(ID3D11DeviceContext* context, ID3D11ShaderResourceView* source, uint32_t sw, uint32_t sh,
    ID3D11RenderTargetView* target, uint32_t width, uint32_t height) noexcept {
    const auto view = round_trip_views(source, sw, sh, target, width, height);
    return rsf_anomaly_sr_run_pass(owner_.get(), context, RSF_SR_ROUND_TRIP, &view) != 0;
}

bool UpscalePass::create(ID3D11Device* device, rsf_sr_log log, void* user) noexcept {
    reset();
    owner_.reset(rsf_anomaly_sr_create_pass(&platform, device, RSF_SR_UPSCALE, log, user));
    return ready();
}

void UpscalePass::reset() noexcept { owner_.reset(); }

bool UpscalePass::run(ID3D11DeviceContext* context, const UpscaleViews& views) noexcept {
    const auto view = upscale_views(views);
    return rsf_anomaly_sr_run_pass(owner_.get(), context, RSF_SR_UPSCALE, &view) != 0;
}

bool UpscalePass::prepare(ID3D11DeviceContext* context, const PrepareViews& v) noexcept {
    const auto view = prepare_views(v);
    return rsf_anomaly_sr_run_pass(owner_.get(), context, RSF_SR_PREPARE, &view) != 0;
}

bool UpscalePass::reinsert(ID3D11DeviceContext* context, const UpscaleViews& views, ID3D11ShaderResourceView* reconstructed) noexcept {
    const auto view = upscale_views(views, reconstructed);
    return rsf_anomaly_sr_run_pass(owner_.get(), context, RSF_SR_REINSERT, &view) != 0;
}
}
