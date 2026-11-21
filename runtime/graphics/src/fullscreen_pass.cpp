/* SPDX-License-Identifier: MIT */
#include "d3d11_state_private.h"
#include "device_owner.h"
#include "fullscreen_platform.h"

#include <rescaleframe/log.h>
#include <rescaleframe/shader_compile.h>

#include <windows.h>

#include <d3d11.h>

#include <wrl/client.h>
#include <cstring>

namespace {

/* One triangle covering the target, addressed by vertex id, so there is no vertex buffer and no
   input layout to disturb. The mode decides what the pixel does with the source. */
const char* const pass_shader = R"(
Texture2D<float4> Source : register(t0);
SamplerState      Sampler : register(s0);

cbuffer Params : register(b0)
{
    uint  Mode;
    float Exposure;
    uint2 Padding;
};

struct Varying
{
    float4 position : SV_Position;
    float2 uv       : TEXCOORD0;
};

Varying vertex_main(uint id : SV_VertexID)
{
    Varying output;
    output.uv = float2((id << 1) & 2, id & 2);
    output.position = float4(output.uv * float2(2.0, -2.0) + float2(-1.0, 1.0), 0.0, 1.0);
    return output;
}

float4 pixel_main(Varying input) : SV_Target
{
    float4 source = Source.Sample(Sampler, input.uv);
    if (Mode == 1)
    {
        // Reinhard and a gamma. Enough to see a linear image; not a grade, and not what the game
        // does. Scene colour here runs well past one, so without this the picture is nearly black.
        float3 colour = source.rgb * Exposure;
        colour = colour / (colour + 1.0);
        return float4(pow(saturate(colour), 1.0 / 2.2), 1.0);
    }
    if (Mode == 2 || Mode == 4 || Mode == 5)
    {
        // Premultiplied. The colour is emitted as it is and the blend supplies
        // `ui.rgb + (1 - ui.a) * dst`, so nothing is divided by alpha here: a layer written by
        // over blends is already premultiplied, and dividing would undo exactly the thing that
        // makes it composite correctly where coverage is partial.
        //
        // The encode is the destination's, not the source's. AC7 stores its interface as linear
        // values in a plain UNORM target and applies the display transform in a later pass; the
        // back buffer this composites onto already holds transformed colour. Blending one into the
        // other without the transform is what makes the interface arrive dark.
        //
        // Applied to the premultiplied colour rather than to a divided one. That is not exact for
        // partial coverage, and it is what a compositor working in display space does; the
        // alternative costs a divide and a multiply per pixel to be differently approximate.
        if (Mode == 4)
        {
            float3 low = source.rgb * 12.92;
            float3 high = 1.055 * pow(max(source.rgb, 0.0), 1.0 / 2.4) - 0.055;
            return float4(source.rgb <= 0.0031308 ? low : high, source.a);
        }
        if (Mode == 5)
        {
            return float4(pow(max(source.rgb, 0.0), 1.0 / 2.2), source.a);
        }
        return source;
    }
    if (Mode == 3)
    {
        return float4(source.aaa, 1.0);
    }
    if (Mode == 6) return source;
    return float4(source.rgb, 1.0);
}
)";

int32_t make_resources(void* raw, const rsf_fullscreen_setup* setup, uint32_t constant_bytes,
    rsf_fullscreen_resources* pass) noexcept try
{
    auto* device = static_cast<ID3D11Device*>(raw);
    /* Flags 0: the shader has always been compiled without strictness and nothing here is new
       enough to be worth finding out what the strict mode would say about it. */
    if (!rsf::compile_vertex(device, pass_shader, "fullscreen_pass", "vertex_main",
                             &pass->vertex_shader, setup->log, setup->log_user, 0) ||
        !rsf::compile_pixel(device, pass_shader, "fullscreen_pass", "pixel_main",
                            &pass->pixel_shader, setup->log, setup->log_user, 0)) {
        rsf::say(setup->log, setup->log_user, "the fullscreen pass shaders could not be created");
        return RSF_FULLSCREEN_ERROR_SHADER_FAILED;
    }

    /* Point sampling unless asked otherwise. The composite draws a layer at the target's own extent,
       so every texel lands on its own pixel and a linear filter would only soften text that was
       rendered sharp. */
    D3D11_SAMPLER_DESC sampler{};
    sampler.Filter = D3D11_FILTER_MIN_MAG_MIP_POINT;
    sampler.AddressU = D3D11_TEXTURE_ADDRESS_CLAMP;
    sampler.AddressV = D3D11_TEXTURE_ADDRESS_CLAMP;
    sampler.AddressW = D3D11_TEXTURE_ADDRESS_CLAMP;
    sampler.ComparisonFunc = D3D11_COMPARISON_NEVER;
    sampler.MaxLOD = D3D11_FLOAT32_MAX;
    D3D11_SAMPLER_DESC linear = sampler;
    linear.Filter = D3D11_FILTER_MIN_MAG_MIP_LINEAR;

    D3D11_BLEND_DESC opaque{};
    opaque.RenderTarget[0].BlendEnable = FALSE;
    opaque.RenderTarget[0].RenderTargetWriteMask = D3D11_COLOR_WRITE_ENABLE_ALL;

    /* `final = ui.rgb + (1 - ui.a) * dst`, which is what every frame generation SDK this project
       targets asks for and is identical across all three. The alpha channel is carried through the
       same way, so compositing a layer onto a layer stays associative. */
    D3D11_BLEND_DESC premultiplied{};
    premultiplied.RenderTarget[0].BlendEnable = TRUE;
    premultiplied.RenderTarget[0].SrcBlend = D3D11_BLEND_ONE;
    premultiplied.RenderTarget[0].DestBlend = D3D11_BLEND_INV_SRC_ALPHA;
    premultiplied.RenderTarget[0].BlendOp = D3D11_BLEND_OP_ADD;
    premultiplied.RenderTarget[0].SrcBlendAlpha = D3D11_BLEND_ONE;
    premultiplied.RenderTarget[0].DestBlendAlpha = D3D11_BLEND_INV_SRC_ALPHA;
    premultiplied.RenderTarget[0].BlendOpAlpha = D3D11_BLEND_OP_ADD;
    premultiplied.RenderTarget[0].RenderTargetWriteMask = D3D11_COLOR_WRITE_ENABLE_ALL;

    /* Depth off, and the faces spelled out rather than left zeroed: zero is not a valid stencil
       operation and D3D11 range checks the whole descriptor, so a zeroed one is refused outright. */
    D3D11_DEPTH_STENCIL_DESC depth{};
    depth.DepthEnable = FALSE;
    depth.DepthWriteMask = D3D11_DEPTH_WRITE_MASK_ZERO;
    depth.DepthFunc = D3D11_COMPARISON_ALWAYS;
    depth.StencilEnable = FALSE;
    depth.StencilReadMask = D3D11_DEFAULT_STENCIL_READ_MASK;
    depth.StencilWriteMask = D3D11_DEFAULT_STENCIL_WRITE_MASK;
    depth.FrontFace.StencilFailOp = D3D11_STENCIL_OP_KEEP;
    depth.FrontFace.StencilDepthFailOp = D3D11_STENCIL_OP_KEEP;
    depth.FrontFace.StencilPassOp = D3D11_STENCIL_OP_KEEP;
    depth.FrontFace.StencilFunc = D3D11_COMPARISON_ALWAYS;
    depth.BackFace = depth.FrontFace;

    /* Scissoring off, and set rather than inherited. The pass sets its own rectangles at draw time
       and the game's may be anything at all. */
    D3D11_RASTERIZER_DESC raster{};
    raster.FillMode = D3D11_FILL_SOLID;
    raster.CullMode = D3D11_CULL_NONE;
    raster.DepthClipEnable = TRUE;
    raster.ScissorEnable = FALSE;

    D3D11_BUFFER_DESC constants{};
    constants.ByteWidth = constant_bytes;
    constants.Usage = D3D11_USAGE_DYNAMIC;
    constants.BindFlags = D3D11_BIND_CONSTANT_BUFFER;
    constants.CPUAccessFlags = D3D11_CPU_ACCESS_WRITE;

    if (FAILED(device->CreateSamplerState(&sampler, &pass->point_sampler)) ||
        FAILED(device->CreateSamplerState(&linear, &pass->linear_sampler)) ||
        FAILED(device->CreateBlendState(&opaque, &pass->opaque_blend)) ||
        FAILED(device->CreateBlendState(&premultiplied, &pass->premultiplied_blend)) ||
        FAILED(device->CreateDepthStencilState(&depth, &pass->depth)) ||
        FAILED(device->CreateRasterizerState(&raster, &pass->raster)) ||
        FAILED(device->CreateBuffer(&constants, nullptr, &pass->constants))) {
        rsf::say(setup->log, setup->log_user, "the fullscreen pass pipeline state could not be created");
        return RSF_FULLSCREEN_ERROR_RESOURCE_FAILED;
    }

    return RSF_FULLSCREEN_OK;
} catch (...) { return RSF_FULLSCREEN_ERROR_RESOURCE_FAILED; }

int32_t admit(void* raw, void* ctx, void* target, void* source)
{
    auto* device = static_cast<ID3D11Device*>(raw);
    auto* context = static_cast<ID3D11DeviceContext*>(ctx);
    return context->GetType() == D3D11_DEVICE_CONTEXT_IMMEDIATE &&
        rsf::same_device(context, device) &&
        rsf::same_device(static_cast<ID3D11RenderTargetView*>(target), device) &&
        rsf::same_device(static_cast<ID3D11ShaderResourceView*>(source), device);
}

int32_t extent(void* raw, uint32_t* size)
{
    auto* target = static_cast<ID3D11RenderTargetView*>(raw);
    Microsoft::WRL::ComPtr<ID3D11Resource> resource;
    target->GetResource(&resource);
    Microsoft::WRL::ComPtr<ID3D11Texture2D> texture;
    if (!resource || FAILED(resource.As(&texture)) || !texture) return 0;
    D3D11_RENDER_TARGET_VIEW_DESC view{};
    D3D11_TEXTURE2D_DESC desc{};
    target->GetDesc(&view);
    texture->GetDesc(&desc);
    const UINT mip = view.ViewDimension == D3D11_RTV_DIMENSION_TEXTURE2D ? view.Texture2D.MipSlice :
        view.ViewDimension == D3D11_RTV_DIMENSION_TEXTURE2DARRAY ? view.Texture2DArray.MipSlice : 0;
    if (mip >= 32 || desc.Width == 0 || desc.Height == 0) return 0;
    const auto width = desc.Width >> mip;
    const auto height = desc.Height >> mip;
    size[0] = width ? width : 1u;
    size[1] = height ? height : 1u;
    return 1;
}

int32_t draw(void* raw, void* target, void* source, const rsf_fullscreen_resources* pass,
    uint32_t width, uint32_t height, uint32_t linear, uint32_t blend, const void* constants,
    uint32_t bytes) noexcept try
{
    auto* context = static_cast<ID3D11DeviceContext*>(raw);
    D3D11_MAPPED_SUBRESOURCE mapped{};
    if (FAILED(context->Map(pass->constants, 0, D3D11_MAP_WRITE_DISCARD, 0, &mapped))) return 0;
    if (!mapped.pData) {
        context->Unmap(pass->constants, 0);
        return 0;
    }
    std::memcpy(mapped.pData, constants, bytes);
    context->Unmap(pass->constants, 0);

    // Rebinding the RTV can unbind UAVs and aliased SRVs. A private
    // snapshot also restores this draw before another pass uses the context.
    rsf_d3d11_state saved{};
    if (!rsf_d3d11_state_save_independent(context, &saved)) return 0;
    struct Restore {
        ID3D11DeviceContext* context;
        rsf_d3d11_state& saved;
        ~Restore() { rsf_d3d11_state_restore(context, &saved); }
    } restore{context, saved};

    auto* target_view = static_cast<ID3D11RenderTargetView*>(target);
    auto* source_view = static_cast<ID3D11ShaderResourceView*>(source);
    context->SetPredication(nullptr, FALSE);
    context->SOSetTargets(0, nullptr, nullptr);
    context->OMSetRenderTargets(1, &target_view, nullptr);
    context->VSSetShader(pass->vertex_shader, nullptr, 0);
    context->HSSetShader(nullptr, nullptr, 0);
    context->DSSetShader(nullptr, nullptr, 0);
    context->GSSetShader(nullptr, nullptr, 0);
    context->PSSetShader(pass->pixel_shader, nullptr, 0);
    context->IASetInputLayout(nullptr);
    context->IASetPrimitiveTopology(D3D11_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
    context->PSSetShaderResources(0, 1, &source_view);
    auto* sampler = linear ? pass->linear_sampler : pass->point_sampler;
    context->PSSetSamplers(0, 1, &sampler);
    context->PSSetConstantBuffers(0, 1, &pass->constants);
    const FLOAT factor[4]{};
    context->OMSetBlendState(blend ? pass->premultiplied_blend : pass->opaque_blend, factor, UINT32_MAX);
    context->OMSetDepthStencilState(pass->depth, 0);
    context->RSSetState(pass->raster);

    const D3D11_VIEWPORT viewport{0.0f, 0.0f, float(width), float(height), 0.0f, 1.0f};
    context->RSSetViewports(1, &viewport);
    context->Draw(3, 0);
    return 1;
} catch (...) { return 0; }

constexpr rsf_fullscreen_platform platform{
    make_resources, admit, extent, draw,
    [](rsf_fullscreen_log_fn log, void* user, const char* message) noexcept {
        try { log(user, message); } catch (...) {}
    },
};
static_assert(sizeof(rsf_fullscreen_resources) == 9 * sizeof(void*));
static_assert(sizeof(rsf_fullscreen_platform) == 5 * sizeof(void*));
static_assert(sizeof(rsf_fullscreen_setup) == 24 && sizeof(rsf_fullscreen_draw) == 20);
static_assert(RSF_FULLSCREEN_PASS_ABI_VERSION == 1 && RSF_FULLSCREEN_OK == 0 &&
    RSF_FULLSCREEN_ERROR_INVALID_ARGUMENT == -1 && RSF_FULLSCREEN_ERROR_ABI_MISMATCH == -2 &&
    RSF_FULLSCREEN_ERROR_RESOURCE_FAILED == -4);
static_assert(RSF_FULLSCREEN_FILTER_POINT == 0 && RSF_FULLSCREEN_FILTER_LINEAR == 1 &&
    RSF_FULLSCREEN_PREMULTIPLIED == 2 && RSF_FULLSCREEN_PREMULTIPLIED_SRGB == 4 &&
    RSF_FULLSCREEN_PREMULTIPLIED_GAMMA22 == 5 && RSF_FULLSCREEN_COPY_RGBA == 6);

} // namespace

const rsf_fullscreen_platform* rsf_fullscreen_native_platform()
{
    return &platform;
}

extern "C" rsf_fullscreen_result rsf_fullscreen_pass_create(
    void* device, const rsf_fullscreen_setup* setup, rsf_fullscreen_pass** out)
{
    return rsf_core_fullscreen_create(device, setup, out, &platform);
}

extern "C" rsf_fullscreen_result rsf_fullscreen_pass_draw(rsf_fullscreen_pass* pass,
    void* context, void* target, void* source, const rsf_fullscreen_draw* parameters)
{
    return rsf_core_fullscreen_draw(pass, context, target, source, parameters);
}

extern "C" void rsf_fullscreen_pass_set_filter(rsf_fullscreen_pass* pass, rsf_fullscreen_filter filter)
{
    rsf_core_fullscreen_filter(pass, filter);
}

extern "C" void rsf_fullscreen_pass_destroy(rsf_fullscreen_pass* pass)
{
    rsf_core_fullscreen_destroy(pass);
}
