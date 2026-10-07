// SPDX-License-Identifier: GPL-3.0-only
#include <rescaleframe/native_translucency.h>
#include <rescaleframe/d3d11_state.h>
#include <rescaleframe/frame_tap.h>
#include <d3d11.h>
#include <d3dcompiler.h>
#include <wrl/client.h>
#include <cmath>
#include <cstring>
#include <mutex>
using Microsoft::WRL::ComPtr;
namespace {
// Mode 0 compares scene colour after a pass drawn into it with the opaque snapshot. Mode 1 adds
// the offscreen layer exactly as the game composites it (scene * a + rgb) and merges the result.
// Differences are taken after a max-channel Reinhard so HDR highlights do not saturate the mask.
constexpr char shader_source[] = R"(
Texture2D<float4> Scene : register(t0);
Texture2D<float4> Opaque : register(t1);
Texture2D<float4> Layer : register(t2);
SamplerState Linear : register(s0);
RWTexture2D<float> Reactive : register(u0);
RWTexture2D<float> Coverage : register(u1);
RWTexture2D<float> Bias : register(u2);
cbuffer Params : register(b0) {
    int2 Origin;
    uint2 Size;
    float2 SceneInvSize;
    uint Mode;
    float CoverageScale;
};
float3 tonemap(float3 c) { c = max(c, 0); return c / (1 + max(c.r, max(c.g, c.b))); }
float change(float3 a, float3 b) { float3 d = abs(tonemap(a) - tonemap(b)); return max(d.r, max(d.g, d.b)); }
[numthreads(8,8,1)] void main(uint3 id : SV_DispatchThreadID) {
    if (any(id.xy >= Size)) return;
    int2 pixel = Origin + int2(id.xy);
    float3 scene = Scene.Load(int3(pixel, 0)).rgb;
    float reactive, coverage;
    if (Mode == 0) {
        reactive = change(scene, Opaque.Load(int3(id.xy, 0)).rgb);
        coverage = saturate(reactive * CoverageScale);
    } else {
        float4 layer = Layer.SampleLevel(Linear, (float2(pixel) + 0.5) * SceneInvSize, 0);
        float opacity = saturate(1 - layer.a);
        float visible = change(scene * saturate(layer.a) + max(layer.rgb, 0), scene);
        reactive = max(Reactive[id.xy], max(visible, opacity));
        coverage = max(Coverage[id.xy], max(opacity, saturate(visible * CoverageScale)));
    }
    reactive = isfinite(reactive) ? reactive : 1;
    coverage = isfinite(coverage) ? coverage : 1;
    Reactive[id.xy] = min(reactive, 0.9);
    Coverage[id.xy] = coverage;
    Bias[id.xy] = coverage >= 0.5 ? 1 : 0;
})";
struct Constants {
    int32_t origin[2]; uint32_t size[2]; float scene_inv_size[2]; uint32_t mode; float coverage_scale;
};
DXGI_FORMAT typed(DXGI_FORMAT format)
{
    switch (format) {
    case DXGI_FORMAT_R16G16B16A16_TYPELESS: return DXGI_FORMAT_R16G16B16A16_FLOAT;
    case DXGI_FORMAT_R32G32B32A32_TYPELESS: return DXGI_FORMAT_R32G32B32A32_FLOAT;
    case DXGI_FORMAT_R10G10B10A2_TYPELESS: return DXGI_FORMAT_R10G10B10A2_UNORM;
    case DXGI_FORMAT_R8G8B8A8_TYPELESS: return DXGI_FORMAT_R8G8B8A8_UNORM;
    default: return format;
    }
}
// Hints are valid only for one native family/view/frame rectangle. The mutex protects cached
// metadata, while the public contract still restricts texture access to the graphics owner.
struct Key {
    uint64_t family = 0, view = 0, frame = 0;
    int32_t rect[4]{};
    bool operator==(const Key& other) const
    { return family == other.family && view == other.view && frame == other.frame && !std::memcmp(rect, other.rect, sizeof(rect)); }
};
struct State {
    std::mutex guard;
    ComPtr<ID3D11Device> device;
    ComPtr<ID3D11ComputeShader> shader;
    ComPtr<ID3D11Buffer> constants;
    ComPtr<ID3D11SamplerState> sampler;
    bool shader_failed = false;
    ComPtr<ID3D11Texture2D> opaque, layer, masks[3], clouds;
    ComPtr<ID3D11UnorderedAccessView> mask_views[3];
    uint32_t width = 0, height = 0;
    DXGI_FORMAT opaque_format = DXGI_FORMAT_UNKNOWN, layer_format = DXGI_FORMAT_UNKNOWN;
    Key key;
    bool snapshot = false, masks_ready = false, layer_ready = false;
    // Clouds composite before translucency without a view identity. The next scene-colour pass
    // with the same rectangle adopts them; anything else discards them.
    int32_t cloud_rect[4]{};
    bool clouds_pending = false, clouds_ready = false;
    void release()
    {
        device.Reset(); shader.Reset(); constants.Reset(); sampler.Reset(); shader_failed = false;
        opaque.Reset(); layer.Reset(); clouds.Reset();
        clouds_pending = clouds_ready = false;
        for (auto& mask : masks) mask.Reset();
        for (auto& view : mask_views) view.Reset();
        width = height = 0; opaque_format = layer_format = DXGI_FORMAT_UNKNOWN;
        key = {}; snapshot = masks_ready = layer_ready = false;
    }
};
State& state() { static State instance; return instance; }

// Cache compiler/allocation refusal until device release; compiling on every frame would stall
// rendering. Release ID3DBlob objects before unloading the compiler that owns their vtables.
bool prepare_device(State& self, ID3D11Device* device)
{
    if (self.device.Get() != device) { self.release(); self.device = device; }
    if (self.shader) return true;
    if (self.shader_failed) return false;
    self.shader_failed = true;
    HMODULE compiler = LoadLibraryExW(L"d3dcompiler_47.dll", nullptr, LOAD_LIBRARY_SEARCH_SYSTEM32);
    if (!compiler) return false;
    auto compile = reinterpret_cast<decltype(&D3DCompile)>(reinterpret_cast<void*>(GetProcAddress(compiler, "D3DCompile")));
    ComPtr<ID3DBlob> code, errors;
    const HRESULT compiled = compile ? compile(shader_source, sizeof(shader_source) - 1, "translucency_masks", nullptr,
        nullptr, "main", "cs_5_0", 0, 0, &code, &errors) : E_FAIL;
    const HRESULT created = SUCCEEDED(compiled) && code ?
        device->CreateComputeShader(code->GetBufferPointer(), code->GetBufferSize(), nullptr, &self.shader) : E_FAIL;
    // Blob vtables live in the compiler module. Drop both before unloading it.
    code.Reset(); errors.Reset(); FreeLibrary(compiler);
    if (FAILED(created)) return false;
    D3D11_BUFFER_DESC buffer{};
    buffer.ByteWidth = sizeof(Constants); buffer.Usage = D3D11_USAGE_DEFAULT; buffer.BindFlags = D3D11_BIND_CONSTANT_BUFFER;
    D3D11_SAMPLER_DESC sampler{};
    sampler.Filter = D3D11_FILTER_MIN_MAG_MIP_LINEAR;
    sampler.AddressU = sampler.AddressV = sampler.AddressW = D3D11_TEXTURE_ADDRESS_CLAMP;
    sampler.MaxLOD = D3D11_FLOAT32_MAX;
    if (FAILED(device->CreateBuffer(&buffer, nullptr, &self.constants)) ||
        FAILED(device->CreateSamplerState(&sampler, &self.sampler))) { self.shader.Reset(); return false; }
    self.shader_failed = false;
    return true;
}
bool texture(ID3D11Device* device, uint32_t width, uint32_t height, DXGI_FORMAT format, UINT bind,
    ComPtr<ID3D11Texture2D>& out)
{
    D3D11_TEXTURE2D_DESC desc{};
    desc.Width = width; desc.Height = height; desc.MipLevels = desc.ArraySize = desc.SampleDesc.Count = 1;
    desc.Format = format; desc.BindFlags = bind;
    out.Reset();
    return SUCCEEDED(device->CreateTexture2D(&desc, nullptr, &out));
}
bool prepare_targets(State& self, uint32_t width, uint32_t height, DXGI_FORMAT opaque_format)
{
    if (self.width == width && self.height == height && self.opaque_format == opaque_format) return true;
    self.width = self.height = 0;
    if (!texture(self.device.Get(), width, height, opaque_format, D3D11_BIND_SHADER_RESOURCE, self.opaque)) return false;
    for (uint32_t i = 0; i < 3; ++i) {
        if (!texture(self.device.Get(), width, height, DXGI_FORMAT_R32_FLOAT,
                D3D11_BIND_SHADER_RESOURCE | D3D11_BIND_UNORDERED_ACCESS, self.masks[i]) ||
            FAILED(self.device->CreateUnorderedAccessView(self.masks[i].Get(), nullptr, &self.mask_views[i]))) return false;
    }
    self.layer.Reset(); self.layer_format = DXGI_FORMAT_UNKNOWN;
    self.width = width; self.height = height; self.opaque_format = opaque_format;
    return true;
}
bool view(ID3D11Device* device, ID3D11Texture2D* texture, ComPtr<ID3D11ShaderResourceView>& out)
{
    D3D11_TEXTURE2D_DESC desc{}; texture->GetDesc(&desc);
    if (desc.SampleDesc.Count != 1 || desc.ArraySize != 1) return false;
    D3D11_SHADER_RESOURCE_VIEW_DESC srv{};
    srv.Format = typed(desc.Format); srv.ViewDimension = D3D11_SRV_DIMENSION_TEXTURE2D; srv.Texture2D.MipLevels = 1;
    return SUCCEEDED(device->CreateShaderResourceView(texture, &srv, &out));
}
// Mode zero initializes all masks; mode one merges a later premultiplied/transmittance layer.
// Save predication and bindings so the diagnostic compute pass cannot alter engine draws.
bool dispatch(State& self, ID3D11DeviceContext* context, ID3D11Texture2D* scene, ID3D11Texture2D* layer, uint32_t mode)
{
    D3D11_TEXTURE2D_DESC scene_desc{}; scene->GetDesc(&scene_desc);
    ComPtr<ID3D11ShaderResourceView> sources[3];
    if (!view(self.device.Get(), scene, sources[0]) || !view(self.device.Get(), self.opaque.Get(), sources[1]) ||
        (layer && !view(self.device.Get(), layer, sources[2]))) return false;
    Constants constants{};
    constants.origin[0] = self.key.rect[0]; constants.origin[1] = self.key.rect[1];
    constants.size[0] = self.width; constants.size[1] = self.height;
    constants.scene_inv_size[0] = 1.0f / float(scene_desc.Width); constants.scene_inv_size[1] = 1.0f / float(scene_desc.Height);
    constants.mode = mode;
    // A tonemapped change of a tenth already reads as fully covered.
    constants.coverage_scale = 10.0f;
    rsf_d3d11_state saved{};
    if (!rsf_d3d11_state_save(context, &saved)) return false;
    context->SetPredication(nullptr, FALSE);
    context->OMSetRenderTargets(0, nullptr, nullptr);
    context->UpdateSubresource(self.constants.Get(), 0, nullptr, &constants, 0, 0);
    ID3D11ShaderResourceView* bound[] = {sources[0].Get(), sources[1].Get(), sources[2].Get()};
    ID3D11UnorderedAccessView* targets[] = {self.mask_views[0].Get(), self.mask_views[1].Get(), self.mask_views[2].Get()};
    ID3D11Buffer* buffers[] = {self.constants.Get()};
    ID3D11SamplerState* samplers[] = {self.sampler.Get()};
    context->CSSetShader(self.shader.Get(), nullptr, 0);
    context->CSSetShaderResources(0, 3, bound);
    context->CSSetUnorderedAccessViews(0, 3, targets, nullptr);
    context->CSSetConstantBuffers(0, 1, buffers);
    context->CSSetSamplers(0, 1, samplers);
    context->Dispatch((self.width + 7) / 8, (self.height + 7) / 8, 1);
    ID3D11UnorderedAccessView* empty[3]{}; context->CSSetUnorderedAccessViews(0, 3, empty, nullptr);
    rsf_d3d11_state_restore(context, &saved);
    return true;
}
// UE 4.27 SceneVisibility.cpp:3244-3245 under temporal upscaling: the view's material texture mip
// bias is log2 of the resolution fraction plus r.ViewTextureMipBias.Offset (-0.3), no lower than
// r.ViewTextureMipBias.Min (-2). 4.18 has no such bias, so 4.18 materials sample render-size mips.
float material_mip_bias(const rsf_game_render_pass& pass)
{
    const int32_t render = pass.render_rect[2] - pass.render_rect[0], output = pass.output_rect[2] - pass.output_rect[0];
    if (render <= 0 || output <= 0 || render > output) return 0;
    return std::fmax(std::log2(float(render) / float(output)) - 0.3f, -2.0f);
}
void material_scope(const rsf_game_render_pass& pass, uint32_t begin)
{
    rsf_frame_tap_set_sampler_bias(begin ? material_mip_bias(pass) : 0.0f);
}
bool inside(const D3D11_TEXTURE2D_DESC& desc, const int32_t rect[4])
{ return rect[0] >= 0 && rect[1] >= 0 && rect[2] > rect[0] && rect[3] > rect[1] &&
    uint32_t(rect[2]) <= desc.Width && uint32_t(rect[3]) <= desc.Height; }
}

extern "C" RSF_RUNTIME_API void rsf_native_translucency_pass(void* context_pointer, const rsf_game_render_pass* pass,
    uint32_t begin) try
{
    if (!context_pointer || !pass || pass->struct_size < sizeof(*pass)) return;
    auto* context = static_cast<ID3D11DeviceContext*>(context_pointer);
    if (pass->role == RSF_GAME_RENDER_CLOUD_DEPTH && begin && pass->depth) {
        auto* clouds = static_cast<ID3D11Texture2D*>(pass->depth);
        ComPtr<ID3D11Device> device; context->GetDevice(&device);
        ComPtr<ID3D11Device> owner; clouds->GetDevice(&owner);
        D3D11_TEXTURE2D_DESC desc{}; clouds->GetDesc(&desc);
        const int32_t width = pass->render_rect[2] - pass->render_rect[0], height = pass->render_rect[3] - pass->render_rect[1];
        auto& self = state();
        std::lock_guard<std::mutex> lock(self.guard);
        self.clouds_pending = false;
        if (owner.Get() != device.Get() || desc.Format != DXGI_FORMAT_R32_FLOAT || width <= 0 || height <= 0 ||
            desc.Width != uint32_t(width) || desc.Height != uint32_t(height)) return;
        if (self.device.Get() != device.Get()) { self.release(); self.device = device; }
        D3D11_TEXTURE2D_DESC owned{};
        if (self.clouds) self.clouds->GetDesc(&owned);
        if (!self.clouds || owned.Width != desc.Width || owned.Height != desc.Height) {
            self.clouds.Reset();
            if (!texture(device.Get(), desc.Width, desc.Height, DXGI_FORMAT_R32_FLOAT, D3D11_BIND_SHADER_RESOURCE, self.clouds)) return;
        }
        context->CopyResource(self.clouds.Get(), clouds);
        std::memcpy(self.cloud_rect, pass->render_rect, sizeof(self.cloud_rect));
        self.clouds_pending = true;
        return;
    }
    if (pass->role == RSF_GAME_RENDER_MATERIALS) { material_scope(*pass, begin); return; }
    if (pass->role != RSF_GAME_RENDER_TRANSLUCENCY || !(pass->flags & RSF_GAME_RENDER_PRIMARY) || !pass->color_input) return;
    // Translucent materials take the same bias. Off before the end-of-pass masks run, on after
    // the opening snapshot; both are compute work that the bias does not touch either way.
    struct Bias {
        const rsf_game_render_pass& pass; uint32_t begin;
        ~Bias() { if (begin) material_scope(pass, 1); }
    } bias{*pass, begin};
    if (!begin) material_scope(*pass, 0);
    auto* scene = static_cast<ID3D11Texture2D*>(pass->color_input);
    ComPtr<ID3D11Device> device; context->GetDevice(&device);
    ComPtr<ID3D11Device> owner; scene->GetDevice(&owner);
    if (owner.Get() != device.Get() || context->GetType() != D3D11_DEVICE_CONTEXT_IMMEDIATE) return;
    auto& self = state();
    std::lock_guard<std::mutex> lock(self.guard);
    if (!prepare_device(self, device.Get())) return;
    Key key{pass->family_key, pass->view_key, pass->native_frame,
        {pass->render_rect[0], pass->render_rect[1], pass->render_rect[2], pass->render_rect[3]}};
    D3D11_TEXTURE2D_DESC scene_desc{}; scene->GetDesc(&scene_desc);
    if (scene_desc.SampleDesc.Count != 1 || scene_desc.ArraySize != 1 || !inside(scene_desc, key.rect)) return;
    const bool layer_pass = (pass->flags & RSF_GAME_RENDER_TRANSLUCENCY_LAYER) != 0;
    const D3D11_BOX box{UINT(key.rect[0]), UINT(key.rect[1]), 0, UINT(key.rect[2]), UINT(key.rect[3]), 1};
    if (begin) {
        if (layer_pass) return;
        // The first scene-colour pass of a frame: everything drawn so far is opaque, sky or cloud.
        self.key = key; self.snapshot = self.masks_ready = self.layer_ready = false;
        self.clouds_ready = self.clouds_pending && !std::memcmp(self.cloud_rect, key.rect, sizeof(key.rect));
        self.clouds_pending = false;
        if (!prepare_targets(self, uint32_t(key.rect[2] - key.rect[0]), uint32_t(key.rect[3] - key.rect[1]),
                typed(scene_desc.Format))) { self.width = 0; return; }
        context->CopySubresourceRegion(self.opaque.Get(), 0, 0, 0, 0, scene, 0, &box);
        self.snapshot = true;
        return;
    }
    if (!(self.key == key) || !self.snapshot) return;
    if (!layer_pass) {
        if (self.masks_ready) return; // A second scene-colour pass would compare against a stale base.
        self.masks_ready = dispatch(self, context, scene, nullptr, 0);
        return;
    }
    auto* layer = static_cast<ID3D11Texture2D*>(pass->color_output);
    if (!self.masks_ready || !layer) return;
    ComPtr<ID3D11Device> layer_owner; layer->GetDevice(&layer_owner);
    if (layer_owner.Get() != device.Get() || !dispatch(self, context, scene, layer, 1)) { self.masks_ready = false; return; }
    D3D11_TEXTURE2D_DESC layer_desc{}; layer->GetDesc(&layer_desc);
    // Only a layer at scene size shares the view rectangle; a scaled one is used for the masks only.
    if (layer_desc.Width != scene_desc.Width || layer_desc.Height != scene_desc.Height ||
        layer_desc.SampleDesc.Count != 1 || layer_desc.ArraySize != 1) return;
    if (!self.layer || self.layer_format != typed(layer_desc.Format)) {
        if (!texture(device.Get(), self.width, self.height, typed(layer_desc.Format), D3D11_BIND_SHADER_RESOURCE, self.layer)) return;
        self.layer_format = typed(layer_desc.Format);
    }
    context->CopySubresourceRegion(self.layer.Get(), 0, 0, 0, 0, layer, 0, &box);
    self.layer_ready = true;
}
catch (...) { state().masks_ready = false; }

extern "C" RSF_RUNTIME_API int rsf_native_translucency_take(const rsf_game_render_pass* sr_pass, rsf_native_translucency_masks* out)
{
    if (!sr_pass || !out || out->struct_size < sizeof(*out)) return 0;
    auto& self = state();
    std::lock_guard<std::mutex> lock(self.guard);
    const Key key{sr_pass->family_key, sr_pass->view_key, sr_pass->native_frame,
        {sr_pass->render_rect[0], sr_pass->render_rect[1], sr_pass->render_rect[2], sr_pass->render_rect[3]}};
    if (!self.masks_ready || !(self.key == key)) return 0;
    out->color_before_transparency = self.opaque.Get();
    out->transparency_layer = self.layer_ready ? self.layer.Get() : nullptr;
    out->reactive = self.masks[0].Get(); out->coverage = self.masks[1].Get(); out->bias = self.masks[2].Get();
    out->motion_depth = self.clouds_ready ? self.clouds.Get() : nullptr;
    return 1;
}

extern "C" RSF_RUNTIME_API void rsf_native_translucency_release(void)
{
    auto& self = state();
    std::lock_guard<std::mutex> lock(self.guard);
    self.release();
}
