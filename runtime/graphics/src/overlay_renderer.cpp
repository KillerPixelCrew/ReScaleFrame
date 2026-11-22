// SPDX-License-Identifier: MIT

#include "overlay_renderer_platform.h"
#include <rescaleframe/log.h>
#include <rescaleframe/shader_compile.h>

#include <windows.h>

#include <d3d11.h>

#include <cstddef>
#include <vector>

namespace {

// Kept as source and compiled at load rather than shipped as bytecode, for the same reason as the
// motion decode pass: the cross build has no shader compiler, so a pass built from bytecode would
// exist only in the MSVC build and could not be built or read anywhere else. d3dcompiler_47 is
// present both on Windows and in a Proton prefix.
//
// Compiled as 4_0 rather than 5_0. Nothing here needs shader model 5, and 4_0 also creates on a
// feature level 10 device, which costs nothing and removes a way for this to fail on hardware
// nobody tested it on.
const char* const overlay_shader = R"(
cbuffer Params : register(b0)
{
    float2 InverseTargetSize;
    float2 Padding;
};

struct VertexIn
{
    float2 position : POSITION;
    float2 uv       : TEXCOORD0;
    float4 color    : COLOR0;
};

struct VertexOut
{
    float4 position : SV_Position;
    float2 uv       : TEXCOORD0;
    float4 color    : COLOR0;
};

VertexOut vertex_main(VertexIn input)
{
    VertexOut output;

    // Physical pixels to clip space, using the target size the caller passed rather than a
    // projection matrix. y is negated because pixel coordinates grow downwards and clip space
    // upwards.
    output.position = float4(input.position.x * InverseTargetSize.x - 1.0,
                             1.0 - input.position.y * InverseTargetSize.y,
                             0.0, 1.0);
    output.uv = input.uv;
    output.color = input.color;
    return output;
}

Texture2D    Atlas        : register(t0);
SamplerState AtlasSampler : register(s0);

float4 pixel_main(VertexOut input) : SV_Target
{
    // Both the vertex colour and the atlas are premultiplied, so a component wise product is
    // premultiplied again and the blend state is the only remaining half of it.
    return input.color * Atlas.Sample(AtlasSampler, input.uv);
}
)";

struct Constants {
    // 2 / width and 2 / height. The subtraction and the flip live in the shader.
    float inverse_target_size[2];
    float padding[2];
};
static_assert(sizeof(Constants) % 16 == 0, "constant buffers are bound in 16 byte registers");

static_assert(sizeof(rsf_overlay_vertex) == 20, "the input layout below describes this exact size");
static_assert(offsetof(rsf_overlay_vertex, u) == 8, "input layout offset");
static_assert(offsetof(rsf_overlay_vertex, color) == 16, "input layout offset");

int32_t make_pipeline(void* raw, const rsf_overlay_renderer_setup* setup,
    rsf_overlay11_pipeline* renderer) noexcept try
{
    auto* device = static_cast<ID3D11Device*>(raw);
    // Flags 0 and shader model 4_0, as before: see the note on overlay_shader.
    std::vector<uint8_t> vertex_bytecode;
    std::vector<uint8_t> pixel_bytecode;
    if (!rsf::compile_shader(overlay_shader, "overlay_renderer", "vertex_main", "vs_4_0",
                             vertex_bytecode, setup->log, setup->log_user, 0) ||
        !rsf::compile_shader(overlay_shader, "overlay_renderer", "pixel_main", "ps_4_0",
                             pixel_bytecode, setup->log, setup->log_user, 0)) {
        return RSF_OVERLAY_RENDERER_ERROR_SHADER_FAILED;
    }

    const HRESULT made_vertex_shader =
        device->CreateVertexShader(vertex_bytecode.data(), vertex_bytecode.size(), nullptr,
                                   &renderer->vertex_shader);
    const HRESULT made_pixel_shader =
        device->CreatePixelShader(pixel_bytecode.data(), pixel_bytecode.size(), nullptr,
                                  &renderer->pixel_shader);
    if (FAILED(made_vertex_shader) || FAILED(made_pixel_shader) || !renderer->vertex_shader ||
        !renderer->pixel_shader) {
        rsf::say(setup->log, setup->log_user, "overlay shaders could not be created");
        return RSF_OVERLAY_RENDERER_ERROR_SHADER_FAILED;
    }

    // egui's vertex, exactly as `rsf_overlay_vertex` lays it out. The colour is premultiplied and
    // already encoded the way the target expects, so it is read as UNORM bytes and not converted.
    const D3D11_INPUT_ELEMENT_DESC elements[] = {
        {"POSITION", 0, DXGI_FORMAT_R32G32_FLOAT, 0, 0, D3D11_INPUT_PER_VERTEX_DATA, 0},
        {"TEXCOORD", 0, DXGI_FORMAT_R32G32_FLOAT, 0, 8, D3D11_INPUT_PER_VERTEX_DATA, 0},
        {"COLOR", 0, DXGI_FORMAT_R8G8B8A8_UNORM, 0, 16, D3D11_INPUT_PER_VERTEX_DATA, 0},
    };
    const HRESULT made_layout = device->CreateInputLayout(
        elements, 3, vertex_bytecode.data(), vertex_bytecode.size(), &renderer->input_layout);
    if (FAILED(made_layout) || !renderer->input_layout) {
        rsf::say(setup->log, setup->log_user, "overlay input layout could not be created");
        return RSF_OVERLAY_RENDERER_ERROR_RESOURCE_FAILED;
    }

    D3D11_BUFFER_DESC constants{};
    constants.ByteWidth = sizeof(Constants);
    constants.Usage = D3D11_USAGE_DYNAMIC;
    constants.BindFlags = D3D11_BIND_CONSTANT_BUFFER;
    constants.CPUAccessFlags = D3D11_CPU_ACCESS_WRITE;
    if (FAILED(device->CreateBuffer(&constants, nullptr, &renderer->constants)) ||
        !renderer->constants) {
        rsf::say(setup->log, setup->log_user, "overlay constant buffer could not be created");
        return RSF_OVERLAY_RENDERER_ERROR_RESOURCE_FAILED;
    }

    // Premultiplied alpha. Straight alpha here would be wrong on every antialiased edge and every
    // glyph, in a way that reads as a font problem rather than as a blend problem.
    D3D11_BLEND_DESC blend{};
    blend.RenderTarget[0].BlendEnable = TRUE;
    blend.RenderTarget[0].SrcBlend = D3D11_BLEND_ONE;
    blend.RenderTarget[0].DestBlend = D3D11_BLEND_INV_SRC_ALPHA;
    blend.RenderTarget[0].BlendOp = D3D11_BLEND_OP_ADD;
    blend.RenderTarget[0].SrcBlendAlpha = D3D11_BLEND_ONE;
    blend.RenderTarget[0].DestBlendAlpha = D3D11_BLEND_INV_SRC_ALPHA;
    blend.RenderTarget[0].BlendOpAlpha = D3D11_BLEND_OP_ADD;
    blend.RenderTarget[0].RenderTargetWriteMask = D3D11_COLOR_WRITE_ENABLE_ALL;
    if (FAILED(device->CreateBlendState(&blend, &renderer->blend)) || !renderer->blend) {
        rsf::say(setup->log, setup->log_user, "overlay blend state could not be created");
        return RSF_OVERLAY_RENDERER_ERROR_RESOURCE_FAILED;
    }

    // The overlay sits on top of a finished image. It reads no depth and writes none, so whatever
    // depth buffer the game has is neither consulted nor damaged.
    //
    // The stencil fields are filled in even though stencil is off. Zero is not a member of
    // D3D11_STENCIL_OP or D3D11_COMPARISON_FUNC, and D3D11 range checks the descriptor it is
    // handed: there are message ids for an invalid FrontFace.StencilFailOp and friends, and the
    // CD3D11_DEPTH_STENCIL_DESC default in d3d11.h writes all of these with StencilEnable FALSE.
    // Whether the runtime really rejects a zeroed pair here could not be tried without a device,
    // so these are set to the documented defaults, which is correct either way.
    const D3D11_DEPTH_STENCILOP_DESC stencil_unused = {D3D11_STENCIL_OP_KEEP, D3D11_STENCIL_OP_KEEP,
                                                       D3D11_STENCIL_OP_KEEP,
                                                       D3D11_COMPARISON_ALWAYS};
    D3D11_DEPTH_STENCIL_DESC depth_stencil{};
    depth_stencil.DepthEnable = FALSE;
    depth_stencil.DepthWriteMask = D3D11_DEPTH_WRITE_MASK_ZERO;
    depth_stencil.DepthFunc = D3D11_COMPARISON_ALWAYS;
    depth_stencil.StencilEnable = FALSE;
    depth_stencil.StencilReadMask = D3D11_DEFAULT_STENCIL_READ_MASK;
    depth_stencil.StencilWriteMask = D3D11_DEFAULT_STENCIL_WRITE_MASK;
    depth_stencil.FrontFace = stencil_unused;
    depth_stencil.BackFace = stencil_unused;
    if (FAILED(device->CreateDepthStencilState(&depth_stencil, &renderer->depth_stencil)) ||
        !renderer->depth_stencil) {
        rsf::say(setup->log, setup->log_user, "overlay depth stencil state could not be created");
        return RSF_OVERLAY_RENDERER_ERROR_RESOURCE_FAILED;
    }

    // Culling off because egui does not promise a winding order, and scissoring on because every
    // draw call carries a clip rectangle that egui relies on.
    D3D11_RASTERIZER_DESC rasterizer{};
    rasterizer.FillMode = D3D11_FILL_SOLID;
    rasterizer.CullMode = D3D11_CULL_NONE;
    rasterizer.FrontCounterClockwise = FALSE;
    rasterizer.DepthClipEnable = TRUE;
    rasterizer.ScissorEnable = TRUE;
    rasterizer.MultisampleEnable = FALSE;
    rasterizer.AntialiasedLineEnable = FALSE;
    if (FAILED(device->CreateRasterizerState(&rasterizer, &renderer->rasterizer)) ||
        !renderer->rasterizer) {
        rsf::say(setup->log, setup->log_user, "overlay rasterizer state could not be created");
        return RSF_OVERLAY_RENDERER_ERROR_RESOURCE_FAILED;
    }

    D3D11_SAMPLER_DESC sampler{};
    sampler.Filter = D3D11_FILTER_MIN_MAG_MIP_LINEAR;
    sampler.AddressU = D3D11_TEXTURE_ADDRESS_CLAMP;
    sampler.AddressV = D3D11_TEXTURE_ADDRESS_CLAMP;
    sampler.AddressW = D3D11_TEXTURE_ADDRESS_CLAMP;
    sampler.ComparisonFunc = D3D11_COMPARISON_ALWAYS;
    sampler.MaxLOD = D3D11_FLOAT32_MAX;
    if (FAILED(device->CreateSamplerState(&sampler, &renderer->sampler)) || !renderer->sampler) {
        rsf::say(setup->log, setup->log_user, "overlay sampler could not be created");
        return RSF_OVERLAY_RENDERER_ERROR_RESOURCE_FAILED;
    }

    return RSF_OVERLAY_RENDERER_OK;
} catch (...) { return RSF_OVERLAY_RENDERER_ERROR_RESOURCE_FAILED; }

constexpr rsf_overlay11_platform platform{
    make_pipeline,
    [](rsf_overlay_renderer_log_fn log, void* user, const char* message) noexcept {
        try { log(user, message); } catch (...) {}
    },
};

static_assert(sizeof(rsf_overlay11_pipeline) == 8 * sizeof(void*));
static_assert(sizeof(rsf_overlay11_platform) == 2 * sizeof(void*) && sizeof(rsf_overlay_renderer_setup) == 24);
static_assert(D3D11_BIND_VERTEX_BUFFER == 1 && D3D11_BIND_INDEX_BUFFER == 2 && D3D11_REQ_TEXTURE2D_U_OR_V_DIMENSION == 16384);
static_assert(RSF_OVERLAY_RENDERER_ABI_VERSION == 1 && RSF_OVERLAY_RENDERER_OK == 0 &&
    RSF_OVERLAY_RENDERER_ERROR_INVALID_ARGUMENT == -1 && RSF_OVERLAY_RENDERER_ERROR_ABI_MISMATCH == -2 &&
    RSF_OVERLAY_RENDERER_ERROR_RESOURCE_FAILED == -4);

} // namespace

extern "C" rsf_overlay_renderer_result rsf_overlay_renderer_create(
    void* device, const rsf_overlay_renderer_setup* setup, rsf_overlay_renderer** out)
{
    return rsf_core_overlay11_create(device, setup, out, &platform);
}
