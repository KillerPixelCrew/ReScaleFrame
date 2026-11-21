// SPDX-License-Identifier: MIT
#include "sr_d3d11.h"
#include "sr_roundtrip_hlsl.h"
#include "sr_upscale_hlsl.h"
#include <rescaleframe/d3d11_state.h>
#include <rescaleframe/shader_compile.h>
#include <array>
#include <bit>
#include <cstring>
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

template<class View, class Desc>
void describe_view(void* raw, Desc& desc, rsf_sr_view& out, uint32_t texture2d) noexcept {
    static_assert(sizeof(Desc) <= sizeof(out.descriptor));
    static_cast<View*>(raw)->GetDesc(&desc);
    out.texture2d = uint32_t(desc.ViewDimension) == texture2d;
    out.format = desc.Format;
    std::memcpy(out.descriptor, &desc, sizeof(desc));
}

template<class Desc>
const Desc* view_template(const rsf_sr_view* source, Desc& desc) noexcept {
    if (!source) return nullptr;
    std::memcpy(&desc, source->descriptor, sizeof(desc));
    return &desc;
}

template<class T, std::size_t Size>
std::array<T*, Size> views(void* const* pointers, uint32_t count) noexcept {
    std::array<T*, Size> result{};
    for (uint32_t i = 0; i < count; ++i) result[i] = static_cast<T*>(pointers[i]);
    return result;
}

void viewport(ID3D11DeviceContext* context, rsf_sr_extent size) noexcept {
    const D3D11_VIEWPORT value{0, 0, float(size.width), float(size.height), 0, 1};
    context->RSSetViewports(1, &value);
}

const rsf_sr_d3d11 platform{
    .retain = [](void* object) { static_cast<IUnknown*>(object)->AddRef(); },
    .release = [](void* object) { static_cast<IUnknown*>(object)->Release(); },
    .as_texture = [](void* resource) -> void* {
        ComPtr<ID3D11Texture2D> texture;
        if (FAILED(static_cast<ID3D11Resource*>(resource)->QueryInterface(IID_PPV_ARGS(&texture)))) return nullptr;
        return texture.Detach();
    },
    .view_resource = [](void* view) -> void* {
        ID3D11Resource* resource{};
        static_cast<ID3D11DepthStencilView*>(view)->GetResource(&resource);
        return resource;
    },
    .texture_device = [](void* texture) -> void* {
        ID3D11Device* device{};
        static_cast<ID3D11Texture2D*>(texture)->GetDevice(&device);
        return device;
    },
    .describe_texture = [](void* texture, rsf_sr_texture* out) {
        D3D11_TEXTURE2D_DESC desc{};
        static_cast<ID3D11Texture2D*>(texture)->GetDesc(&desc);
        *out = std::bit_cast<rsf_sr_texture>(desc);
    },
    .describe_view = [](void* raw, uint32_t kind, rsf_sr_view* out) {
        if (kind == RSF_SR_RTV) {
            D3D11_RENDER_TARGET_VIEW_DESC desc{};
            describe_view<ID3D11RenderTargetView>(raw, desc, *out, D3D11_RTV_DIMENSION_TEXTURE2D);
        } else if (kind == RSF_SR_SRV) {
            D3D11_SHADER_RESOURCE_VIEW_DESC desc{};
            describe_view<ID3D11ShaderResourceView>(raw, desc, *out, D3D11_SRV_DIMENSION_TEXTURE2D);
        } else if (kind == RSF_SR_UAV) {
            D3D11_UNORDERED_ACCESS_VIEW_DESC desc{};
            describe_view<ID3D11UnorderedAccessView>(raw, desc, *out, D3D11_UAV_DIMENSION_TEXTURE2D);
        } else {
            D3D11_DEPTH_STENCIL_VIEW_DESC desc{};
            describe_view<ID3D11DepthStencilView>(raw, desc, *out, D3D11_DSV_DIMENSION_TEXTURE2D);
            out->flags = desc.Flags;
        }
    },
    .format_support = [](void* device, uint32_t format) -> uint32_t {
        UINT support{};
        return SUCCEEDED(static_cast<ID3D11Device*>(device)->CheckFormatSupport(DXGI_FORMAT(format), &support)) ? support : 0;
    },
    .texture = [](void* device, const rsf_sr_texture* input, const void* initial, uint32_t pitch) -> void* {
        const auto desc = std::bit_cast<D3D11_TEXTURE2D_DESC>(*input);
        const D3D11_SUBRESOURCE_DATA data{initial, pitch, 0};
        ComPtr<ID3D11Texture2D> texture;
        if (FAILED(static_cast<ID3D11Device*>(device)->CreateTexture2D(&desc, initial ? &data : nullptr, &texture))) return nullptr;
        return texture.Detach();
    },
    .view = [](void* raw, void* texture, uint32_t kind, const rsf_sr_view* source) -> void* {
        auto* device = static_cast<ID3D11Device*>(raw);
        auto* resource = static_cast<ID3D11Texture2D*>(texture);
        switch (kind) {
        case RSF_SR_RTV: {
            D3D11_RENDER_TARGET_VIEW_DESC desc{};
            ComPtr<ID3D11RenderTargetView> view;
            if (FAILED(device->CreateRenderTargetView(resource, view_template(source, desc), &view))) return nullptr;
            return view.Detach();
        }
        case RSF_SR_UAV: {
            D3D11_UNORDERED_ACCESS_VIEW_DESC desc{};
            ComPtr<ID3D11UnorderedAccessView> view;
            if (FAILED(device->CreateUnorderedAccessView(resource, view_template(source, desc), &view))) return nullptr;
            return view.Detach();
        }
        case RSF_SR_DSV: {
            D3D11_DEPTH_STENCIL_VIEW_DESC desc{};
            desc.Format = DXGI_FORMAT_D24_UNORM_S8_UINT;
            desc.ViewDimension = D3D11_DSV_DIMENSION_TEXTURE2D;
            ComPtr<ID3D11DepthStencilView> view;
            if (FAILED(device->CreateDepthStencilView(resource, &desc, &view))) return nullptr;
            return view.Detach();
        }
        case RSF_SR_DEPTH_SRV:
        case RSF_SR_SRV: {
            if (kind == RSF_SR_SRV && !source)
                return rsf_d3d11_create_shader_view(device, static_cast<ID3D11Resource*>(resource));
            D3D11_SHADER_RESOURCE_VIEW_DESC desc{};
            const auto* selected = view_template(source, desc);
            if (kind == RSF_SR_DEPTH_SRV) {
                desc.Format = DXGI_FORMAT_R24_UNORM_X8_TYPELESS;
                desc.ViewDimension = D3D11_SRV_DIMENSION_TEXTURE2D;
                desc.Texture2D.MipLevels = 1;
                selected = &desc;
            }
            ComPtr<ID3D11ShaderResourceView> view;
            if (FAILED(device->CreateShaderResourceView(resource, selected, &view))) return nullptr;
            return view.Detach();
        }
        default:
            return nullptr;
        }
    },
    .shader = [](void* raw, uint32_t kind, uint32_t vertex, const char* entry, rsf_sr_log log, void* user, void** out) -> int32_t {
        auto* device = static_cast<ID3D11Device*>(raw);
        const bool round_trip = kind == RSF_SR_ROUND_TRIP;
        const char* source = round_trip ? round_trip_source() : upscale_source();
        const char* name = round_trip ? "rsf_sr_roundtrip.hlsl" : "rsf_sr_upscale.hlsl";
        try {
            if (vertex) {
                ComPtr<ID3D11VertexShader> shader;
                if (!rsf::compile_vertex(device, source, name, entry, &shader, log, user)) return 0;
                *out = shader.Detach();
            } else {
                ComPtr<ID3D11PixelShader> shader;
                if (!rsf::compile_pixel(device, source, name, entry, &shader, log, user)) return 0;
                *out = shader.Detach();
            }
            return 1;
        } catch (...) {
            return -1;
        }
    },
    .constant_buffer = [](void* raw) -> void* {
        auto* device = static_cast<ID3D11Device*>(raw);
        const D3D11_BUFFER_DESC desc{16, D3D11_USAGE_DEFAULT, D3D11_BIND_CONSTANT_BUFFER, 0, 0, 0};
        ComPtr<ID3D11Buffer> buffer;
        if (FAILED(device->CreateBuffer(&desc, nullptr, &buffer))) return nullptr;
        return buffer.Detach();
    },
    .raster_state = [](void* raw) -> void* {
        auto* device = static_cast<ID3D11Device*>(raw);
        D3D11_RASTERIZER_DESC desc{};
        desc.FillMode = D3D11_FILL_SOLID;
        desc.CullMode = D3D11_CULL_NONE;
        desc.DepthClipEnable = TRUE;
        ComPtr<ID3D11RasterizerState> state;
        if (FAILED(device->CreateRasterizerState(&desc, &state))) return nullptr;
        return state.Detach();
    },
    .blend_state = [](void* raw) -> void* {
        auto* device = static_cast<ID3D11Device*>(raw);
        D3D11_BLEND_DESC desc{};
        desc.IndependentBlendEnable = FALSE;
        desc.RenderTarget[0].RenderTargetWriteMask = D3D11_COLOR_WRITE_ENABLE_ALL;
        ComPtr<ID3D11BlendState> state;
        if (FAILED(device->CreateBlendState(&desc, &state))) return nullptr;
        return state.Detach();
    },
    .depth_state = [](void* raw, uint32_t write_depth) -> void* {
        auto* device = static_cast<ID3D11Device*>(raw);
        D3D11_DEPTH_STENCIL_DESC desc{};
        desc.DepthEnable = write_depth ? TRUE : FALSE;
        desc.DepthWriteMask = write_depth ? D3D11_DEPTH_WRITE_MASK_ALL : D3D11_DEPTH_WRITE_MASK_ZERO;
        desc.DepthFunc = D3D11_COMPARISON_ALWAYS;
        ComPtr<ID3D11DepthStencilState> state;
        if (FAILED(device->CreateDepthStencilState(&desc, &state))) return nullptr;
        return state.Detach();
    },
    .save_state = [](void* raw) -> void* {
        auto state = std::unique_ptr<rsf_d3d11_state>(new (std::nothrow) rsf_d3d11_state{});
        if (!state || !rsf_d3d11_state_save(raw, state.get())) return nullptr;
        return state.release();
    },
    .restore_state = [](void* raw, void* snapshot) {
        std::unique_ptr<rsf_d3d11_state> state(static_cast<rsf_d3d11_state*>(snapshot));
        rsf_d3d11_state_restore(raw, state.get());
    },
    .update_constants = [](void* raw, void* buffer, const uint32_t* data) {
        static_cast<ID3D11DeviceContext*>(raw)->UpdateSubresource(static_cast<ID3D11Buffer*>(buffer), 0, nullptr, data, 0, 0);
    },
    .ps_constant = [](void* raw, uint32_t slot, void* buffer) {
        auto* constants = static_cast<ID3D11Buffer*>(buffer);
        static_cast<ID3D11DeviceContext*>(raw)->PSSetConstantBuffers(slot, 1, &constants);
    },
    .targets = [](void* raw, uint32_t count, void* const* targets, void* depth) {
        const auto rtvs = views<ID3D11RenderTargetView, 3>(targets, count);
        static_cast<ID3D11DeviceContext*>(raw)->OMSetRenderTargetsAndUnorderedAccessViews(
            count, rtvs.data(), static_cast<ID3D11DepthStencilView*>(depth), 0, 0, nullptr, nullptr);
    },
    .ps_resources = [](void* raw, uint32_t slot, uint32_t count, void* const* inputs) {
        const auto srvs = views<ID3D11ShaderResourceView, 6>(inputs, count);
        static_cast<ID3D11DeviceContext*>(raw)->PSSetShaderResources(slot, count, srvs.data());
    },
    .clear_stencil = [](void* raw, void* depth) {
        static_cast<ID3D11DeviceContext*>(raw)->ClearDepthStencilView(
            static_cast<ID3D11DepthStencilView*>(depth), D3D11_CLEAR_STENCIL, 1.0f, 0);
    },
    .draw = [](void* raw, const rsf_sr_draw* draw) {
        auto* context = static_cast<ID3D11DeviceContext*>(raw);
        context->IASetInputLayout(nullptr);
        context->IASetPrimitiveTopology(D3D11_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
        context->VSSetShader(static_cast<ID3D11VertexShader*>(draw->vertex), nullptr, 0);
        context->HSSetShader(nullptr, nullptr, 0);
        context->DSSetShader(nullptr, nullptr, 0);
        context->GSSetShader(nullptr, nullptr, 0);
        context->PSSetShader(static_cast<ID3D11PixelShader*>(draw->pixel), nullptr, 0);
        auto* constants = static_cast<ID3D11Buffer*>(draw->constants);
        context->PSSetConstantBuffers(0, 1, &constants);
        context->RSSetState(static_cast<ID3D11RasterizerState*>(draw->raster));
        viewport(context, draw->size);
        const float factor[4]{};
        context->OMSetBlendState(static_cast<ID3D11BlendState*>(draw->blend), factor, 0xffffffff);
        context->OMSetDepthStencilState(static_cast<ID3D11DepthStencilState*>(draw->depth), 0);
        ID3D11Predicate* predicate{};
        BOOL predicate_value{};
        context->GetPredication(&predicate, &predicate_value);
        context->SetPredication(nullptr, FALSE);
        context->Draw(3, 0);
        context->SetPredication(predicate, predicate_value);
        if (predicate) predicate->Release();
    },
};

rsf_sr_draw_views upscale_views(const UpscaleViews& v, ID3D11ShaderResourceView* reconstructed = nullptr) noexcept {
    return {{v.width, v.height}, {v.source_width, v.source_height}, {v.colour_target, v.position_target, v.distort_target},
        v.depth_target, {v.colour, v.position, v.distort, v.depth, reconstructed}, 0, 0};
}
}

const char* round_trip_source() noexcept { return rsf_sr_roundtrip_hlsl; }
const char* upscale_source() noexcept { return rsf_sr_upscale_hlsl; }
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
    out.rtv = TwinView<ID3D11RenderTargetView, RSF_SR_RTV>(owner, view.rtv);
    out.srv = TwinView<ID3D11ShaderResourceView, RSF_SR_SRV>(owner, view.srv);
    out.uav = TwinView<ID3D11UnorderedAccessView, RSF_SR_UAV>(owner, view.uav);
    auto& f = out.fields;
    f.surface = f.texture_surface = f.desc_cache = view.texture;
    f.rtv = view.rtv;
    f.srv = view.srv;
    f.uav = view.uav;
    f.width = view.size.width;
    f.height = view.size.height;
    f.format = view.format;
    std::memcpy(std::data(f.desc), &view.descriptor, sizeof(view.descriptor));
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
    if (context && width && height) viewport(context, {width, height});
}

bool RoundTripPass::create(ID3D11Device* device, rsf_sr_log log, void* user) noexcept {
    reset();
    owner_.reset(rsf_anomaly_sr_create_pass(&platform, device, RSF_SR_ROUND_TRIP, log, user));
    return ready();
}

void RoundTripPass::reset() noexcept { owner_.reset(); }

bool RoundTripPass::run(ID3D11DeviceContext* context, ID3D11ShaderResourceView* source, uint32_t sw, uint32_t sh,
    ID3D11RenderTargetView* target, uint32_t width, uint32_t height) noexcept {
    const rsf_sr_draw_views view{{width, height}, {sw, sh}, {target}, nullptr, {source}, 0, 0};
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
    const rsf_sr_draw_views view{{v.width, v.height}, {}, {v.colour_target, v.motion_target}, nullptr,
        {v.colour, v.depth, v.motion}, std::bit_cast<uint32_t>(v.first_person_depth), uint32_t(v.first_person_zero)};
    return rsf_anomaly_sr_run_pass(owner_.get(), context, RSF_SR_PREPARE, &view) != 0;
}

bool UpscalePass::reinsert(ID3D11DeviceContext* context, const UpscaleViews& views, ID3D11ShaderResourceView* reconstructed) noexcept {
    const auto view = upscale_views(views, reconstructed);
    return rsf_anomaly_sr_run_pass(owner_.get(), context, RSF_SR_REINSERT, &view) != 0;
}
}
