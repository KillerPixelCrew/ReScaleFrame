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
// This proves the pass and its numerics, not the in-game swap.
#include "sr_d3d11.h"
#include "sr_tonemap.h"

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
    std::printf("%d failure(s)\n", failures);
    return failures ? 1 : 0;
}
