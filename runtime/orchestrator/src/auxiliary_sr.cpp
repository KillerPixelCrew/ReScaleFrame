// SPDX-License-Identifier: MIT
#include "auxiliary_sr.h"
#include "auxiliary_sr_platform.h"
#include "../../graphics/src/depth_scale_private.h"
#include <rescaleframe/shader_compile.h>
#include <wrl/client.h>
#include <cstddef>

extern "C" {
rsf_auxiliary_sr* rsf_auxiliary_sr_create(const rsf_aux_platform*);
void rsf_auxiliary_sr_destroy(rsf_auxiliary_sr*);
int32_t rsf_auxiliary_sr_pending(const rsf_auxiliary_sr*);
int32_t rsf_auxiliary_sr_matches(const rsf_auxiliary_sr*, uint64_t session, uint64_t frame, uint64_t view);
int32_t rsf_auxiliary_sr_reserve(rsf_auxiliary_sr*, void* context, const rsf_game_render_pass*,
    uint32_t count, const rsf_d3d11_auxiliary_surface*, rsf_game_log_fn, void*, uint32_t recovery);
int32_t rsf_auxiliary_sr_refine(rsf_auxiliary_sr*, uint32_t count, const rsf_d3d11_auxiliary_surface*);
int32_t rsf_auxiliary_sr_seal(rsf_auxiliary_sr*);
int32_t rsf_auxiliary_sr_cancel(rsf_auxiliary_sr*);
void* rsf_auxiliary_sr_original_colour(const rsf_auxiliary_sr*);
int32_t rsf_auxiliary_sr_temporal_prepare(rsf_auxiliary_sr*, uint32_t count, const rsf_d3d11_auxiliary_surface*);
int32_t rsf_auxiliary_sr_spatial_prepare(rsf_auxiliary_sr*);
int32_t rsf_auxiliary_sr_commit(rsf_auxiliary_sr*, uint32_t temporal);
int32_t rsf_auxiliary_sr_restore(rsf_auxiliary_sr*);
}

namespace {
using Microsoft::WRL::ComPtr;
static_assert(sizeof(rsf_aux_draw_constants) == 64);
static_assert(RSF_AUX_FORMAT_RGBA32_FLOAT == DXGI_FORMAT_R32G32B32A32_FLOAT);
static_assert(RSF_AUX_FORMAT_RGBA16_FLOAT == DXGI_FORMAT_R16G16B16A16_FLOAT);
static_assert(RSF_AUX_FORMAT_R11G11B10_FLOAT == DXGI_FORMAT_R11G11B10_FLOAT);
static_assert(RSF_AUX_FORMAT_RGBA8_UNORM == DXGI_FORMAT_R8G8B8A8_UNORM);
static_assert(RSF_AUX_FORMAT_RG16_FLOAT == DXGI_FORMAT_R16G16_FLOAT);
static_assert(RSF_AUX_FORMAT_R32_FLOAT == DXGI_FORMAT_R32_FLOAT);
static_assert(RSF_AUX_FORMAT_R24G8_TYPELESS == DXGI_FORMAT_R24G8_TYPELESS);
static_assert(RSF_AUX_FORMAT_R16_FLOAT == DXGI_FORMAT_R16_FLOAT);
static_assert(RSF_AUX_FORMAT_R8_UNORM == DXGI_FORMAT_R8_UNORM);
static_assert(RSF_AUX_USAGE_DEFAULT == D3D11_USAGE_DEFAULT);
static_assert(RSF_AUX_PACKED_SUPPORT_MASK == (D3D11_FORMAT_SUPPORT_TEXTURE2D |
    D3D11_FORMAT_SUPPORT_SHADER_LOAD | D3D11_FORMAT_SUPPORT_RENDER_TARGET));
static_assert(sizeof(rsf_aux_texture_facts) == 40 && offsetof(rsf_aux_texture_facts, same_device) == 20);

constexpr const char* colour_shader = R"(
Texture2D<float4> Source : register(t0);
cbuffer Params : register(b0) {
    float4 SourceRect;
    float4 OutputRect;
    float4 ValidTexels;
    float2 Jitter;
    float2 Padding;
};
float4 vertex_main(uint id : SV_VertexID) : SV_Position {
    float2 uv = float2((id << 1) & 2, id & 2);
    return float4(uv * float2(2, -2) + float2(-1, 1), 0, 1);
}
float4 pixel_main(float4 position : SV_Position) : SV_Target {
    float2 uv = (position.xy - OutputRect.xy) / (OutputRect.zw - OutputRect.xy);
    float2 p = floor(SourceRect.xy + uv * (SourceRect.zw - SourceRect.xy) - Jitter);
    return Source.Load(int3(int2(clamp(p, ValidTexels.xy, ValidTexels.zw)), 0));
}
)";

void* immediate_device(void* raw_context)
{
    auto* context = static_cast<ID3D11DeviceContext*>(raw_context);
    if (context->GetType() != D3D11_DEVICE_CONTEXT_IMMEDIATE) return nullptr;
    ID3D11Device* device{};
    context->GetDevice(&device);
    return device;
}

int32_t device_ready(void* device)
{
    return SUCCEEDED(static_cast<ID3D11Device*>(device)->GetDeviceRemovedReason());
}

int32_t describe_texture(void* raw_texture, void* device, rsf_aux_texture_facts* out)
{
    auto* texture = static_cast<ID3D11Texture2D*>(raw_texture);
    ComPtr<ID3D11Device> owner;
    D3D11_TEXTURE2D_DESC desc{};
    texture->GetDevice(&owner);
    texture->GetDesc(&desc);
    *out = {{desc.Width, desc.Height, static_cast<uint32_t>(desc.Format), desc.SampleDesc.Quality,
        (desc.BindFlags & D3D11_BIND_DEPTH_STENCIL) != 0},
        owner.Get() == device, desc.ArraySize, desc.MipLevels, desc.SampleDesc.Count, static_cast<uint32_t>(desc.Usage)};
    return 1;
}

int32_t packed_colour_support(void* device, uint32_t* flags)
{
    UINT support{};
    const auto result = static_cast<ID3D11Device*>(device)->CheckFormatSupport(
        DXGI_FORMAT_R11G11B10_FLOAT, &support);
    *flags = support;
    return SUCCEEDED(result);
}

int32_t create_pipeline(void* raw_device, rsf_game_log_fn log, void* user, rsf_aux_colour_pipeline* out) try
{
    auto* device = static_cast<ID3D11Device*>(raw_device);
    ComPtr<ID3D11VertexShader> vertex;
    ComPtr<ID3D11PixelShader> pixel;
    ComPtr<ID3D11RasterizerState> raster;
    ComPtr<ID3D11Buffer> constants;
    D3D11_RASTERIZER_DESC r{};
    r.FillMode = D3D11_FILL_SOLID;
    r.CullMode = D3D11_CULL_NONE;
    r.DepthClipEnable = TRUE;
    D3D11_BUFFER_DESC b{};
    b.ByteWidth = sizeof(rsf_aux_draw_constants);
    b.Usage = D3D11_USAGE_DEFAULT;
    b.BindFlags = D3D11_BIND_CONSTANT_BUFFER;
    if (!rsf::compile_vertex(device, colour_shader, "auxiliary_scale", "vertex_main", &vertex, log, user) ||
        !rsf::compile_pixel(device, colour_shader, "auxiliary_scale", "pixel_main", &pixel, log, user) ||
        FAILED(device->CreateRasterizerState(&r, &raster)) ||
        FAILED(device->CreateBuffer(&b, nullptr, &constants))) return 0;
    *out = {vertex.Detach(), pixel.Detach(), raster.Detach(), constants.Detach()};
    return 1;
} catch (...) { return 0; }

void* create_texture(void* raw_device, const rsf_aux_texture_info* info, uint32_t usage)
{
    D3D11_TEXTURE2D_DESC desc{};
    desc.Width = info->width;
    desc.Height = info->height;
    desc.Format = static_cast<DXGI_FORMAT>(info->format);
    desc.MipLevels = desc.ArraySize = desc.SampleDesc.Count = 1;
    desc.SampleDesc.Quality = info->sample_quality;
    desc.Usage = D3D11_USAGE_DEFAULT;
    if (usage == RSF_AUX_TEXTURE_SOURCE) desc.BindFlags = D3D11_BIND_SHADER_RESOURCE;
    if (usage == RSF_AUX_TEXTURE_OUTPUT) desc.BindFlags = D3D11_BIND_SHADER_RESOURCE | D3D11_BIND_RENDER_TARGET;
    ComPtr<ID3D11Texture2D> texture;
    return SUCCEEDED(static_cast<ID3D11Device*>(raw_device)->CreateTexture2D(&desc, nullptr, &texture)) ?
        texture.Detach() : nullptr;
}

void* create_output_view(void* device, void* texture)
{
    ComPtr<ID3D11RenderTargetView> target;
    return SUCCEEDED(static_cast<ID3D11Device*>(device)->CreateRenderTargetView(
        static_cast<ID3D11Texture2D*>(texture), nullptr, &target)) ? target.Detach() : nullptr;
}

void clear(void* context)
{
    static_cast<ID3D11DeviceContext*>(context)->ClearState();
}

void draw(void* raw_context, const rsf_aux_colour_pipeline* pipeline, void* raw_view, void* raw_target,
    const rsf_aux_draw_constants* params)
{
    auto* context = static_cast<ID3D11DeviceContext*>(raw_context);
    const auto* r = params->output;
    const D3D11_VIEWPORT viewport{r[0], r[1], r[2]-r[0], r[3]-r[1], 0, 1};
    context->RSSetViewports(1, &viewport);
    context->RSSetState(static_cast<ID3D11RasterizerState*>(pipeline->raster));
    context->IASetPrimitiveTopology(D3D11_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
    context->VSSetShader(static_cast<ID3D11VertexShader*>(pipeline->vertex), nullptr, 0);
    context->PSSetShader(static_cast<ID3D11PixelShader*>(pipeline->pixel), nullptr, 0);
    auto* target = static_cast<ID3D11RenderTargetView*>(raw_target);
    context->OMSetRenderTargets(1, &target, nullptr);
    auto* view = static_cast<ID3D11ShaderResourceView*>(raw_view);
    context->PSSetShaderResources(0, 1, &view);
    auto* buffer = static_cast<ID3D11Buffer*>(pipeline->constants);
    context->PSSetConstantBuffers(0, 1, &buffer);
    context->UpdateSubresource(buffer, 0, nullptr, params, 0, 0);
    context->Draw(3, 0);
}

int32_t depth_reserve(rsf_depth_scale* depth, void* texture)
{
    return rsf_depth_scale_reserve(depth, static_cast<ID3D11Texture2D*>(texture));
}

constexpr rsf_aux_platform platform{
    immediate_device, device_ready, describe_texture, packed_colour_support, create_pipeline, create_texture,
    rsf_d3d11_create_shader_view, create_output_view, rsf_d3d11_state_save, rsf_d3d11_state_restore,
    clear, rsf_d3d11_copy_resource, draw, rsf_depth_scale_create, depth_reserve, rsf_depth_scale_prepare,
    rsf_depth_scale_redraw, rsf_depth_scale_commit, rsf_depth_scale_restore, rsf_depth_scale_discard,
    rsf_depth_scale_destroy
};
}

// The D3D11 host owner in Rust creates its transactions from the same table.
extern "C" const rsf_aux_platform* rsf_auxiliary_sr_platform(void) { return &platform; }

AuxiliarySurfaces::AuxiliarySurfaces() : state_(rsf_auxiliary_sr_create(&platform)) {}
AuxiliarySurfaces::~AuxiliarySurfaces() { rsf_auxiliary_sr_destroy(state_); }
bool AuxiliarySurfaces::pending() const { return rsf_auxiliary_sr_pending(state_) != 0; }
bool AuxiliarySurfaces::matches(const rsf_d3d11_auxiliary_scope& scope) const {
    return rsf_auxiliary_sr_matches(state_, scope.session_id, scope.native_frame, scope.view_key) != 0;
}
bool AuxiliarySurfaces::prepare(ID3D11DeviceContext* context, const rsf_game_render_pass& pass,
    const rsf_d3d11_auxiliary_pass& inputs, rsf_game_log_fn log, void* user) {
    return reserve(context, pass, inputs, log, user) && seal();
}
bool AuxiliarySurfaces::reserve(ID3D11DeviceContext* context, const rsf_game_render_pass& pass,
    const rsf_d3d11_auxiliary_pass& inputs, rsf_game_log_fn log, void* user, bool recovery) {
    return rsf_auxiliary_sr_reserve(state_, context, &pass, inputs.surface_count, inputs.surfaces, log, user, recovery) != 0;
}
bool AuxiliarySurfaces::refine(const rsf_d3d11_auxiliary_pass& request) {
    return rsf_auxiliary_sr_refine(state_, request.surface_count, request.surfaces) != 0;
}
bool AuxiliarySurfaces::seal() { return rsf_auxiliary_sr_seal(state_) != 0; }
bool AuxiliarySurfaces::cancel() { return rsf_auxiliary_sr_cancel(state_) != 0; }
ID3D11Texture2D* AuxiliarySurfaces::original_colour() const {
    return static_cast<ID3D11Texture2D*>(rsf_auxiliary_sr_original_colour(state_));
}
bool AuxiliarySurfaces::temporal_prepare(const rsf_d3d11_auxiliary_pass& request) {
    return rsf_auxiliary_sr_temporal_prepare(state_, request.surface_count, request.surfaces) != 0;
}
bool AuxiliarySurfaces::spatial_prepare() { return rsf_auxiliary_sr_spatial_prepare(state_) != 0; }
bool AuxiliarySurfaces::commit(bool temporal) { return rsf_auxiliary_sr_commit(state_, temporal) != 0; }
bool AuxiliarySurfaces::restore() { return rsf_auxiliary_sr_restore(state_) != 0; }
