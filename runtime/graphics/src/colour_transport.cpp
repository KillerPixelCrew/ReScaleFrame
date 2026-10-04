// SPDX-License-Identifier: GPL-3.0-only
#include <rescaleframe/colour_transport.h>
#include <rescaleframe/d3d11_state.h>
#include <d3d11.h>
#include <d3dcompiler.h>
#include <wrl/client.h>
#include <new>
using Microsoft::WRL::ComPtr;
namespace {
constexpr char shaders[] = R"(
Texture2D<float4> Source : register(t0);
Texture2D<float> Exposure : register(t1);
Texture2D<float4> Scene : register(t2);
SamplerState LinearClamp : register(s0);
RWTexture2D<float4> Result : register(u0);
cbuffer Params : register(b0) {
    uint2 Size;
    uint HasExposure;
    uint HasScene;
    float2 SceneView;
    float2 SceneTexture;
    float2 Jitter;
    float2 Padding;
};
static const float Knee = 4;
float exposure() {
    float value = HasExposure != 0 ? Exposure.Load(int3(0,0,0)) : 1;
    return isfinite(value) && value > 0 ? value : 1;
}
[numthreads(8,8,1)] void encode(uint3 id : SV_DispatchThreadID) {
    if (any(id.xy >= Size)) return;
    float4 colour = Source.Load(int3(id.xy,0));
    float3 scaled = max(isfinite(colour.rgb) ? colour.rgb : 0, 0) * exposure();
    Result[id.xy] = float4(pow(scaled / (scaled + Knee), 1 / 2.2), colour.a);
}
[numthreads(8,8,1)] void decode(uint3 id : SV_DispatchThreadID) {
    if (any(id.xy >= Size)) return;
    float4 colour = Source.Load(int3(id.xy,0));
    // Below 1 by one half-float step of the encoded value, the largest finite inverse. The
    // reconstruction saturates its display-range output at 1, so nothing above this survives it.
    float3 y = clamp(colour.rgb, 0, 0.99951);
    float3 t = pow(y, 2.2);
    float3 linearColour = Knee * t / (1 - t) / exposure();
    if (HasScene != 0) {
        // In the saturated band the output says only "at least this bright". Take the current
        // frame's own HDR value there, sampled where this output pixel lies in the jittered render
        // grid, never below the value at the start of the band.
        float2 viewUV = (float2(id.xy) + 0.5) / float2(Size) + Jitter / SceneView;
        float3 scene = max(Scene.SampleLevel(LinearClamp, viewUV * SceneView / SceneTexture, 0).rgb, 0);
        const float bandStart = 0.998;
        float tStart = pow(bandStart, 2.2);
        float floorValue = Knee * tStart / (1 - tStart) / exposure();
        float weight = saturate((max(y.r, max(y.g, y.b)) - bandStart) / 0.0015);
        linearColour = lerp(linearColour, max(scene, min(linearColour, floorValue)), weight);
    }
    Result[id.xy] = float4(linearColour, colour.a);
})";
struct Constants {
    uint32_t size[2]; uint32_t has_exposure, has_scene;
    float scene_view[2], scene_texture[2], jitter[2], padding[2];
};
struct Highlights { ID3D11Texture2D* scene = nullptr; uint32_t width = 0, height = 0; float jitter[2]{}; };
}
struct rsf_colour_transport {
    ComPtr<ID3D11Device> device;
    ComPtr<ID3D11ComputeShader> encode, decode;
    ComPtr<ID3D11Buffer> constants;
    ComPtr<ID3D11SamplerState> sampler;
    ComPtr<ID3D11Texture2D> encoded, scratch;
    ComPtr<ID3D11UnorderedAccessView> encoded_target, scratch_target;
    uint32_t encoded_width = 0, encoded_height = 0;
    D3D11_TEXTURE2D_DESC scratch_desc{};
};
namespace {
bool compile(ID3D11Device* device, const char* entry, ComPtr<ID3D11ComputeShader>& out)
{
    HMODULE module = LoadLibraryExW(L"d3dcompiler_47.dll", nullptr, LOAD_LIBRARY_SEARCH_SYSTEM32);
    if (!module) return false;
    auto compile = reinterpret_cast<decltype(&D3DCompile)>(reinterpret_cast<void*>(GetProcAddress(module, "D3DCompile")));
    ComPtr<ID3DBlob> code, errors;
    HRESULT result = compile ? compile(shaders, sizeof(shaders) - 1, "ColourTransport", nullptr, nullptr, entry,
        "cs_5_0", D3DCOMPILE_ENABLE_STRICTNESS, 0, &code, &errors) : E_FAIL;
    if (SUCCEEDED(result)) result = device->CreateComputeShader(code->GetBufferPointer(), code->GetBufferSize(), nullptr, &out);
    // Blob vtables live in the compiler module. Drop both before unloading it.
    code.Reset(); errors.Reset(); FreeLibrary(module);
    return SUCCEEDED(result);
}
DXGI_FORMAT typed(DXGI_FORMAT format)
{
    if (format == DXGI_FORMAT_R16G16B16A16_TYPELESS) return DXGI_FORMAT_R16G16B16A16_FLOAT;
    if (format == DXGI_FORMAT_R32G32B32A32_TYPELESS) return DXGI_FORMAT_R32G32B32A32_FLOAT;
    return format;
}
bool view_of(ID3D11Device* device, ID3D11Texture2D* texture, ComPtr<ID3D11ShaderResourceView>& out)
{
    D3D11_TEXTURE2D_DESC desc{}; texture->GetDesc(&desc);
    if (desc.SampleDesc.Count != 1) return false;
    D3D11_SHADER_RESOURCE_VIEW_DESC view{}; view.ViewDimension = D3D11_SRV_DIMENSION_TEXTURE2D; view.Texture2D.MipLevels = 1;
    view.Format = typed(desc.Format);
    return SUCCEEDED(device->CreateShaderResourceView(texture, &view, &out));
}
bool run(rsf_colour_transport& self, ID3D11DeviceContext* context, ID3D11ComputeShader* shader,
    ID3D11Texture2D* source, ID3D11Texture2D* exposure, ID3D11UnorderedAccessView* target, uint32_t width, uint32_t height,
    const Highlights& highlights = {})
{
    D3D11_TEXTURE2D_DESC desc{}; source->GetDesc(&desc);
    if (desc.Width < width || desc.Height < height) return false;
    ComPtr<ID3D11ShaderResourceView> source_view, exposure_view, scene_view;
    if (!view_of(self.device.Get(), source, source_view)) return false;
    if (exposure) {
        D3D11_TEXTURE2D_DESC scalar{}; exposure->GetDesc(&scalar);
        if (scalar.Format != DXGI_FORMAT_R32_FLOAT || FAILED(self.device->CreateShaderResourceView(exposure, nullptr, &exposure_view)))
            return false;
    }
    Constants constants{{width, height}, exposure_view ? 1u : 0u, 0u, {}, {}, {highlights.jitter[0], highlights.jitter[1]}, {}};
    if (highlights.scene) {
        D3D11_TEXTURE2D_DESC scene{}; highlights.scene->GetDesc(&scene);
        if (!highlights.width || !highlights.height || scene.Width < highlights.width || scene.Height < highlights.height ||
            !view_of(self.device.Get(), highlights.scene, scene_view)) return false;
        constants.has_scene = 1;
        constants.scene_view[0] = float(highlights.width); constants.scene_view[1] = float(highlights.height);
        // The view can be a sub-rectangle of a larger texture: normalise against the texture.
        constants.scene_texture[0] = float(scene.Width); constants.scene_texture[1] = float(scene.Height);
    }
    rsf_d3d11_state saved{};
    if (!rsf_d3d11_state_save(context, &saved)) return false;
    context->SetPredication(nullptr, FALSE);
    context->OMSetRenderTargets(0, nullptr, nullptr);
    context->UpdateSubresource(self.constants.Get(), 0, nullptr, &constants, 0, 0);
    ID3D11ShaderResourceView* views[] = {source_view.Get(), exposure_view.Get(), scene_view.Get()};
    ID3D11Buffer* buffers[] = {self.constants.Get()};
    ID3D11SamplerState* samplers[] = {self.sampler.Get()};
    context->CSSetShader(shader, nullptr, 0);
    context->CSSetShaderResources(0, 3, views);
    context->CSSetSamplers(0, 1, samplers);
    context->CSSetUnorderedAccessViews(0, 1, &target, nullptr);
    context->CSSetConstantBuffers(0, 1, buffers);
    context->Dispatch((width + 7) / 8, (height + 7) / 8, 1);
    ID3D11UnorderedAccessView* empty = nullptr; context->CSSetUnorderedAccessViews(0, 1, &empty, nullptr);
    rsf_d3d11_state_restore(context, &saved);
    return true;
}
}
extern "C" int rsf_colour_transport_create(void* device, rsf_colour_transport** out)
{
    if (!device || !out) return 0;
    *out = nullptr;
    auto* self = new (std::nothrow) rsf_colour_transport;
    if (!self) return 0;
    self->device = static_cast<ID3D11Device*>(device);
    D3D11_BUFFER_DESC buffer{};
    buffer.ByteWidth = sizeof(Constants); buffer.Usage = D3D11_USAGE_DEFAULT; buffer.BindFlags = D3D11_BIND_CONSTANT_BUFFER;
    D3D11_SAMPLER_DESC sampler{};
    sampler.Filter = D3D11_FILTER_MIN_MAG_MIP_LINEAR;
    sampler.AddressU = sampler.AddressV = sampler.AddressW = D3D11_TEXTURE_ADDRESS_CLAMP;
    sampler.MaxLOD = D3D11_FLOAT32_MAX;
    if (!compile(self->device.Get(), "encode", self->encode) || !compile(self->device.Get(), "decode", self->decode) ||
        FAILED(self->device->CreateBuffer(&buffer, nullptr, &self->constants)) ||
        FAILED(self->device->CreateSamplerState(&sampler, &self->sampler))) { delete self; return 0; }
    *out = self; return 1;
}
extern "C" int rsf_colour_transport_encode(rsf_colour_transport* self, void* context_pointer, void* source,
    void* exposure, uint32_t width, uint32_t height, void** encoded)
{
    if (!self || !context_pointer || !source || !encoded || !width || !height) return 0;
    if (!self->encoded || self->encoded_width != width || self->encoded_height != height) {
        self->encoded.Reset(); self->encoded_target.Reset(); self->encoded_width = self->encoded_height = 0;
        D3D11_TEXTURE2D_DESC desc{};
        desc.Width = width; desc.Height = height; desc.MipLevels = desc.ArraySize = desc.SampleDesc.Count = 1;
        desc.Format = DXGI_FORMAT_R16G16B16A16_FLOAT; desc.BindFlags = D3D11_BIND_SHADER_RESOURCE | D3D11_BIND_UNORDERED_ACCESS;
        if (FAILED(self->device->CreateTexture2D(&desc, nullptr, &self->encoded)) ||
            FAILED(self->device->CreateUnorderedAccessView(self->encoded.Get(), nullptr, &self->encoded_target))) return 0;
        self->encoded_width = width; self->encoded_height = height;
    }
    if (!run(*self, static_cast<ID3D11DeviceContext*>(context_pointer), self->encode.Get(), static_cast<ID3D11Texture2D*>(source),
            static_cast<ID3D11Texture2D*>(exposure), self->encoded_target.Get(), width, height)) return 0;
    *encoded = self->encoded.Get();
    return 1;
}
extern "C" int rsf_colour_transport_decode(rsf_colour_transport* self, void* context_pointer, void* target_pointer,
    void* exposure, uint32_t width, uint32_t height, void* scene, uint32_t scene_width, uint32_t scene_height,
    const float jitter[2])
{
    if (!self || !context_pointer || !target_pointer || !width || !height) return 0;
    auto* target = static_cast<ID3D11Texture2D*>(target_pointer);
    D3D11_TEXTURE2D_DESC desc{}; target->GetDesc(&desc);
    if (!(desc.BindFlags & D3D11_BIND_SHADER_RESOURCE)) return 0;
    if (!self->scratch || self->scratch_desc.Width != desc.Width || self->scratch_desc.Height != desc.Height ||
        self->scratch_desc.Format != desc.Format) {
        self->scratch.Reset(); self->scratch_target.Reset(); self->scratch_desc = {};
        D3D11_TEXTURE2D_DESC scratch = desc;
        scratch.MipLevels = scratch.ArraySize = 1; scratch.Usage = D3D11_USAGE_DEFAULT; scratch.CPUAccessFlags = 0; scratch.MiscFlags = 0;
        scratch.BindFlags = D3D11_BIND_SHADER_RESOURCE | D3D11_BIND_UNORDERED_ACCESS;
        D3D11_UNORDERED_ACCESS_VIEW_DESC view{}; view.ViewDimension = D3D11_UAV_DIMENSION_TEXTURE2D; view.Format = typed(desc.Format);
        if (FAILED(self->device->CreateTexture2D(&scratch, nullptr, &self->scratch)) ||
            FAILED(self->device->CreateUnorderedAccessView(self->scratch.Get(), &view, &self->scratch_target))) return 0;
        self->scratch_desc = desc;
    }
    auto* context = static_cast<ID3D11DeviceContext*>(context_pointer);
    const Highlights highlights{static_cast<ID3D11Texture2D*>(scene), scene_width, scene_height,
        {jitter ? jitter[0] : 0.0f, jitter ? jitter[1] : 0.0f}};
    if (!run(*self, context, self->decode.Get(), target, static_cast<ID3D11Texture2D*>(exposure),
            self->scratch_target.Get(), width, height, highlights)) return 0;
    const D3D11_BOX box{0, 0, 0, width, height, 1};
    context->CopySubresourceRegion(target, 0, 0, 0, 0, self->scratch.Get(), 0, &box);
    return 1;
}
extern "C" void rsf_colour_transport_destroy(rsf_colour_transport* self) { delete self; }
