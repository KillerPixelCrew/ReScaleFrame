/* SPDX-License-Identifier: GPL-3.0-only */
#include <rescaleframe/fullscreen_pass.h>

#include <windows.h>

#include <d3d11.h>
#include <d3dcompiler.h>

#include <cstdarg>
#include <cstdio>
#include <cstring>
#include <new>

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
    return float4(source.rgb, 1.0);
}
)";

struct Constants {
    uint32_t mode;
    float exposure;
    uint32_t padding[2];
};
static_assert(sizeof(Constants) % 16 == 0, "constant buffers are bound in 16 byte registers");

using compile_fn = HRESULT(WINAPI*)(LPCVOID, SIZE_T, LPCSTR, const D3D_SHADER_MACRO*, void*, LPCSTR,
                                    LPCSTR, UINT, UINT, ID3DBlob**, ID3DBlob**);

/* Loaded from the system directory rather than linked, so nothing here needs the compiler import
   library, and so a `d3dcompiler_47.dll` dropped next to the game cannot answer instead. */
compile_fn load_compiler()
{
    const HMODULE module =
        LoadLibraryExW(L"d3dcompiler_47.dll", nullptr, LOAD_LIBRARY_SEARCH_SYSTEM32);
    if (!module) {
        return nullptr;
    }
    return reinterpret_cast<compile_fn>(
        reinterpret_cast<void*>(GetProcAddress(module, "D3DCompile")));
}

} // namespace

struct rsf_fullscreen_pass {
    ID3D11Device* device = nullptr;
    ID3D11VertexShader* vertex_shader = nullptr;
    ID3D11PixelShader* pixel_shader = nullptr;
    ID3D11SamplerState* sampler = nullptr;
    /* Two blend states, because the difference between them is the whole point of the premultiplied
       mode and choosing at draw time costs nothing. */
    ID3D11BlendState* opaque_blend = nullptr;
    ID3D11BlendState* premultiplied_blend = nullptr;
    ID3D11DepthStencilState* depth = nullptr;
    ID3D11RasterizerState* raster = nullptr;
    ID3D11Buffer* constants = nullptr;
    rsf_fullscreen_log_fn log = nullptr;
    void* log_user = nullptr;
};

namespace {

void say(const rsf_fullscreen_pass* pass, const char* format, ...)
{
    char message[512];
    va_list arguments;
    if (!pass || !pass->log) {
        return;
    }
    va_start(arguments, format);
    std::vsnprintf(message, sizeof(message), format, arguments);
    va_end(arguments);
    pass->log(pass->log_user, message);
}

bool compile_one(rsf_fullscreen_pass* pass, compile_fn compile, const char* entry,
                 const char* target, ID3DBlob** out)
{
    ID3DBlob* errors = nullptr;
    const HRESULT compiled = compile(pass_shader, std::strlen(pass_shader), "fullscreen_pass",
                                     nullptr, nullptr, entry, target, 0, 0, out, &errors);
    if (FAILED(compiled) || !*out) {
        say(pass, "%s did not compile: %s", entry,
            errors ? static_cast<const char*>(errors->GetBufferPointer()) : "no message");
        if (errors) {
            errors->Release();
        }
        return false;
    }
    if (errors) {
        errors->Release();
    }
    return true;
}

/* Everything this draw disturbs. Restored in full, scissor rectangles included: the blit this was
   generalised from saved viewports and not scissors, and a scissor left from a pass that used one
   would clip the game's next draw to a rectangle nobody set. */
struct SavedState {
    ID3D11RenderTargetView* targets[D3D11_SIMULTANEOUS_RENDER_TARGET_COUNT] = {};
    ID3D11DepthStencilView* depth_view = nullptr;
    ID3D11VertexShader* vertex_shader = nullptr;
    ID3D11PixelShader* pixel_shader = nullptr;
    ID3D11InputLayout* layout = nullptr;
    D3D11_PRIMITIVE_TOPOLOGY topology = D3D11_PRIMITIVE_TOPOLOGY_UNDEFINED;
    ID3D11ShaderResourceView* resource = nullptr;
    ID3D11SamplerState* sampler = nullptr;
    ID3D11Buffer* constants = nullptr;
    ID3D11BlendState* blend = nullptr;
    FLOAT blend_factor[4] = {};
    UINT blend_mask = 0;
    ID3D11DepthStencilState* depth_state = nullptr;
    UINT stencil_reference = 0;
    ID3D11RasterizerState* raster = nullptr;
    UINT viewport_count = D3D11_VIEWPORT_AND_SCISSORRECT_OBJECT_COUNT_PER_PIPELINE;
    D3D11_VIEWPORT viewports[D3D11_VIEWPORT_AND_SCISSORRECT_OBJECT_COUNT_PER_PIPELINE] = {};
    UINT scissor_count = D3D11_VIEWPORT_AND_SCISSORRECT_OBJECT_COUNT_PER_PIPELINE;
    D3D11_RECT scissors[D3D11_VIEWPORT_AND_SCISSORRECT_OBJECT_COUNT_PER_PIPELINE] = {};
};

void save(ID3D11DeviceContext* context, SavedState& state)
{
    context->OMGetRenderTargets(D3D11_SIMULTANEOUS_RENDER_TARGET_COUNT, state.targets,
                                &state.depth_view);
    context->VSGetShader(&state.vertex_shader, nullptr, nullptr);
    context->PSGetShader(&state.pixel_shader, nullptr, nullptr);
    context->IAGetInputLayout(&state.layout);
    context->IAGetPrimitiveTopology(&state.topology);
    context->PSGetShaderResources(0, 1, &state.resource);
    context->PSGetSamplers(0, 1, &state.sampler);
    context->PSGetConstantBuffers(0, 1, &state.constants);
    context->OMGetBlendState(&state.blend, state.blend_factor, &state.blend_mask);
    context->OMGetDepthStencilState(&state.depth_state, &state.stencil_reference);
    context->RSGetState(&state.raster);
    context->RSGetViewports(&state.viewport_count, state.viewports);
    context->RSGetScissorRects(&state.scissor_count, state.scissors);
}

void restore(ID3D11DeviceContext* context, SavedState& state)
{
    context->OMSetRenderTargets(D3D11_SIMULTANEOUS_RENDER_TARGET_COUNT, state.targets,
                                state.depth_view);
    context->VSSetShader(state.vertex_shader, nullptr, 0);
    context->PSSetShader(state.pixel_shader, nullptr, 0);
    context->IASetInputLayout(state.layout);
    context->IASetPrimitiveTopology(state.topology);
    context->PSSetShaderResources(0, 1, &state.resource);
    context->PSSetSamplers(0, 1, &state.sampler);
    context->PSSetConstantBuffers(0, 1, &state.constants);
    context->OMSetBlendState(state.blend, state.blend_factor, state.blend_mask);
    context->OMSetDepthStencilState(state.depth_state, state.stencil_reference);
    context->RSSetState(state.raster);
    context->RSSetViewports(state.viewport_count, state.viewports);
    context->RSSetScissorRects(state.scissor_count, state.scissors);

    auto drop = [](auto*& item) {
        if (item) {
            item->Release();
            item = nullptr;
        }
    };
    for (auto*& target : state.targets) {
        drop(target);
    }
    drop(state.depth_view);
    drop(state.vertex_shader);
    drop(state.pixel_shader);
    drop(state.layout);
    drop(state.resource);
    drop(state.sampler);
    drop(state.constants);
    drop(state.blend);
    drop(state.depth_state);
    drop(state.raster);
}

} // namespace

extern "C" rsf_fullscreen_result rsf_fullscreen_pass_create(void* device_pointer,
                                                            const rsf_fullscreen_setup* setup,
                                                            rsf_fullscreen_pass** out)
{
    if (!device_pointer || !setup || !out || setup->struct_size < sizeof(rsf_fullscreen_setup)) {
        return RSF_FULLSCREEN_ERROR_INVALID_ARGUMENT;
    }
    if (setup->abi_version != RSF_FULLSCREEN_PASS_ABI_VERSION) {
        return RSF_FULLSCREEN_ERROR_ABI_MISMATCH;
    }
    *out = nullptr;

    auto* device = static_cast<ID3D11Device*>(device_pointer);
    auto* pass = new (std::nothrow) rsf_fullscreen_pass();
    if (!pass) {
        return RSF_FULLSCREEN_ERROR_RESOURCE_FAILED;
    }
    pass->device = device;
    pass->device->AddRef();
    pass->log = setup->log;
    pass->log_user = setup->log_user;

    const compile_fn compile = load_compiler();
    if (!compile) {
        say(pass, "d3dcompiler_47.dll could not be loaded from the system directory");
        rsf_fullscreen_pass_destroy(pass);
        return RSF_FULLSCREEN_ERROR_SHADER_FAILED;
    }

    ID3DBlob* vertex_code = nullptr;
    ID3DBlob* pixel_code = nullptr;
    if (!compile_one(pass, compile, "vertex_main", "vs_5_0", &vertex_code) ||
        !compile_one(pass, compile, "pixel_main", "ps_5_0", &pixel_code)) {
        if (vertex_code) {
            vertex_code->Release();
        }
        rsf_fullscreen_pass_destroy(pass);
        return RSF_FULLSCREEN_ERROR_SHADER_FAILED;
    }

    const HRESULT made_vertex =
        device->CreateVertexShader(vertex_code->GetBufferPointer(), vertex_code->GetBufferSize(),
                                   nullptr, &pass->vertex_shader);
    const HRESULT made_pixel = device->CreatePixelShader(
        pixel_code->GetBufferPointer(), pixel_code->GetBufferSize(), nullptr, &pass->pixel_shader);
    vertex_code->Release();
    pixel_code->Release();
    if (FAILED(made_vertex) || FAILED(made_pixel)) {
        say(pass, "the fullscreen pass shaders could not be created");
        rsf_fullscreen_pass_destroy(pass);
        return RSF_FULLSCREEN_ERROR_SHADER_FAILED;
    }

    /* Point sampling. The composite draws a layer at the target's own extent, so every texel lands
       on its own pixel and a linear filter would only soften text that was rendered sharp. */
    D3D11_SAMPLER_DESC sampler{};
    sampler.Filter = D3D11_FILTER_MIN_MAG_MIP_POINT;
    sampler.AddressU = D3D11_TEXTURE_ADDRESS_CLAMP;
    sampler.AddressV = D3D11_TEXTURE_ADDRESS_CLAMP;
    sampler.AddressW = D3D11_TEXTURE_ADDRESS_CLAMP;
    sampler.ComparisonFunc = D3D11_COMPARISON_NEVER;
    sampler.MaxLOD = D3D11_FLOAT32_MAX;

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
    constants.ByteWidth = sizeof(Constants);
    constants.Usage = D3D11_USAGE_DYNAMIC;
    constants.BindFlags = D3D11_BIND_CONSTANT_BUFFER;
    constants.CPUAccessFlags = D3D11_CPU_ACCESS_WRITE;

    if (FAILED(device->CreateSamplerState(&sampler, &pass->sampler)) ||
        FAILED(device->CreateBlendState(&opaque, &pass->opaque_blend)) ||
        FAILED(device->CreateBlendState(&premultiplied, &pass->premultiplied_blend)) ||
        FAILED(device->CreateDepthStencilState(&depth, &pass->depth)) ||
        FAILED(device->CreateRasterizerState(&raster, &pass->raster)) ||
        FAILED(device->CreateBuffer(&constants, nullptr, &pass->constants))) {
        say(pass, "the fullscreen pass pipeline state could not be created");
        rsf_fullscreen_pass_destroy(pass);
        return RSF_FULLSCREEN_ERROR_RESOURCE_FAILED;
    }

    say(pass, "fullscreen pass ready");
    *out = pass;
    return RSF_FULLSCREEN_OK;
}

extern "C" rsf_fullscreen_result rsf_fullscreen_pass_draw(rsf_fullscreen_pass* pass,
                                                          void* context_pointer, void* target,
                                                          void* source,
                                                          const rsf_fullscreen_draw* parameters)
{
    if (!pass || !context_pointer || !target || !source || !parameters ||
        parameters->struct_size < sizeof(rsf_fullscreen_draw)) {
        return RSF_FULLSCREEN_ERROR_INVALID_ARGUMENT;
    }
    if (parameters->mode > RSF_FULLSCREEN_PREMULTIPLIED_GAMMA22) {
        return RSF_FULLSCREEN_ERROR_INVALID_ARGUMENT;
    }
    auto* context = static_cast<ID3D11DeviceContext*>(context_pointer);
    auto* target_view = static_cast<ID3D11RenderTargetView*>(target);
    auto* source_view = static_cast<ID3D11ShaderResourceView*>(source);

    uint32_t width = parameters->width;
    uint32_t height = parameters->height;
    if (width == 0 || height == 0) {
        /* The target's own extent, taken from the resource behind the view rather than asked of
           the caller, because a caller that has to work it out will eventually work it out wrong. */
        ID3D11Resource* resource = nullptr;
        target_view->GetResource(&resource);
        if (!resource) {
            return RSF_FULLSCREEN_ERROR_INVALID_ARGUMENT;
        }
        ID3D11Texture2D* texture = nullptr;
        resource->QueryInterface(__uuidof(ID3D11Texture2D), reinterpret_cast<void**>(&texture));
        resource->Release();
        if (!texture) {
            return RSF_FULLSCREEN_ERROR_INVALID_ARGUMENT;
        }
        D3D11_TEXTURE2D_DESC description{};
        texture->GetDesc(&description);
        texture->Release();
        width = description.Width;
        height = description.Height;
    }

    D3D11_MAPPED_SUBRESOURCE mapped{};
    if (FAILED(context->Map(pass->constants, 0, D3D11_MAP_WRITE_DISCARD, 0, &mapped)) ||
        !mapped.pData) {
        return RSF_FULLSCREEN_ERROR_RESOURCE_FAILED;
    }
    Constants values{};
    values.mode = parameters->mode;
    values.exposure = parameters->exposure > 0.0f ? parameters->exposure : 1.0f;
    *static_cast<Constants*>(mapped.pData) = values;
    context->Unmap(pass->constants, 0);

    SavedState state;
    save(context, state);

    /* No depth stencil view, ever. This is an overlay: binding one would also mean matching its
       extent to the target's, and a mismatched pair is a binding Windows refuses and DXVK
       tolerates, which is the worst of both. */
    context->OMSetRenderTargets(1, &target_view, nullptr);
    context->VSSetShader(pass->vertex_shader, nullptr, 0);
    context->PSSetShader(pass->pixel_shader, nullptr, 0);
    context->IASetInputLayout(nullptr);
    context->IASetPrimitiveTopology(D3D11_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
    context->PSSetShaderResources(0, 1, &source_view);
    context->PSSetSamplers(0, 1, &pass->sampler);
    context->PSSetConstantBuffers(0, 1, &pass->constants);
    const FLOAT factor[4] = {0.0f, 0.0f, 0.0f, 0.0f};
    const bool premultiplied = parameters->mode == RSF_FULLSCREEN_PREMULTIPLIED ||
                               parameters->mode == RSF_FULLSCREEN_PREMULTIPLIED_SRGB ||
                               parameters->mode == RSF_FULLSCREEN_PREMULTIPLIED_GAMMA22;
    context->OMSetBlendState(premultiplied ? pass->premultiplied_blend : pass->opaque_blend, factor,
                             0xffffffffu);
    context->OMSetDepthStencilState(pass->depth, 0);
    context->RSSetState(pass->raster);

    D3D11_VIEWPORT viewport{};
    viewport.Width = static_cast<float>(width);
    viewport.Height = static_cast<float>(height);
    viewport.MaxDepth = 1.0f;
    context->RSSetViewports(1, &viewport);

    context->Draw(3, 0);

    restore(context, state);
    return RSF_FULLSCREEN_OK;
}

extern "C" void rsf_fullscreen_pass_destroy(rsf_fullscreen_pass* pass)
{
    if (!pass) {
        return;
    }
    auto drop = [](auto* item) {
        if (item) {
            item->Release();
        }
    };
    drop(pass->constants);
    drop(pass->raster);
    drop(pass->depth);
    drop(pass->premultiplied_blend);
    drop(pass->opaque_blend);
    drop(pass->sampler);
    drop(pass->pixel_shader);
    drop(pass->vertex_shader);
    drop(pass->device);
    delete pass;
}
