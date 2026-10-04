// SPDX-License-Identifier: GPL-3.0-only
// Translucency hints and cloud depth on WARP: an opaque snapshot at the first scene-colour pass,
// masks from what that pass and the offscreen layer changed, and identity-keyed handover.
#include <rescaleframe/native_translucency.h>
#include <d3d11.h>
#include <DirectXPackedVector.h>
#include <wrl/client.h>
#include <cmath>
#include <cstdio>
#include <vector>
using Microsoft::WRL::ComPtr;
using DirectX::PackedVector::XMConvertFloatToHalf;
using DirectX::PackedVector::XMConvertHalfToFloat;
namespace {
constexpr uint32_t width = 16, height = 8;
ComPtr<ID3D11Device> device; ComPtr<ID3D11DeviceContext> context;
ComPtr<ID3D11Texture2D> rgba(float r, float a)
{
    std::vector<uint16_t> texels(width * height * 4);
    for (uint32_t i = 0; i < width * height; ++i) {
        texels[i * 4] = texels[i * 4 + 1] = texels[i * 4 + 2] = XMConvertFloatToHalf(r);
        texels[i * 4 + 3] = XMConvertFloatToHalf(a);
    }
    D3D11_TEXTURE2D_DESC desc{};
    desc.Width = width; desc.Height = height; desc.MipLevels = desc.ArraySize = desc.SampleDesc.Count = 1;
    desc.Format = DXGI_FORMAT_R16G16B16A16_FLOAT; desc.BindFlags = D3D11_BIND_SHADER_RESOURCE | D3D11_BIND_RENDER_TARGET;
    D3D11_SUBRESOURCE_DATA data{}; data.pSysMem = texels.data(); data.SysMemPitch = width * 8;
    ComPtr<ID3D11Texture2D> texture; device->CreateTexture2D(&desc, &data, &texture);
    return texture;
}
void set(ID3D11Texture2D* texture, uint32_t x, uint32_t y, float r, float a)
{
    const uint16_t texel[4]{XMConvertFloatToHalf(r), XMConvertFloatToHalf(r), XMConvertFloatToHalf(r), XMConvertFloatToHalf(a)};
    const D3D11_BOX box{x, y, 0, x + 1, y + 1, 1};
    context->UpdateSubresource(texture, 0, &box, texel, 8, 8);
}
std::vector<float> read(void* pointer)
{
    auto* texture = static_cast<ID3D11Texture2D*>(pointer);
    D3D11_TEXTURE2D_DESC desc{}; texture->GetDesc(&desc);
    desc.BindFlags = 0; desc.Usage = D3D11_USAGE_STAGING; desc.CPUAccessFlags = D3D11_CPU_ACCESS_READ;
    ComPtr<ID3D11Texture2D> staging; device->CreateTexture2D(&desc, nullptr, &staging);
    context->CopyResource(staging.Get(), texture);
    D3D11_MAPPED_SUBRESOURCE mapped{}; context->Map(staging.Get(), 0, D3D11_MAP_READ, 0, &mapped);
    std::vector<float> values(desc.Width * desc.Height);
    for (uint32_t y = 0; y < desc.Height; ++y) for (uint32_t x = 0; x < desc.Width; ++x) {
        const auto* row = static_cast<const unsigned char*>(mapped.pData) + y * mapped.RowPitch;
        values[y * desc.Width + x] = desc.Format == DXGI_FORMAT_R32_FLOAT ? reinterpret_cast<const float*>(row)[x] :
            XMConvertHalfToFloat(reinterpret_cast<const uint16_t*>(row)[x * 4]);
    }
    context->Unmap(staging.Get(), 0);
    return values;
}
rsf_game_render_pass pass(uint32_t role, uint64_t frame, uint32_t flags)
{
    rsf_game_render_pass p{}; p.struct_size = sizeof(p); p.role = role; p.family_key = 1; p.view_key = 2;
    p.native_frame = frame; p.flags = RSF_GAME_RENDER_PRIMARY | flags;
    p.render_rect[0] = 4; p.render_rect[1] = 2; p.render_rect[2] = 12; p.render_rect[3] = 6;
    return p;
}
}
int main()
{
    if (FAILED(D3D11CreateDevice(nullptr, D3D_DRIVER_TYPE_WARP, nullptr, 0, nullptr, 0,
                                 D3D11_SDK_VERSION, &device, nullptr, &context))) return 77;
    bool passed = true;
    auto scene = rgba(0.5f, 1), layer = rgba(0, 1);
    auto standard = pass(RSF_GAME_RENDER_TRANSLUCENCY, 3, 0);
    standard.color_input = scene.Get();
    rsf_native_translucency_pass(context.Get(), &standard, 1);
    set(scene.Get(), 5, 3, 4.0f, 1);   // a bright translucent draw inside the rectangle
    set(scene.Get(), 0, 0, 4.0f, 1);   // outside the view rectangle
    rsf_native_translucency_pass(context.Get(), &standard, 0);
    auto separate = pass(RSF_GAME_RENDER_TRANSLUCENCY, 3, RSF_GAME_RENDER_TRANSLUCENCY_LAYER);
    separate.color_input = scene.Get();
    rsf_native_translucency_pass(context.Get(), &separate, 1);
    set(layer.Get(), 8, 4, 0, 0.2f);   // 80% opaque black smoke in the layer
    separate.color_output = layer.Get();
    rsf_native_translucency_pass(context.Get(), &separate, 0);

    auto sr = pass(RSF_GAME_RENDER_SR, 3, 0);
    rsf_native_translucency_masks masks{}; masks.struct_size = sizeof(masks);
    passed &= rsf_native_translucency_take(&sr, &masks) == 1 && masks.reactive && masks.coverage && masks.bias &&
        masks.color_before_transparency && masks.transparency_layer && !masks.motion_depth;
    if (passed) {
        const auto reactive = read(masks.reactive), coverage = read(masks.coverage), bias = read(masks.bias);
        const auto opaque = read(masks.color_before_transparency), copied = read(masks.transparency_layer);
        // Rect-local 8x4: scene (5,3) is (1,1), scene (8,4) is (4,2).
        const uint32_t drawn = 1 * 8 + 1, smoke = 2 * 8 + 4, untouched = 0;
        std::fprintf(stderr, "drawn r=%g c=%g b=%g; smoke r=%g c=%g b=%g; untouched r=%g c=%g b=%g; opaque=%g layer.a@smoke=%g\n",
            reactive[drawn], coverage[drawn], bias[drawn], reactive[smoke], coverage[smoke], bias[smoke],
            reactive[untouched], coverage[untouched], bias[untouched], opaque[drawn], copied[smoke]);
        // Tonemapped 0.5 -> 1/3 and 4 -> 4/5: a change of 0.47, coverage saturates.
        passed &= std::fabs(reactive[drawn] - (0.8f - 1.0f / 3.0f)) < 0.01f && coverage[drawn] == 1 && bias[drawn] == 1;
        // Reactive follows the 80% opacity; the visible darkening (1/3 -> 1/11) saturates coverage.
        passed &= std::fabs(reactive[smoke] - 0.8f) < 0.01f && coverage[smoke] == 1 && bias[smoke] == 1;
        passed &= reactive[untouched] == 0 && coverage[untouched] == 0 && bias[untouched] == 0;
        passed &= opaque[drawn] == 0.5f;
    }
    // Another frame's SR pass must not receive these.
    auto stale = pass(RSF_GAME_RENDER_SR, 4, 0);
    passed &= rsf_native_translucency_take(&stale, &masks) == 0;

    // Cloud depth composited before the next frame's first translucency pass is adopted by it.
    std::vector<float> depth(8 * 4, 0.0f); depth[9] = 0.25f;
    D3D11_TEXTURE2D_DESC desc{};
    desc.Width = 8; desc.Height = 4; desc.MipLevels = desc.ArraySize = desc.SampleDesc.Count = 1;
    desc.Format = DXGI_FORMAT_R32_FLOAT; desc.BindFlags = D3D11_BIND_SHADER_RESOURCE;
    D3D11_SUBRESOURCE_DATA data{}; data.pSysMem = depth.data(); data.SysMemPitch = 32;
    ComPtr<ID3D11Texture2D> clouds; device->CreateTexture2D(&desc, &data, &clouds);
    auto cloud = pass(RSF_GAME_RENDER_CLOUD_DEPTH, 0, 0); cloud.depth = clouds.Get();
    rsf_native_translucency_pass(context.Get(), &cloud, 1);
    auto next = pass(RSF_GAME_RENDER_TRANSLUCENCY, 4, 0); next.color_input = scene.Get();
    rsf_native_translucency_pass(context.Get(), &next, 1);
    rsf_native_translucency_pass(context.Get(), &next, 0);
    passed &= rsf_native_translucency_take(&stale, &masks) == 1 && masks.motion_depth && !masks.transparency_layer;
    if (masks.motion_depth) passed &= read(masks.motion_depth)[9] == 0.25f;
    // Without a fresh composite the following frame has no cloud depth.
    auto later = pass(RSF_GAME_RENDER_TRANSLUCENCY, 5, 0); later.color_input = scene.Get();
    rsf_native_translucency_pass(context.Get(), &later, 1);
    rsf_native_translucency_pass(context.Get(), &later, 0);
    auto later_sr = pass(RSF_GAME_RENDER_SR, 5, 0);
    passed &= rsf_native_translucency_take(&later_sr, &masks) == 1 && !masks.motion_depth;
    rsf_native_translucency_release();
    std::puts(passed ? "native_translucency passed" : "native_translucency FAILED");
    return passed ? 0 : 1;
}
