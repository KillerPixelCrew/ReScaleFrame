// SPDX-License-Identifier: GPL-3.0-only
#include "../runtime/orchestrator/src/native_reinsert.h"
#include <wrl/client.h>
#include <array>
#include <cmath>
#include <cstdio>
#include <cstring>
#include <vector>

namespace {
using Microsoft::WRL::ComPtr;
int failures{};
void check(bool value, const char* name) { if (!value) { ++failures; std::fprintf(stderr, "FAIL %s\n", name); } }
ComPtr<ID3D11Texture2D> texture(ID3D11Device* device, DXGI_FORMAT format, UINT width, UINT height,
    UINT stride, const void* data, UINT bind = 0)
{
    D3D11_TEXTURE2D_DESC desc{}; desc.Width = width; desc.Height = height; desc.Format = format;
    desc.MipLevels = desc.ArraySize = desc.SampleDesc.Count = 1; desc.BindFlags = bind;
    D3D11_SUBRESOURCE_DATA initial{data, width * stride, 0};
    ComPtr<ID3D11Texture2D> result;
    check(SUCCEEDED(device->CreateTexture2D(&desc, &initial, &result)), "fixture texture creation");
    return result;
}
std::vector<unsigned char> read(ID3D11Device* device, ID3D11DeviceContext* context, ID3D11Texture2D* source, UINT stride)
{
    D3D11_TEXTURE2D_DESC desc{}; source->GetDesc(&desc);
    desc.Usage = D3D11_USAGE_STAGING; desc.BindFlags = desc.MiscFlags = 0; desc.CPUAccessFlags = D3D11_CPU_ACCESS_READ;
    ComPtr<ID3D11Texture2D> staging;
    if (FAILED(device->CreateTexture2D(&desc, nullptr, &staging))) return {};
    context->CopyResource(staging.Get(), source);
    D3D11_MAPPED_SUBRESOURCE mapped{};
    if (FAILED(context->Map(staging.Get(), 0, D3D11_MAP_READ, 0, &mapped))) return {};
    std::vector<unsigned char> result(size_t(desc.Width) * desc.Height * stride);
    for (UINT y = 0; y < desc.Height; ++y)
        std::memcpy(result.data() + size_t(y) * desc.Width * stride,
            static_cast<unsigned char*>(mapped.pData) + size_t(y) * mapped.RowPitch, size_t(desc.Width) * stride);
    context->Unmap(staging.Get(), 0); return result;
}
float unsigned_float(uint32_t bits, unsigned mantissa)
{
    const uint32_t mask = (1u << mantissa) - 1;
    const auto exponent = bits >> mantissa;
    return exponent ? std::ldexp(1.0f + float(bits & mask) / float(1u << mantissa), int(exponent) - 15) :
        std::ldexp(float(bits & mask), -14 - int(mantissa));
}
}

int main()
{
    ComPtr<ID3D11Device> device; ComPtr<ID3D11DeviceContext> context;
    const D3D_FEATURE_LEVEL level = D3D_FEATURE_LEVEL_11_0;
    if (FAILED(D3D11CreateDevice(nullptr, D3D_DRIVER_TYPE_WARP, nullptr, 0, &level, 1, D3D11_SDK_VERSION,
        &device, nullptr, &context))) return 77;
    // Different HDR colours expose clipping, filtering, transfer functions and source-coordinate errors.
    const std::array<std::array<float, 4>, 4> pixels{{{4, 2, 0.5f, 0.25f}, {0.25f, 16, 1, 0.5f},
        {8, 0.5f, 32, 0.75f}, {1, 4, 2, 1}}};
    const std::array<uint32_t, 20> empty{};
    const std::array<uint16_t, 80> empty16{};
    const std::array<float, 80> empty32{};
    const std::array<uint32_t, 20> poison{0x7f7f7f7f};
    auto source = texture(device.Get(), DXGI_FORMAT_R32G32B32A32_FLOAT, 2, 2, 16, pixels.data(),
        D3D11_BIND_SHADER_RESOURCE | D3D11_BIND_UNORDERED_ACCESS);
    auto target = texture(device.Get(), DXGI_FORMAT_R11G11B10_FLOAT, 5, 4, 4, empty.data());
    auto mirror = texture(device.Get(), DXGI_FORMAT_R16G16B16A16_FLOAT, 5, 4, 8, empty16.data());
    auto direct = texture(device.Get(), DXGI_FORMAT_R32G32B32A32_FLOAT, 5, 4, 16, empty32.data());
    auto unsupported = texture(device.Get(), DXGI_FORMAT_R8G8B8A8_UNORM, 5, 4, 4, poison.data());
    if (!source || !target || !mirror || !direct || !unsupported) return 1;
    ComPtr<ID3D11UnorderedAccessView> source_uav;
    check(SUCCEEDED(device->CreateUnorderedAccessView(source.Get(), nullptr, &source_uav)), "source UAV creation");
    auto* bound_uav = source_uav.Get();
    context->CSSetUnorderedAccessViews(0, 1, &bound_uav, nullptr);
    const int32_t rect[]{1, 1, 3, 3};
    check(!rsf_native_reinsert(context.Get(), source.Get(), target.Get(), unsupported.Get(), rect),
        "unsupported mirror refused before any engine writes");
    check(read(device.Get(), context.Get(), target.Get(), 4) == std::vector<unsigned char>(80), "target untouched after mirror refusal");
    const int32_t outside[]{4, 3, 6, 5};
    check(!rsf_native_reinsert(context.Get(), source.Get(), target.Get(), nullptr, outside), "out-of-bounds rectangle refused");
    check(rsf_native_reinsert(context.Get(), source.Get(), target.Get(), mirror.Get(), rect), "two different HDR destination formats");
    ComPtr<ID3D11UnorderedAccessView> restored_uav;
    context->CSGetUnorderedAccessViews(0, 1, &restored_uav);
    check(restored_uav.Get() == source_uav.Get(), "conflicting source UAV restored after conversion");
    const auto packed = read(device.Get(), context.Get(), target.Get(), 4);
    const auto halves = read(device.Get(), context.Get(), mirror.Get(), 8);
    check(packed.size() == 80 && halves.size() == 160, "HDR readback completed");
    if (packed.size() == 80 && halves.size() == 160) {
        for (UINT y = 0; y < 4; ++y) for (UINT x = 0; x < 5; ++x) {
            uint32_t value{}; std::memcpy(&value, packed.data() + (y * 5 + x) * 4, 4);
            uint16_t half[4]{}; std::memcpy(half, halves.data() + (y * 5 + x) * 8, 8);
            if (x >= 1 && x < 3 && y >= 1 && y < 3) {
                const auto& expected = pixels[(y - 1) * 2 + x - 1];
                const float rgb[]{unsigned_float(value & 0x7ff, 6), unsigned_float((value >> 11) & 0x7ff, 6),
                    unsigned_float(value >> 22, 5)};
                for (unsigned c = 0; c < 3; ++c) {
                    check(std::abs(rgb[c] - expected[c]) < 0.001f, "linear HDR values and point texel mapping preserved");
                    check(std::abs(unsigned_float(half[c], 10) - expected[c]) < 0.001f, "RGBA16F mirror values preserved");
                }
                check(std::abs(unsigned_float(half[3], 10) - expected[3]) < 0.001f, "mirror alpha preserved");
            } else check(value == 0 && half[0] == 0 && half[1] == 0 && half[2] == 0 && half[3] == 0,
                "pixels outside destination rectangle preserved");
        }
    }
    check(rsf_native_reinsert(context.Get(), source.Get(), direct.Get(), direct.Get(), rect), "same-format copy with aliased mirror");
    const auto exact = read(device.Get(), context.Get(), direct.Get(), 16);
    check(exact.size() == 320, "same-format readback completed");
    if (exact.size() == 320) for (UINT y = 0; y < 2; ++y)
        check(!std::memcmp(exact.data() + ((y + 1) * 5 + 1) * 16, pixels[y * 2].data(), 32), "same-format copy is bit exact");
    check(rsf_native_reinsert(context.Get(), source.Get(), target.Get(), mirror.Get(), rect), "cached conversion resources reused");
    rsf_native_reinsert_release();
    check(rsf_native_reinsert(context.Get(), source.Get(), target.Get(), nullptr, rect), "conversion recreated after retirement");
    rsf_native_reinsert_release();
    std::printf("Native reinsertion: %d failures\n", failures);
    return failures ? 1 : 0;
}
