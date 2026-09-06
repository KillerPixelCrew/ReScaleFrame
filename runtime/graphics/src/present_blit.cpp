// SPDX-License-Identifier: GPL-3.0-only

#include <rescaleframe/present_blit.h>

#include <windows.h>

#include <d3d11.h>
#include <dxgi.h>

#include <cstdarg>
#include <cstdio>
#include <cstring>

namespace {

// One triangle covering the screen, generated from the vertex id so there is no vertex buffer, no
// input layout and nothing to bind or restore for geometry. A triangle rather than two: the seam
// down the diagonal of a quad costs a little and buys nothing.
const char* const blit_shader = R"(
Texture2D<float4> Source : register(t0);
SamplerState      Sampler : register(s0);

cbuffer Params : register(b0)
{
    uint  Tonemap;
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
    float3 colour = Source.Sample(Sampler, input.uv).rgb;
    if (Tonemap != 0)
    {
        // Reinhard and a gamma. Enough to see a linear image; not a grade, and not what the game
        // does. Scene colour here runs well past one, so without this the picture is nearly black.
        colour *= Exposure;
        colour = colour / (colour + 1.0);
        colour = pow(saturate(colour), 1.0 / 2.2);
    }
    return float4(colour, 1.0);
}
)";

struct Constants {
    uint32_t tonemap;
    float exposure;
    uint32_t padding[2];
};
static_assert(sizeof(Constants) % 16 == 0, "constant buffers are bound in 16 byte registers");

using compile_fn = HRESULT(WINAPI*)(LPCVOID, SIZE_T, LPCSTR, const D3D_SHADER_MACRO*, void*, LPCSTR,
                                    LPCSTR, UINT, UINT, ID3DBlob**, ID3DBlob**);

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

struct rsf_present_blit {
    ID3D11Device* device = nullptr;
    ID3D11VertexShader* vertex_shader = nullptr;
    ID3D11PixelShader* pixel_shader = nullptr;
    ID3D11SamplerState* sampler = nullptr;
    ID3D11BlendState* blend = nullptr;
    ID3D11DepthStencilState* depth = nullptr;
    ID3D11RasterizerState* raster = nullptr;
    ID3D11Buffer* constants = nullptr;
    rsf_present_blit_log_fn log = nullptr;
    void* log_user = nullptr;
};

namespace {

void say(const rsf_present_blit* blit, const char* format, ...)
{
    if (!blit || !blit->log) {
        return;
    }
    char message[512];
    va_list arguments;
    va_start(arguments, format);
    std::vsnprintf(message, sizeof(message), format, arguments);
    va_end(arguments);
    blit->log(blit->log_user, message);
}

bool compile_one(rsf_present_blit* blit, compile_fn compile, const char* entry, const char* target,
                 ID3DBlob** out)
{
    ID3DBlob* errors = nullptr;
    const HRESULT compiled = compile(blit_shader, std::strlen(blit_shader), "present_blit", nullptr,
                                     nullptr, entry, target, 0, 0, out, &errors);
    if (FAILED(compiled) || !*out) {
        say(blit, "%s did not compile: %s", entry,
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

// Everything this draw disturbs. Restored in full, because the game is mid frame and a state left
// changed here is a rendering fault somewhere else entirely, which is the hardest kind to trace
// back to its cause.
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

    for (ID3D11RenderTargetView* target : state.targets) {
        if (target) {
            target->Release();
        }
    }
    if (state.depth_view) {
        state.depth_view->Release();
    }
    if (state.vertex_shader) {
        state.vertex_shader->Release();
    }
    if (state.pixel_shader) {
        state.pixel_shader->Release();
    }
    if (state.layout) {
        state.layout->Release();
    }
    if (state.resource) {
        state.resource->Release();
    }
    if (state.sampler) {
        state.sampler->Release();
    }
    if (state.constants) {
        state.constants->Release();
    }
    if (state.blend) {
        state.blend->Release();
    }
    if (state.depth_state) {
        state.depth_state->Release();
    }
    if (state.raster) {
        state.raster->Release();
    }
}

} // namespace

extern "C" rsf_present_blit_result rsf_present_blit_create(void* device_pointer,
                                                           const rsf_present_blit_setup* setup,
                                                           rsf_present_blit** out)
{
    if (!device_pointer || !setup || !out || setup->struct_size < sizeof(rsf_present_blit_setup)) {
        return RSF_PRESENT_BLIT_ERROR_INVALID_ARGUMENT;
    }
    if (setup->abi_version != RSF_PRESENT_BLIT_ABI_VERSION) {
        return RSF_PRESENT_BLIT_ERROR_ABI_MISMATCH;
    }
    *out = nullptr;

    auto* device = static_cast<ID3D11Device*>(device_pointer);
    auto* blit = new rsf_present_blit();
    blit->device = device;
    blit->device->AddRef();
    blit->log = setup->log;
    blit->log_user = setup->log_user;

    const compile_fn compile = load_compiler();
    if (!compile) {
        say(blit, "d3dcompiler_47.dll could not be loaded from the system directory");
        rsf_present_blit_destroy(blit);
        return RSF_PRESENT_BLIT_ERROR_SHADER_FAILED;
    }

    ID3DBlob* vertex_code = nullptr;
    ID3DBlob* pixel_code = nullptr;
    if (!compile_one(blit, compile, "vertex_main", "vs_5_0", &vertex_code) ||
        !compile_one(blit, compile, "pixel_main", "ps_5_0", &pixel_code)) {
        if (vertex_code) {
            vertex_code->Release();
        }
        rsf_present_blit_destroy(blit);
        return RSF_PRESENT_BLIT_ERROR_SHADER_FAILED;
    }

    const HRESULT made_vertex = device->CreateVertexShader(
        vertex_code->GetBufferPointer(), vertex_code->GetBufferSize(), nullptr,
        &blit->vertex_shader);
    const HRESULT made_pixel = device->CreatePixelShader(
        pixel_code->GetBufferPointer(), pixel_code->GetBufferSize(), nullptr, &blit->pixel_shader);
    vertex_code->Release();
    pixel_code->Release();
    if (FAILED(made_vertex) || FAILED(made_pixel)) {
        say(blit, "the blit shaders could not be created");
        rsf_present_blit_destroy(blit);
        return RSF_PRESENT_BLIT_ERROR_SHADER_FAILED;
    }

    D3D11_SAMPLER_DESC sampler{};
    sampler.Filter = D3D11_FILTER_MIN_MAG_MIP_LINEAR;
    sampler.AddressU = D3D11_TEXTURE_ADDRESS_CLAMP;
    sampler.AddressV = D3D11_TEXTURE_ADDRESS_CLAMP;
    sampler.AddressW = D3D11_TEXTURE_ADDRESS_CLAMP;
    sampler.ComparisonFunc = D3D11_COMPARISON_NEVER;
    sampler.MaxLOD = D3D11_FLOAT32_MAX;

    D3D11_BLEND_DESC blend{};
    blend.RenderTarget[0].BlendEnable = FALSE;
    blend.RenderTarget[0].RenderTargetWriteMask = D3D11_COLOR_WRITE_ENABLE_ALL;

    // Depth off, and the faces spelled out rather than left zeroed: zero is not a valid stencil
    // operation and D3D11 range checks the whole descriptor, so a zeroed one is refused outright.
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

    D3D11_RASTERIZER_DESC raster{};
    raster.FillMode = D3D11_FILL_SOLID;
    raster.CullMode = D3D11_CULL_NONE;
    raster.DepthClipEnable = TRUE;

    D3D11_BUFFER_DESC constants{};
    constants.ByteWidth = sizeof(Constants);
    constants.Usage = D3D11_USAGE_DYNAMIC;
    constants.BindFlags = D3D11_BIND_CONSTANT_BUFFER;
    constants.CPUAccessFlags = D3D11_CPU_ACCESS_WRITE;

    if (FAILED(device->CreateSamplerState(&sampler, &blit->sampler)) ||
        FAILED(device->CreateBlendState(&blend, &blit->blend)) ||
        FAILED(device->CreateDepthStencilState(&depth, &blit->depth)) ||
        FAILED(device->CreateRasterizerState(&raster, &blit->raster)) ||
        FAILED(device->CreateBuffer(&constants, nullptr, &blit->constants))) {
        say(blit, "the blit pipeline state could not be created");
        rsf_present_blit_destroy(blit);
        return RSF_PRESENT_BLIT_ERROR_RESOURCE_FAILED;
    }

    say(blit, "present blit ready");
    *out = blit;
    return RSF_PRESENT_BLIT_OK;
}

extern "C" rsf_present_blit_result rsf_present_blit_draw(rsf_present_blit* blit,
                                                         void* context_pointer, void* swapchain,
                                                         void* source, uint32_t tonemap)
{
    if (!blit || !context_pointer || !swapchain || !source) {
        return RSF_PRESENT_BLIT_ERROR_INVALID_ARGUMENT;
    }
    auto* context = static_cast<ID3D11DeviceContext*>(context_pointer);
    auto* chain = static_cast<IDXGISwapChain*>(swapchain);

    ID3D11Texture2D* back_buffer = nullptr;
    if (FAILED(chain->GetBuffer(0, __uuidof(ID3D11Texture2D),
                                reinterpret_cast<void**>(&back_buffer))) ||
        !back_buffer) {
        return RSF_PRESENT_BLIT_ERROR_NO_BACK_BUFFER;
    }
    D3D11_TEXTURE2D_DESC target_description{};
    back_buffer->GetDesc(&target_description);

    ID3D11RenderTargetView* target = nullptr;
    const HRESULT made_target = blit->device->CreateRenderTargetView(back_buffer, nullptr, &target);
    back_buffer->Release();
    if (FAILED(made_target) || !target) {
        return RSF_PRESENT_BLIT_ERROR_RESOURCE_FAILED;
    }

    ID3D11ShaderResourceView* view = nullptr;
    if (FAILED(blit->device->CreateShaderResourceView(static_cast<ID3D11Resource*>(source), nullptr,
                                                      &view)) ||
        !view) {
        target->Release();
        return RSF_PRESENT_BLIT_ERROR_RESOURCE_FAILED;
    }

    D3D11_MAPPED_SUBRESOURCE mapped{};
    if (SUCCEEDED(context->Map(blit->constants, 0, D3D11_MAP_WRITE_DISCARD, 0, &mapped))) {
        Constants values{};
        values.tonemap = tonemap;
        values.exposure = 1.0f;
        std::memcpy(mapped.pData, &values, sizeof(values));
        context->Unmap(blit->constants, 0);
    }

    SavedState state;
    save(context, state);

    D3D11_VIEWPORT viewport{};
    viewport.Width = float(target_description.Width);
    viewport.Height = float(target_description.Height);
    viewport.MaxDepth = 1.0f;

    const FLOAT no_factor[4] = {0.0f, 0.0f, 0.0f, 0.0f};
    context->OMSetRenderTargets(1, &target, nullptr);
    context->RSSetViewports(1, &viewport);
    context->RSSetState(blit->raster);
    context->OMSetBlendState(blit->blend, no_factor, 0xffffffffu);
    context->OMSetDepthStencilState(blit->depth, 0);
    context->IASetInputLayout(nullptr);
    context->IASetPrimitiveTopology(D3D11_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
    context->VSSetShader(blit->vertex_shader, nullptr, 0);
    context->PSSetShader(blit->pixel_shader, nullptr, 0);
    context->PSSetShaderResources(0, 1, &view);
    context->PSSetSamplers(0, 1, &blit->sampler);
    context->PSSetConstantBuffers(0, 1, &blit->constants);
    context->Draw(3, 0);

    // Unbound before the restore, so the source cannot stay bound as an input while the game binds
    // it as something else.
    ID3D11ShaderResourceView* nothing = nullptr;
    context->PSSetShaderResources(0, 1, &nothing);

    restore(context, state);
    view->Release();
    target->Release();
    return RSF_PRESENT_BLIT_OK;
}

extern "C" void rsf_present_blit_destroy(rsf_present_blit* blit)
{
    if (!blit) {
        return;
    }
    if (blit->constants) {
        blit->constants->Release();
    }
    if (blit->raster) {
        blit->raster->Release();
    }
    if (blit->depth) {
        blit->depth->Release();
    }
    if (blit->blend) {
        blit->blend->Release();
    }
    if (blit->sampler) {
        blit->sampler->Release();
    }
    if (blit->pixel_shader) {
        blit->pixel_shader->Release();
    }
    if (blit->vertex_shader) {
        blit->vertex_shader->Release();
    }
    if (blit->device) {
        blit->device->Release();
    }
    delete blit;
}
