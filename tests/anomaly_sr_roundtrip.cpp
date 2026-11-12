// SPDX-License-Identifier: GPL-3.0-only
// SR Stage 1 D3D11 fixture. Runs the production round-trip pass (games/anomaly/src/sr_d3d11.cpp with the embedded
// shaders/sr/rsf_sr_roundtrip.hlsl) on a real device and compares its UNORM8 output with what a stock UNORM8 target
// stores for the same tonemapped value:
//   1. every FP16 bit pattern as the twin's stored value y;
//   2. the stock curve y = f(x) over x in [0, 1e4], stored as FP16, against the stock UNORM8 of f(x) in float64;
//   3. nearest mapping when source and output differ (Stage 2 shape), state preservation, twin creation;
//   4. with the debug layer: mixed FP16 + UNORM8 MRT with a D24S8 DSV (no messages expected) and, informational only,
//      what D3D11 reports for a half-size RTV bound with a full-size DSV (stock phase_blur does that).
// Hardware first (with the debug layer when installed), then WARP; exit 77 when neither exists.
//   5. Stage 2: the seam upscale pass (shaders/sr/rsf_sr_upscale.hlsl) at 2:1, 1.5:1 and an odd 0.67 shape against a
//      float64 bilinear in f^-1 space, nearest position/depth copies, stencil clear, identity with the Stage 1 round
//      trip at scale 1.0, state preservation, render-size world twins and the R24G8 depth twin, and the debug layer.
// This proves the passes and their numerics, not the in-game swap.
#include "sr_d3d11.h"
#include "sr_tonemap.h"
#include <rescaleframe/shader_compile.h>

#include <d3d11sdklayers.h>
#include <dxgi.h>
#include <algorithm>
#include <cmath>
#include <cstdio>
#include <cstring>
#include <limits>
#include <vector>

namespace {
using namespace rsf::anomaly::sr;
using Microsoft::WRL::ComPtr;
int failures = 0;
#define CHECK(condition) do { if (!(condition)) { std::printf("FAIL %s:%d %s\n", __FILE__, __LINE__, #condition); ++failures; } } while (0)

float half_to_float(uint16_t h) {
    const uint32_t sign = uint32_t(h & 0x8000) << 16, exponent = (h >> 10) & 0x1f, mantissa = h & 0x3ff;
    uint32_t bits;
    if (exponent == 0x1f) bits = sign | 0x7f800000 | (mantissa << 13);
    else if (exponent) bits = sign | ((exponent + 112) << 23) | (mantissa << 13);
    else if (!mantissa) bits = sign;
    else {
        int e = -1;
        uint32_t m = mantissa;
        do { ++e; m <<= 1; } while (!(m & 0x400));
        bits = sign | uint32_t(112 - e) << 23 | ((m & 0x3ff) << 13);
    }
    float value;
    std::memcpy(&value, &bits, sizeof(value));
    return value;
}
// Round to nearest even, as a GPU stores a float shader output into an FP16 target.
uint16_t float_to_half(float value) {
    uint32_t bits;
    std::memcpy(&bits, &value, sizeof(bits));
    const uint16_t sign = uint16_t((bits >> 16) & 0x8000);
    const uint32_t magnitude = bits & 0x7fffffff;
    if (magnitude >= 0x7f800000) return uint16_t(sign | 0x7c00 | (magnitude > 0x7f800000 ? 0x200 : 0));
    if (magnitude >= 0x477ff000) return uint16_t(sign | 0x7c00);   // rounds above 65504
    if (magnitude < 0x38800000) {                                    // subnormal half
        const float scaled = std::fabs(value) * 16777216.0f;          // 2^24
        return uint16_t(sign | uint16_t(std::nearbyint(scaled)));
    }
    const uint32_t rebased = magnitude - (112u << 23);
    const uint32_t rounded = rebased + 0xfff + ((rebased >> 13) & 1);
    return uint16_t(sign | (rounded >> 13));
}

struct Device {
    ComPtr<ID3D11Device> device;
    ComPtr<ID3D11DeviceContext> context;
    ComPtr<ID3D11InfoQueue> info;
    bool debug{};
    bool create() {
        const D3D_FEATURE_LEVEL levels[] = {D3D_FEATURE_LEVEL_11_0};
        for (const auto type : {D3D_DRIVER_TYPE_HARDWARE, D3D_DRIVER_TYPE_WARP}) {
            for (const UINT flags : {UINT(D3D11_CREATE_DEVICE_DEBUG), 0u}) {
                if (SUCCEEDED(D3D11CreateDevice(nullptr, type, nullptr, flags, levels, 1, D3D11_SDK_VERSION, &device, nullptr, &context))) {
                    debug = flags != 0;
                    if (debug) device.As(&info);
                    ComPtr<IDXGIDevice> dxgi;
                    ComPtr<IDXGIAdapter> adapter;
                    DXGI_ADAPTER_DESC desc{};
                    if (SUCCEEDED(device.As(&dxgi)) && SUCCEEDED(dxgi->GetAdapter(&adapter))) adapter->GetDesc(&desc);
                    std::printf("device: %ls (%s, debug layer %s)\n", desc.Description,
                        type == D3D_DRIVER_TYPE_HARDWARE ? "hardware" : "WARP", debug ? "on" : "unavailable");
                    return true;
                }
            }
        }
        return false;
    }
    UINT64 errors() {
        if (!info) return 0;
        UINT64 count = 0;
        for (UINT64 i = 0, n = info->GetNumStoredMessages(); i < n; ++i) {
            SIZE_T size = 0;
            info->GetMessage(i, nullptr, &size);
            std::vector<char> storage(size);
            auto* message = reinterpret_cast<D3D11_MESSAGE*>(storage.data());
            if (SUCCEEDED(info->GetMessage(i, message, &size)) &&
                (message->Severity == D3D11_MESSAGE_SEVERITY_ERROR || message->Severity == D3D11_MESSAGE_SEVERITY_CORRUPTION)) {
                std::printf("  debug layer: %.*s\n", int(message->DescriptionByteLength), message->pDescription);
                ++count;
            }
        }
        info->ClearStoredMessages();
        return count;
    }
};

ComPtr<ID3D11Texture2D> texture(ID3D11Device* device, uint32_t w, uint32_t h, DXGI_FORMAT format, UINT bind,
    const void* data = nullptr, UINT pitch = 0, UINT samples = 1) {
    D3D11_TEXTURE2D_DESC desc{w, h, 1, 1, format, {samples, 0}, D3D11_USAGE_DEFAULT, bind, 0, 0};
    D3D11_SUBRESOURCE_DATA initial{data, pitch, 0};
    ComPtr<ID3D11Texture2D> result;
    device->CreateTexture2D(&desc, data ? &initial : nullptr, &result);
    return result;
}
std::vector<uint8_t> read_rgba8(Device& d, ID3D11Texture2D* source, uint32_t w, uint32_t h) {
    D3D11_TEXTURE2D_DESC desc{w, h, 1, 1, DXGI_FORMAT_R8G8B8A8_UNORM, {1, 0}, D3D11_USAGE_STAGING, 0, D3D11_CPU_ACCESS_READ, 0};
    ComPtr<ID3D11Texture2D> staging;
    std::vector<uint8_t> pixels(size_t(w) * h * 4);
    if (FAILED(d.device->CreateTexture2D(&desc, nullptr, &staging))) return {};
    d.context->CopyResource(staging.Get(), source);
    D3D11_MAPPED_SUBRESOURCE mapped{};
    if (FAILED(d.context->Map(staging.Get(), 0, D3D11_MAP_READ, 0, &mapped))) return {};
    for (uint32_t y = 0; y < h; ++y) std::memcpy(&pixels[size_t(y) * w * 4], static_cast<uint8_t*>(mapped.pData) + size_t(y) * mapped.RowPitch, size_t(w) * 4);
    d.context->Unmap(staging.Get(), 0);
    return pixels;
}
// Runs the pass on an FP16 source given as half bit patterns (4 per pixel) and returns the UNORM8 output.
std::vector<uint8_t> run_pass(Device& d, RoundTripPass& pass, const std::vector<uint16_t>& halves, uint32_t w, uint32_t h,
    uint32_t out_w, uint32_t out_h) {
    auto source = texture(d.device.Get(), w, h, DXGI_FORMAT_R16G16B16A16_FLOAT, D3D11_BIND_SHADER_RESOURCE | D3D11_BIND_RENDER_TARGET,
        halves.data(), w * 8);
    auto target = texture(d.device.Get(), out_w, out_h, DXGI_FORMAT_R8G8B8A8_UNORM, D3D11_BIND_SHADER_RESOURCE | D3D11_BIND_RENDER_TARGET);
    ComPtr<ID3D11ShaderResourceView> srv;
    ComPtr<ID3D11RenderTargetView> rtv;
    if (!source || !target || FAILED(d.device->CreateShaderResourceView(source.Get(), nullptr, &srv)) ||
        FAILED(d.device->CreateRenderTargetView(target.Get(), nullptr, &rtv))) return {};
    const float clear[4]{0.25f, 0.5f, 0.75f, 1};
    d.context->ClearRenderTargetView(rtv.Get(), clear);
    if (!pass.run(d.context.Get(), srv.Get(), w, h, rtv.Get(), out_w, out_h)) return {};
    return read_rgba8(d, target.Get(), out_w, out_h);
}

void every_half(Device& d, RoundTripPass& pass) {
    constexpr uint32_t w = 256, h = 256;
    std::vector<uint16_t> halves(size_t(w) * h * 4);
    for (uint32_t i = 0; i < w * h; ++i) {
        const auto bits = uint16_t(i);
        halves[i * 4 + 0] = halves[i * 4 + 1] = halves[i * 4 + 2] = halves[i * 4 + 3] = bits;
    }
    const auto out = run_pass(d, pass, halves, w, h, w, h);
    CHECK(out.size() == size_t(w) * h * 4);
    if (out.empty()) return;
    int worst = 0;
    uint64_t exact = 0, compared = 0, nans = 0;
    for (uint32_t i = 0; i < w * h; ++i) {
        const float y = half_to_float(uint16_t(i));
        const int expected = std::isnan(y) ? 0 : unorm8(y);   // stock UNORM8 conversion: NaN -> 0, saturate, round
        if (std::isnan(y)) ++nans;
        for (int c = 0; c < 4; ++c) {
            const int diff = std::abs(int(out[size_t(i) * 4 + c]) - expected);
            worst = std::max(worst, diff);
            exact += diff == 0;
            ++compared;
        }
    }
    std::printf("every FP16 pattern: %llu channels, %llu exact, worst %d code value(s), %llu NaN patterns\n",
        static_cast<unsigned long long>(compared), static_cast<unsigned long long>(exact), worst, static_cast<unsigned long long>(nans));
    CHECK(worst <= 1);
}

void stock_curve(Device& d, RoundTripPass& pass) {
    constexpr uint32_t w = 256, h = 256;
    std::vector<uint16_t> halves(size_t(w) * h * 4);
    std::vector<double> x(size_t(w) * h);
    for (uint32_t i = 0; i < w * h; ++i) {
        // First half linear over [0, 2] where UNORM8 codes are dense, second half logarithmic up to 1e4.
        x[i] = i < w * h / 2 ? 2.0 * i / (w * h / 2) : std::pow(10.0, std::log10(2.0) + (4.0 - std::log10(2.0)) * (i - w * h / 2) / (w * h / 2 - 1));
        const auto y = float_to_half(float(tonemap_forward(x[i])));
        halves[i * 4 + 0] = halves[i * 4 + 1] = halves[i * 4 + 2] = y;
        halves[i * 4 + 3] = float_to_half(1.0f);
    }
    const auto out = run_pass(d, pass, halves, w, h, w, h);
    CHECK(!out.empty());
    if (out.empty()) return;
    int worst = 0;
    uint64_t exact = 0;
    double worst_x = 0;
    for (uint32_t i = 0; i < w * h; ++i) {
        const int expected = unorm8(tonemap_forward(x[i]));   // stock: combine_1 output f(x) written to UNORM8
        const int diff = std::abs(int(out[size_t(i) * 4]) - expected);
        if (diff > worst) { worst = diff; worst_x = x[i]; }
        exact += diff == 0;
        CHECK(out[size_t(i) * 4 + 3] == 255);
    }
    std::printf("stock curve x in [0, 1e4]: %u samples, %llu exact, worst %d code value(s) at x=%.6g\n", w * h,
        static_cast<unsigned long long>(exact), worst, worst_x);
    CHECK(worst <= 1);
    // CPU reference of the inverse itself (same check as .local/anomaly/sr-seam/tonemap_inverse_check.py).
    double worst_inverse = 0;
    for (double value = 1e-6; value <= 20.0; value *= 1.01)
        worst_inverse = std::max(worst_inverse, std::fabs(tonemap_inverse(tonemap_forward(value)) - value) / value);
    std::printf("float64 f^-1(f(x)) relative error up to x=20: %.3g\n", worst_inverse);
    CHECK(worst_inverse < 1e-12);
}

void nearest_mapping(Device& d, RoundTripPass& pass) {
    constexpr uint32_t w = 4, h = 4;
    std::vector<uint16_t> halves(size_t(w) * h * 4);
    for (uint32_t i = 0; i < w * h; ++i) {
        halves[i * 4 + 0] = float_to_half(float(tonemap_forward(tonemap_inverse((i + 1) / 17.0))));
        halves[i * 4 + 1] = halves[i * 4 + 2] = 0;
        halves[i * 4 + 3] = float_to_half(1.0f);
    }
    const auto out = run_pass(d, pass, halves, w, h, 8, 8);
    CHECK(out.size() == 8u * 8 * 4);
    if (out.empty()) return;
    for (uint32_t y = 0; y < 8; ++y)
        for (uint32_t x = 0; x < 8; ++x) {
            const uint32_t source = (y / 2) * w + x / 2;
            CHECK(std::abs(int(out[(y * 8 + x) * 4]) - unorm8((source + 1) / 17.0)) <= 1);
        }
}

void state_preserved(Device& d, RoundTripPass& pass) {
    auto engine_rt = texture(d.device.Get(), 64, 64, DXGI_FORMAT_R8G8B8A8_UNORM, D3D11_BIND_RENDER_TARGET | D3D11_BIND_SHADER_RESOURCE);
    auto other = texture(d.device.Get(), 64, 64, DXGI_FORMAT_R8G8B8A8_UNORM, D3D11_BIND_RENDER_TARGET | D3D11_BIND_SHADER_RESOURCE);
    auto twin = texture(d.device.Get(), 64, 64, DXGI_FORMAT_R16G16B16A16_FLOAT, D3D11_BIND_RENDER_TARGET | D3D11_BIND_SHADER_RESOURCE);
    ComPtr<ID3D11RenderTargetView> bound_rtv, target_rtv;
    ComPtr<ID3D11ShaderResourceView> bound_srv, twin_srv;
    d.device->CreateRenderTargetView(other.Get(), nullptr, &bound_rtv);
    d.device->CreateRenderTargetView(engine_rt.Get(), nullptr, &target_rtv);
    d.device->CreateShaderResourceView(other.Get(), nullptr, &bound_srv);
    d.device->CreateShaderResourceView(twin.Get(), nullptr, &twin_srv);
    // Engine-like state: a different RT bound, an SRV in slot 5, a 32x32 viewport, a blend state with a factor.
    d.context->OMSetRenderTargets(1, bound_rtv.GetAddressOf(), nullptr);
    ID3D11ShaderResourceView* slot5[] = {twin_srv.Get()};
    d.context->PSSetShaderResources(5, 1, slot5);
    const D3D11_VIEWPORT viewport{0, 0, 32, 32, 0.25f, 0.75f};
    d.context->RSSetViewports(1, &viewport);
    D3D11_BLEND_DESC blend_desc{};
    blend_desc.RenderTarget[0].BlendEnable = TRUE;
    blend_desc.RenderTarget[0].SrcBlend = blend_desc.RenderTarget[0].SrcBlendAlpha = D3D11_BLEND_SRC_ALPHA;
    blend_desc.RenderTarget[0].DestBlend = blend_desc.RenderTarget[0].DestBlendAlpha = D3D11_BLEND_INV_SRC_ALPHA;
    blend_desc.RenderTarget[0].BlendOp = blend_desc.RenderTarget[0].BlendOpAlpha = D3D11_BLEND_OP_ADD;
    blend_desc.RenderTarget[0].RenderTargetWriteMask = 0x7;
    ComPtr<ID3D11BlendState> blend;
    d.device->CreateBlendState(&blend_desc, &blend);
    const float factor[4]{0.1f, 0.2f, 0.3f, 0.4f};
    d.context->OMSetBlendState(blend.Get(), factor, 0xf0f0f0f0);
    // The twin is bound as an input in slot 5 and the pass reads it in slot 0 while writing the engine target.
    CHECK(pass.run(d.context.Get(), twin_srv.Get(), 64, 64, target_rtv.Get(), 64, 64));
    ComPtr<ID3D11RenderTargetView> rtv_after;
    d.context->OMGetRenderTargets(1, &rtv_after, nullptr);
    ComPtr<ID3D11ShaderResourceView> srv_after;
    d.context->PSGetShaderResources(5, 1, &srv_after);
    D3D11_VIEWPORT viewport_after{};
    UINT viewports = 1;
    d.context->RSGetViewports(&viewports, &viewport_after);
    ComPtr<ID3D11BlendState> blend_after;
    float factor_after[4]{};
    UINT mask_after{};
    d.context->OMGetBlendState(&blend_after, factor_after, &mask_after);
    ComPtr<ID3D11PixelShader> shader_after;
    d.context->PSGetShader(&shader_after, nullptr, nullptr);
    CHECK(rtv_after.Get() == bound_rtv.Get() && srv_after.Get() == twin_srv.Get());
    CHECK(viewports == 1 && viewport_after.Width == 32 && viewport_after.MinDepth == 0.25f);
    CHECK(blend_after.Get() == blend.Get() && factor_after[2] == 0.3f && mask_after == 0xf0f0f0f0 && !shader_after);
    d.context->ClearState();
}

void twins(Device& d) {
    auto engine = texture(d.device.Get(), 320, 200, DXGI_FORMAT_R8G8B8A8_UNORM, D3D11_BIND_RENDER_TARGET | D3D11_BIND_SHADER_RESOURCE);
    Twin twin;
    CHECK(make_twin(d.device.Get(), engine.Get(), false, twin) == TwinError::None);
    D3D11_TEXTURE2D_DESC desc{};
    if (twin.texture) twin.texture->GetDesc(&desc);
    CHECK(desc.Width == 320 && desc.Height == 200 && desc.Format == DXGI_FORMAT_R16G16B16A16_FLOAT && desc.SampleDesc.Count == 1);
    CHECK(twin.fields.surface == twin.texture.Get() && twin.fields.texture_surface == twin.fields.surface &&
        twin.fields.desc_cache == twin.fields.surface && twin.fields.rtv && twin.fields.srv && !twin.fields.uav &&
        twin.fields.format == d3dfmt_a16b16g16r16f && std::memcmp(twin.fields.desc.data(), &desc, sizeof(desc)) == 0);
    Twin with_uav;
    CHECK(make_twin(d.device.Get(), engine.Get(), true, with_uav) == TwinError::None && with_uav.fields.uav);
    auto wrong = texture(d.device.Get(), 64, 64, DXGI_FORMAT_R16G16B16A16_FLOAT, D3D11_BIND_RENDER_TARGET | D3D11_BIND_SHADER_RESOURCE);
    Twin refused;
    CHECK(make_twin(d.device.Get(), wrong.Get(), false, refused) == TwinError::Format && !refused.texture);
    auto msaa = texture(d.device.Get(), 64, 64, DXGI_FORMAT_R8G8B8A8_UNORM, D3D11_BIND_RENDER_TARGET | D3D11_BIND_SHADER_RESOURCE, nullptr, 0, 4);
    if (msaa) CHECK(make_twin(d.device.Get(), msaa.Get(), false, refused) == TwinError::Shape);
    CHECK(make_twin(d.device.Get(), nullptr, false, refused) == TwinError::NotTexture);
}

// Mixed-format MRT as the world window binds it (FP16 twin generic0 + UNORM8 generic1 + D24S8), and the stock
// half-size blur bind against a full-size DSV.
void debug_layer(Device& d, RoundTripPass& pass) {
    if (!d.info) { std::puts("debug-layer checks skipped: D3D11 SDK layers are not installed"); return; }
    d.errors();
    auto generic0 = texture(d.device.Get(), 128, 128, DXGI_FORMAT_R16G16B16A16_FLOAT, D3D11_BIND_RENDER_TARGET | D3D11_BIND_SHADER_RESOURCE);
    auto generic1 = texture(d.device.Get(), 128, 128, DXGI_FORMAT_R8G8B8A8_UNORM, D3D11_BIND_RENDER_TARGET | D3D11_BIND_SHADER_RESOURCE);
    auto depth = texture(d.device.Get(), 128, 128, DXGI_FORMAT_R24G8_TYPELESS, D3D11_BIND_DEPTH_STENCIL | D3D11_BIND_SHADER_RESOURCE);
    auto half = texture(d.device.Get(), 64, 64, DXGI_FORMAT_R8G8B8A8_UNORM, D3D11_BIND_RENDER_TARGET);
    ComPtr<ID3D11RenderTargetView> rtv0, rtv1, half_rtv;
    ComPtr<ID3D11DepthStencilView> dsv;
    D3D11_DEPTH_STENCIL_VIEW_DESC dsv_desc{DXGI_FORMAT_D24_UNORM_S8_UINT, D3D11_DSV_DIMENSION_TEXTURE2D};
    CHECK(SUCCEEDED(d.device->CreateRenderTargetView(generic0.Get(), nullptr, &rtv0)) &&
        SUCCEEDED(d.device->CreateRenderTargetView(generic1.Get(), nullptr, &rtv1)) &&
        SUCCEEDED(d.device->CreateRenderTargetView(half.Get(), nullptr, &half_rtv)) &&
        SUCCEEDED(d.device->CreateDepthStencilView(depth.Get(), &dsv_desc, &dsv)));
    ID3D11RenderTargetView* mrt[] = {rtv0.Get(), rtv1.Get()};
    d.context->OMSetRenderTargets(2, mrt, dsv.Get());
    const float clear[4]{2.5f, 0.5f, 0.25f, 1};
    d.context->ClearRenderTargetView(rtv0.Get(), clear);
    d.context->ClearDepthStencilView(dsv.Get(), D3D11_CLEAR_DEPTH | D3D11_CLEAR_STENCIL, 1, 0);
    // Draw through the pass's shaders into both targets with blending on target 0 (FP16 blending).
    ComPtr<ID3D11ShaderResourceView> twin_srv;
    auto other = texture(d.device.Get(), 128, 128, DXGI_FORMAT_R16G16B16A16_FLOAT, D3D11_BIND_SHADER_RESOURCE);
    d.device->CreateShaderResourceView(other.Get(), nullptr, &twin_srv);
    CHECK(pass.run(d.context.Get(), twin_srv.Get(), 128, 128, rtv0.Get(), 128, 128));
    const auto mixed = d.errors();
    std::printf("debug layer: mixed FP16/UNORM8 MRT with D24S8 DSV: %llu error(s)\n", static_cast<unsigned long long>(mixed));
    CHECK(mixed == 0);
    // Informational (design register item 3): a 64x64 RTV with the 128x128 DSV, as stock phase_blur binds it.
    d.context->OMSetRenderTargets(1, half_rtv.GetAddressOf(), dsv.Get());
    ComPtr<ID3D11RenderTargetView> bound;
    ComPtr<ID3D11DepthStencilView> bound_dsv;
    d.context->OMGetRenderTargets(1, &bound, &bound_dsv);
    const auto mismatch = d.errors();
    std::printf("debug layer: 64x64 RTV + 128x128 DSV bind: %llu error(s); afterwards the context holds %s (informational)\n",
        static_cast<unsigned long long>(mismatch),
        bound.Get() == half_rtv.Get() && bound_dsv.Get() == dsv.Get() ? "the requested half RTV and full DSV"
            : bound.Get() == half_rtv.Get() ? "the half RTV without the DSV"
            : "the previous binding (the call was dropped)");
    d.context->ClearState();
}

// ---- Stage 2: seam upscale pass, world twins and the depth twin ---------------------------------------------------
std::vector<uint8_t> read_raw(Device& d, ID3D11Texture2D* source, DXGI_FORMAT format, uint32_t w, uint32_t h, uint32_t texel) {
    D3D11_TEXTURE2D_DESC desc{w, h, 1, 1, format, {1, 0}, D3D11_USAGE_STAGING, 0, D3D11_CPU_ACCESS_READ, 0};
    ComPtr<ID3D11Texture2D> staging;
    std::vector<uint8_t> bytes(size_t(w) * h * texel);
    if (FAILED(d.device->CreateTexture2D(&desc, nullptr, &staging))) return {};
    d.context->CopyResource(staging.Get(), source);
    D3D11_MAPPED_SUBRESOURCE mapped{};
    if (FAILED(d.context->Map(staging.Get(), 0, D3D11_MAP_READ, 0, &mapped))) return {};
    for (uint32_t y = 0; y < h; ++y)
        std::memcpy(&bytes[size_t(y) * w * texel], static_cast<uint8_t*>(mapped.pData) + size_t(y) * mapped.RowPitch, size_t(w) * texel);
    d.context->Unmap(staging.Get(), 0);
    return bytes;
}
template<class T> T at(const std::vector<uint8_t>& bytes, size_t index) { T value; std::memcpy(&value, &bytes[index * sizeof(T)], sizeof(T)); return value; }

// Render-size inputs and output-size engine targets, as the seam sees them.
struct Rig {
    uint32_t rw, rh, ow, oh;
    std::vector<uint16_t> colour, position;
    std::vector<uint8_t> distort;
    ComPtr<ID3D11Texture2D> colour_tex, position_tex, distort_tex, engine_colour, engine_position, engine_distort, engine_depth;
    ComPtr<ID3D11ShaderResourceView> colour_srv, position_srv, distort_srv;
    ComPtr<ID3D11RenderTargetView> colour_rtv, position_rtv, distort_rtv;
    ComPtr<ID3D11DepthStencilView> engine_dsv;
    DepthTwin depth;
    bool ok{};
    UpscaleViews views() const {
        return {colour_srv.Get(), position_srv.Get(), distort_srv.Get(), depth.srv.Get(), rw, rh, colour_rtv.Get(),
            position_rtv.Get(), distort_rtv.Get(), engine_dsv.Get(), ow, oh};
    }
};
uint32_t hash(uint32_t v) { v ^= v >> 16; v *= 0x7feb352d; v ^= v >> 15; v *= 0x846ca68b; v ^= v >> 16; return v; }

// Fills the depth twin with a per-pixel hash through SV_Depth (depth textures take no initial data).
bool fill_depth(Device& d, ID3D11DepthStencilView* dsv, uint32_t w, uint32_t h) {
    static const char* source = R"(
float4 vs(uint id : SV_VertexID) : SV_Position { float2 uv = float2((id << 1) & 2, id & 2); return float4(uv * float2(2, -2) + float2(-1, 1), 0, 1); }
float ps(float4 p : SV_Position) : SV_Depth { uint2 q = uint2(p.xy); uint v = q.x * 7919u + q.y * 104729u + 17u; v ^= v >> 13; v *= 0x5bd1e995u; v ^= v >> 15; return float(v & 0xffffffu) / 16777215.0; }
)";
    ComPtr<ID3D11VertexShader> vs;
    ComPtr<ID3D11PixelShader> ps;
    if (!rsf::compile_vertex(d.device.Get(), source, "fill_depth", "vs", &vs) || !rsf::compile_pixel(d.device.Get(), source, "fill_depth", "ps", &ps))
        return false;
    D3D11_DEPTH_STENCIL_DESC ds{};
    ds.DepthEnable = TRUE;
    ds.DepthWriteMask = D3D11_DEPTH_WRITE_MASK_ALL;
    ds.DepthFunc = D3D11_COMPARISON_ALWAYS;
    ComPtr<ID3D11DepthStencilState> state;
    d.device->CreateDepthStencilState(&ds, &state);
    d.context->ClearDepthStencilView(dsv, D3D11_CLEAR_DEPTH | D3D11_CLEAR_STENCIL, 1.0f, 0x33);
    d.context->OMSetRenderTargets(0, nullptr, dsv);
    d.context->OMSetDepthStencilState(state.Get(), 0);
    d.context->IASetPrimitiveTopology(D3D11_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
    d.context->VSSetShader(vs.Get(), nullptr, 0);
    d.context->PSSetShader(ps.Get(), nullptr, 0);
    const D3D11_VIEWPORT viewport{0, 0, float(w), float(h), 0, 1};
    d.context->RSSetViewports(1, &viewport);
    d.context->Draw(3, 0);
    d.context->ClearState();
    return true;
}

Rig make_rig(Device& d, uint32_t rw, uint32_t rh, uint32_t ow, uint32_t oh, const std::vector<uint16_t>* colour = nullptr) {
    Rig r{rw, rh, ow, oh};
    const size_t n = size_t(rw) * rh;
    if (colour) r.colour = *colour;
    else {
        r.colour.resize(n * 4);
        for (size_t i = 0; i < n; ++i) {
            for (int c = 0; c < 3; ++c) {
                // x in [0, 6]: covers the UNORM8 knee at W = 1.7 and the clipped range above it.
                const double x = 6.0 * double(hash(uint32_t(i * 4 + c)) % 10000u) / 9999.0;
                r.colour[i * 4 + c] = float_to_half(float(tonemap_forward(x)));
            }
            r.colour[i * 4 + 3] = float_to_half(float(hash(uint32_t(i)) % 256u) / 255.0f);
        }
    }
    r.position.resize(n * 4);
    r.distort.resize(n * 4);
    for (size_t i = 0; i < n * 4; ++i) {
        r.position[i] = uint16_t(hash(uint32_t(i) + 99u) & 0x7bff);   // finite halves of both signs
        r.distort[i] = uint8_t(hash(uint32_t(i) + 7u));
    }
    auto* dev = d.device.Get();
    const UINT bind = D3D11_BIND_RENDER_TARGET | D3D11_BIND_SHADER_RESOURCE;
    r.colour_tex = texture(dev, rw, rh, DXGI_FORMAT_R16G16B16A16_FLOAT, bind, r.colour.data(), rw * 8);
    r.position_tex = texture(dev, rw, rh, DXGI_FORMAT_R16G16B16A16_FLOAT, bind, r.position.data(), rw * 8);
    r.distort_tex = texture(dev, rw, rh, DXGI_FORMAT_R8G8B8A8_UNORM, bind, r.distort.data(), rw * 4);
    r.engine_colour = texture(dev, ow, oh, DXGI_FORMAT_R8G8B8A8_UNORM, bind);
    r.engine_position = texture(dev, ow, oh, DXGI_FORMAT_R16G16B16A16_FLOAT, bind);
    r.engine_distort = texture(dev, ow, oh, DXGI_FORMAT_R8G8B8A8_UNORM, bind);
    r.engine_depth = texture(dev, ow, oh, DXGI_FORMAT_D24_UNORM_S8_UINT, D3D11_BIND_DEPTH_STENCIL);   // as dx10HW.cpp:1017-1058
    if (!r.colour_tex || !r.position_tex || !r.distort_tex || !r.engine_colour || !r.engine_position || !r.engine_distort || !r.engine_depth ||
        FAILED(dev->CreateShaderResourceView(r.colour_tex.Get(), nullptr, &r.colour_srv)) ||
        FAILED(dev->CreateShaderResourceView(r.position_tex.Get(), nullptr, &r.position_srv)) ||
        FAILED(dev->CreateShaderResourceView(r.distort_tex.Get(), nullptr, &r.distort_srv)) ||
        FAILED(dev->CreateRenderTargetView(r.engine_colour.Get(), nullptr, &r.colour_rtv)) ||
        FAILED(dev->CreateRenderTargetView(r.engine_position.Get(), nullptr, &r.position_rtv)) ||
        FAILED(dev->CreateRenderTargetView(r.engine_distort.Get(), nullptr, &r.distort_rtv)) ||
        FAILED(dev->CreateDepthStencilView(r.engine_depth.Get(), nullptr, &r.engine_dsv)))
        return r;
    if (make_depth_twin(dev, r.engine_dsv.Get(), ow, oh, rw, rh, r.depth) != TwinError::None || !fill_depth(d, r.depth.dsv.Get(), rw, rh)) return r;
    // Stale engine contents: the pass must overwrite every pixel and clear the stencil.
    const float grey[4]{0.5f, 0.25f, 0.75f, 0.125f};
    d.context->ClearRenderTargetView(r.colour_rtv.Get(), grey);
    d.context->ClearRenderTargetView(r.position_rtv.Get(), grey);
    d.context->ClearRenderTargetView(r.distort_rtv.Get(), grey);
    d.context->ClearDepthStencilView(r.engine_dsv.Get(), D3D11_CLEAR_DEPTH | D3D11_CLEAR_STENCIL, 0.5f, 0x5a);
    r.ok = true;
    return r;
}

struct Output { std::vector<uint8_t> colour, position, distort, depth, twin_depth; };
Output run_upscale(Device& d, UpscalePass& pass, Rig& r) {
    Output out;
    if (!r.ok || !pass.run(d.context.Get(), r.views())) return out;
    out.colour = read_raw(d, r.engine_colour.Get(), DXGI_FORMAT_R8G8B8A8_UNORM, r.ow, r.oh, 4);
    out.position = read_raw(d, r.engine_position.Get(), DXGI_FORMAT_R16G16B16A16_FLOAT, r.ow, r.oh, 8);
    out.distort = read_raw(d, r.engine_distort.Get(), DXGI_FORMAT_R8G8B8A8_UNORM, r.ow, r.oh, 4);
    out.depth = read_raw(d, r.engine_depth.Get(), DXGI_FORMAT_D24_UNORM_S8_UINT, r.ow, r.oh, 4);
    out.twin_depth = read_raw(d, r.depth.texture.Get(), DXGI_FORMAT_R24G8_TYPELESS, r.rw, r.rh, 4);
    return out;
}

// CPU reference of the seam pass for one output pixel and channel, at source coordinate `coord` (two candidates are
// tried where a coordinate sits within float rounding of a texel centre: D3D11 float division is not exact).
double reference_colour(const Rig& r, double cx, double cy, int c) {
    const double bx = std::floor(cx), by = std::floor(cy), wx = cx - bx, wy = cy - by;
    const auto clampi = [](double v, uint32_t n) { return int(std::min<double>(std::max<double>(v, 0.0), double(n - 1))); };
    const int x0 = clampi(bx, r.rw), x1 = clampi(bx + 1, r.rw), y0 = clampi(by, r.rh), y1 = clampi(by + 1, r.rh);
    const auto lin = [&](int x, int y) {
        float y_value = half_to_float(r.colour[(size_t(y) * r.rw + x) * 4 + c]);
        y_value = std::min(std::max(y_value, 0.0f), 65504.0f);
        return tonemap_inverse(y_value);
    };
    const double top = lin(x0, y0) + (lin(x1, y0) - lin(x0, y0)) * wx, bottom = lin(x0, y1) + (lin(x1, y1) - lin(x0, y1)) * wx;
    return tonemap_forward(top + (bottom - top) * wy);
}
double reference_distort(const Rig& r, double cx, double cy, int c) {
    const double bx = std::floor(cx), by = std::floor(cy), wx = cx - bx, wy = cy - by;
    const auto clampi = [](double v, uint32_t n) { return int(std::min<double>(std::max<double>(v, 0.0), double(n - 1))); };
    const int x0 = clampi(bx, r.rw), x1 = clampi(bx + 1, r.rw), y0 = clampi(by, r.rh), y1 = clampi(by + 1, r.rh);
    const auto v = [&](int x, int y) { return r.distort[(size_t(y) * r.rw + x) * 4 + c] / 255.0; };
    const double top = v(x0, y0) + (v(x1, y0) - v(x0, y0)) * wx, bottom = v(x0, y1) + (v(x1, y1) - v(x0, y1)) * wx;
    return top + (bottom - top) * wy;
}

void check_upscale(Device& d, UpscalePass& pass, uint32_t rw, uint32_t rh, uint32_t ow, uint32_t oh) {
    auto rig = make_rig(d, rw, rh, ow, oh);
    CHECK(rig.ok);
    const auto out = run_upscale(d, pass, rig);
    CHECK(!out.colour.empty() && !out.depth.empty() && !out.twin_depth.empty());
    if (out.colour.empty() || out.depth.empty() || out.twin_depth.empty()) return;
    int worst_colour = 0, worst_distort = 0, alpha_bad = 0, position_bad = 0, depth_bad = 0, stencil_bad = 0;
    for (uint32_t y = 0; y < oh; ++y)
        for (uint32_t x = 0; x < ow; ++x) {
            const size_t o = size_t(y) * ow + x;
            const uint32_t nx = std::min(x * rw / ow, rw - 1), ny = std::min(y * rh / oh, rh - 1);
            const size_t s = size_t(ny) * rw + nx;
            const double cx = (x + 0.5) * rw / ow - 0.5, cy = (y + 0.5) * rh / oh - 0.5;
            for (int c = 0; c < 3; ++c) {
                int best = 255, best_d = 255;
                for (const double e : {0.0, 1e-4, -1e-4}) {
                    best = std::min(best, std::abs(int(out.colour[o * 4 + c]) - unorm8(reference_colour(rig, cx + e, cy + e, c))));
                    best_d = std::min(best_d, std::abs(int(out.distort[o * 4 + c]) - unorm8(reference_distort(rig, cx + e, cy + e, c))));
                }
                worst_colour = std::max(worst_colour, best);
                worst_distort = std::max(worst_distort, best_d);
            }
            const float a = half_to_float(rig.colour[s * 4 + 3]);
            alpha_bad += std::abs(int(out.colour[o * 4 + 3]) - unorm8(a)) > 1;
            for (int c = 0; c < 4; ++c) position_bad += at<uint16_t>(out.position, o * 4 + c) != rig.position[s * 4 + c];
            const uint32_t engine = at<uint32_t>(out.depth, o), twin = at<uint32_t>(out.twin_depth, s);
            depth_bad += (engine & 0xffffff) != (twin & 0xffffff);
            stencil_bad += (engine >> 24) != 0;
        }
    std::printf("upscale %ux%u -> %ux%u: colour worst %d code(s) vs float64 bilinear in f^-1 space, generic1 worst %d, "
        "alpha mismatches %d, position bit mismatches %d, depth mismatches %d, stencil not cleared %d\n",
        rw, rh, ow, oh, worst_colour, worst_distort, alpha_bad, position_bad, depth_bad, stencil_bad);
    CHECK(worst_colour <= 1 && worst_distort <= 1 && alpha_bad == 0 && position_bad == 0 && depth_bad == 0 && stencil_bad == 0);
}

// At scale 1.0 the seam pass must equal the Stage 1 round trip bit for bit, and copy position/generic1/depth exactly.
void upscale_identity(Device& d, UpscalePass& pass, RoundTripPass& round_trip) {
    constexpr uint32_t w = 256, h = 256;
    std::vector<uint16_t> halves(size_t(w) * h * 4);
    for (uint32_t i = 0; i < w * h; ++i) halves[i * 4 + 0] = halves[i * 4 + 1] = halves[i * 4 + 2] = halves[i * 4 + 3] = uint16_t(i);
    auto rig = make_rig(d, w, h, w, h, &halves);
    CHECK(rig.ok);
    const auto out = run_upscale(d, pass, rig);
    const auto stage1 = run_pass(d, round_trip, halves, w, h, w, h);
    CHECK(!out.colour.empty() && out.colour == stage1);
    int position_bad = 0, distort_bad = 0, depth_bad = 0;
    for (size_t i = 0; i < size_t(w) * h && !out.colour.empty(); ++i) {
        for (int c = 0; c < 4; ++c) {
            position_bad += at<uint16_t>(out.position, i * 4 + c) != rig.position[i * 4 + c];
            distort_bad += out.distort[i * 4 + c] != rig.distort[i * 4 + c];
        }
        depth_bad += (at<uint32_t>(out.depth, i) & 0xffffff) != (at<uint32_t>(out.twin_depth, i) & 0xffffff);
    }
    std::printf("upscale at scale 1.0 on every FP16 pattern: colour %s the Stage 1 round trip; position %d, generic1 %d, depth %d mismatches\n",
        !out.colour.empty() && out.colour == stage1 ? "equals" : "DIFFERS FROM", position_bad, distort_bad, depth_bad);
    CHECK(position_bad == 0 && distort_bad == 0 && depth_bad == 0);
}

void upscale_state_and_refusals(Device& d, UpscalePass& pass) {
    auto rig = make_rig(d, 32, 18, 48, 27);
    CHECK(rig.ok);
    if (!rig.ok) return;
    auto other = texture(d.device.Get(), 32, 18, DXGI_FORMAT_R8G8B8A8_UNORM, D3D11_BIND_RENDER_TARGET);
    ComPtr<ID3D11RenderTargetView> bound;
    d.device->CreateRenderTargetView(other.Get(), nullptr, &bound);
    // Engine-like state inside the world window: a render-size target with the depth twin and a render viewport.
    d.context->OMSetRenderTargets(1, bound.GetAddressOf(), rig.depth.dsv.Get());
    const D3D11_VIEWPORT viewport{0, 0, 32, 18, 0, 0.02f};
    d.context->RSSetViewports(1, &viewport);
    CHECK(pass.run(d.context.Get(), rig.views()));
    ComPtr<ID3D11RenderTargetView> rtv_after;
    ComPtr<ID3D11DepthStencilView> dsv_after;
    d.context->OMGetRenderTargets(1, &rtv_after, &dsv_after);
    D3D11_VIEWPORT viewport_after{};
    UINT count = 1;
    d.context->RSGetViewports(&count, &viewport_after);
    CHECK(rtv_after.Get() == bound.Get() && dsv_after.Get() == rig.depth.dsv.Get() && count == 1 && viewport_after.Width == 32 &&
        viewport_after.MaxDepth == 0.02f);
    set_full_viewport(d.context.Get(), 48, 27);
    d.context->RSGetViewports(&count, &viewport_after);
    CHECK(viewport_after.Width == 48 && viewport_after.Height == 27 && viewport_after.MinDepth == 0 && viewport_after.MaxDepth == 1);
    d.context->ClearState();
    // Refusals: missing inputs, a source larger than the output.
    auto views = rig.views();
    views.depth = nullptr;
    CHECK(!pass.run(d.context.Get(), views));
    views = rig.views();
    views.source_width = 64;
    CHECK(!pass.run(d.context.Get(), views));
}

void world_twins(Device& d) {
    auto* dev = d.device.Get();
    const UINT bind = D3D11_BIND_RENDER_TARGET | D3D11_BIND_SHADER_RESOURCE;
    // rt_ssao_temp: R16F with a UAV, kept format, render size.
    auto ssao = texture(dev, 96, 54, DXGI_FORMAT_R16_FLOAT, bind | D3D11_BIND_UNORDERED_ACCESS);
    ComPtr<ID3D11RenderTargetView> ssao_rtv;
    ComPtr<ID3D11ShaderResourceView> ssao_srv;
    ComPtr<ID3D11UnorderedAccessView> ssao_uav;
    dev->CreateRenderTargetView(ssao.Get(), nullptr, &ssao_rtv);
    dev->CreateShaderResourceView(ssao.Get(), nullptr, &ssao_srv);
    dev->CreateUnorderedAccessView(ssao.Get(), nullptr, &ssao_uav);
    TargetFields engine{ssao.Get(), ssao_rtv.Get(), nullptr, ssao_uav.Get(), 96, 54, 111};
    engine.srv = ssao_srv.Get();
    Twin twin;
    CHECK(make_world_twin(dev, engine, 64, 36, false, twin) == TwinError::None);
    D3D11_TEXTURE2D_DESC desc{};
    if (twin.texture) twin.texture->GetDesc(&desc);
    CHECK(desc.Width == 64 && desc.Height == 36 && desc.Format == DXGI_FORMAT_R16_FLOAT && (desc.BindFlags & D3D11_BIND_UNORDERED_ACCESS));
    CHECK(twin.fields.uav && twin.fields.format == 111 && twin.fields.width == 64 && twin.fields.desc_cache == twin.fields.surface &&
        std::memcmp(twin.fields.desc.data(), &desc, sizeof(desc)) == 0);
    // A typeless engine texture with UNORM views: the twin views copy the engine view formats.
    auto typeless = texture(dev, 96, 54, DXGI_FORMAT_R8G8B8A8_TYPELESS, bind);
    D3D11_RENDER_TARGET_VIEW_DESC rtv_desc{DXGI_FORMAT_R8G8B8A8_UNORM, D3D11_RTV_DIMENSION_TEXTURE2D};
    D3D11_SHADER_RESOURCE_VIEW_DESC srv_desc{DXGI_FORMAT_R8G8B8A8_UNORM, D3D11_SRV_DIMENSION_TEXTURE2D};
    srv_desc.Texture2D.MipLevels = 1;
    ComPtr<ID3D11RenderTargetView> typeless_rtv;
    ComPtr<ID3D11ShaderResourceView> typeless_srv;
    CHECK(SUCCEEDED(dev->CreateRenderTargetView(typeless.Get(), &rtv_desc, &typeless_rtv)) &&
        SUCCEEDED(dev->CreateShaderResourceView(typeless.Get(), &srv_desc, &typeless_srv)));
    TargetFields albedo{typeless.Get(), typeless_rtv.Get(), nullptr, nullptr, 96, 54, 21};
    albedo.srv = typeless_srv.Get();
    Twin kept;
    CHECK(make_world_twin(dev, albedo, 64, 36, false, kept) == TwinError::None && kept.rtv && kept.srv);
    D3D11_RENDER_TARGET_VIEW_DESC kept_rtv{};
    if (kept.rtv) kept.rtv->GetDesc(&kept_rtv);
    CHECK(kept_rtv.Format == DXGI_FORMAT_R8G8B8A8_UNORM && kept.fields.format == 21 && !kept.fields.uav);
    // generic0 promoted at render size.
    Twin promoted;
    CHECK(make_world_twin(dev, albedo, 64, 36, true, promoted) == TwinError::None);
    if (promoted.texture) promoted.texture->GetDesc(&desc);
    CHECK(desc.Format == DXGI_FORMAT_R16G16B16A16_FLOAT && desc.Width == 64 && promoted.fields.format == d3dfmt_a16b16g16r16f);
    // Promotion needs a UNORM8 engine target; a zero size is refused.
    CHECK(make_world_twin(dev, engine, 64, 36, true, promoted) == TwinError::Format && !promoted.texture);
    CHECK(make_world_twin(dev, albedo, 0, 36, false, promoted) == TwinError::Shape);
}

void depth_twins(Device& d) {
    auto* dev = d.device.Get();
    auto depth = texture(dev, 96, 54, DXGI_FORMAT_D24_UNORM_S8_UINT, D3D11_BIND_DEPTH_STENCIL);
    ComPtr<ID3D11DepthStencilView> dsv;
    CHECK(SUCCEEDED(dev->CreateDepthStencilView(depth.Get(), nullptr, &dsv)));
    DepthTwin twin;
    CHECK(make_depth_twin(dev, dsv.Get(), 96, 54, 64, 36, twin) == TwinError::None && twin.dsv && twin.srv);
    D3D11_TEXTURE2D_DESC desc{};
    if (twin.texture) twin.texture->GetDesc(&desc);
    D3D11_DEPTH_STENCIL_VIEW_DESC dsv_desc{};
    D3D11_SHADER_RESOURCE_VIEW_DESC srv_desc{};
    if (twin.dsv) twin.dsv->GetDesc(&dsv_desc);
    if (twin.srv) twin.srv->GetDesc(&srv_desc);
    CHECK(desc.Width == 64 && desc.Height == 36 && desc.Format == DXGI_FORMAT_R24G8_TYPELESS && twin.width == 64 && twin.height == 36);
    CHECK(dsv_desc.Format == DXGI_FORMAT_D24_UNORM_S8_UINT && srv_desc.Format == DXGI_FORMAT_R24_UNORM_X8_TYPELESS);
    DepthTwin refused;
    CHECK(make_depth_twin(dev, dsv.Get(), 128, 54, 64, 36, refused) == TwinError::Depth && !refused.texture);   // not output size
    auto d32 = texture(dev, 96, 54, DXGI_FORMAT_D32_FLOAT, D3D11_BIND_DEPTH_STENCIL);
    ComPtr<ID3D11DepthStencilView> d32_dsv;
    dev->CreateDepthStencilView(d32.Get(), nullptr, &d32_dsv);
    CHECK(make_depth_twin(dev, d32_dsv.Get(), 96, 54, 64, 36, refused) == TwinError::Depth);
    auto msaa = texture(dev, 96, 54, DXGI_FORMAT_D24_UNORM_S8_UINT, D3D11_BIND_DEPTH_STENCIL, nullptr, 0, 4);
    if (msaa) {
        ComPtr<ID3D11DepthStencilView> msaa_dsv;
        dev->CreateDepthStencilView(msaa.Get(), nullptr, &msaa_dsv);
        CHECK(make_depth_twin(dev, msaa_dsv.Get(), 96, 54, 64, 36, refused) == TwinError::Depth);
    }
    CHECK(make_depth_twin(dev, nullptr, 96, 54, 64, 36, refused) == TwinError::NotTexture);
}

// The seam pass binds three mixed-format MRTs and the engine D24S8: no debug-layer message. Informational: the world
// window's phase_occq binds the output-size HW.pBaseRT with the render-size depth twin (r4_rendertarget_phase_occq.cpp:6).
void debug_layer_stage2(Device& d, UpscalePass& pass) {
    if (!d.info) { std::puts("Stage 2 debug-layer checks skipped: D3D11 SDK layers are not installed"); return; }
    d.errors();
    auto rig = make_rig(d, 64, 36, 96, 54);
    CHECK(rig.ok && pass.run(d.context.Get(), rig.views()));
    const auto seam = d.errors();
    std::printf("debug layer: seam pass (3 MRTs FP16/UNORM8 + D24S8, SV_Depth): %llu error(s)\n", static_cast<unsigned long long>(seam));
    CHECK(seam == 0);
    d.context->OMSetRenderTargets(1, rig.colour_rtv.GetAddressOf(), rig.depth.dsv.Get());   // 96x54 RTV, 64x36 DSV
    ComPtr<ID3D11RenderTargetView> bound;
    ComPtr<ID3D11DepthStencilView> bound_dsv;
    d.context->OMGetRenderTargets(1, &bound, &bound_dsv);
    const auto occq = d.errors();
    std::printf("debug layer: output-size RTV + render-size DSV (phase_occq without the stand-in): %llu error(s); the context holds %s\n",
        static_cast<unsigned long long>(occq),
        bound.Get() == rig.colour_rtv.Get() && bound_dsv.Get() == rig.depth.dsv.Get() ? "both views"
            : bound.Get() == rig.colour_rtv.Get() ? "the RTV without the DSV"
            : bound_dsv.Get() == rig.depth.dsv.Get() ? "the DSV without the RTV"
            : "the previous binding (the call was dropped)");
    CHECK(occq == 1);   // why Stage 2 swaps HW.pBaseRT for a render-size stand-in
    // With the stand-in (what Stage 2 installs in HW.pBaseRT for the window): a valid bind.
    ScratchTarget scratch;
    CHECK(make_scratch_target(d.device.Get(), rig.colour_rtv.Get(), 64, 36, scratch) == TwinError::None && scratch.rtv);
    d.context->OMSetRenderTargets(1, scratch.rtv.GetAddressOf(), rig.depth.dsv.Get());
    bound.Reset();
    bound_dsv.Reset();
    d.context->OMGetRenderTargets(1, &bound, &bound_dsv);
    const auto stand_in = d.errors();
    std::printf("debug layer: render-size HW.pBaseRT stand-in + depth twin: %llu error(s)\n", static_cast<unsigned long long>(stand_in));
    CHECK(stand_in == 0 && bound.Get() == scratch.rtv.Get() && bound_dsv.Get() == rig.depth.dsv.Get());
    D3D11_TEXTURE2D_DESC desc{};
    if (scratch.texture) scratch.texture->GetDesc(&desc);
    CHECK(desc.Width == 64 && desc.Height == 36 && desc.Format == DXGI_FORMAT_R8G8B8A8_UNORM);
    CHECK(make_scratch_target(d.device.Get(), nullptr, 64, 36, scratch) == TwinError::NotTexture);
    d.context->ClearState();
}
}

int main() {
    Device d;
    if (!d.create()) { std::puts("SKIP: no D3D11 hardware or WARP device"); return 77; }
    RoundTripPass pass;
    if (!pass.create(d.device.Get(), [](void*, const char* line) { std::printf("%s\n", line); })) {
        std::puts("FAIL: round-trip pass could not be created (d3dcompiler_47)");
        return 1;
    }
    every_half(d, pass);
    stock_curve(d, pass);
    nearest_mapping(d, pass);
    state_preserved(d, pass);
    twins(d);
    debug_layer(d, pass);
    UpscalePass upscale;
    if (!upscale.create(d.device.Get(), [](void*, const char* line) { std::printf("%s\n", line); })) {
        std::puts("FAIL: upscale pass could not be created (d3dcompiler_47)");
        return 1;
    }
    upscale_identity(d, upscale, pass);
    check_upscale(d, upscale, 64, 36, 128, 72);     // scale 0.5
    check_upscale(d, upscale, 64, 36, 96, 54);      // 1.5:1
    check_upscale(d, upscale, 65, 41, 97, 61);      // render_extent(97/61, 0.67): odd sizes
    upscale_state_and_refusals(d, upscale);
    world_twins(d);
    depth_twins(d);
    debug_layer_stage2(d, upscale);
    std::printf("%d failure(s)\n", failures);
    return failures ? 1 : 0;
}
