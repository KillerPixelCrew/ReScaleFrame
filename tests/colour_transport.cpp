// SPDX-License-Identifier: GPL-3.0-only
/**
 * @file
 * Check the display-range colour transport with WARP texture readback.
 * Analytic encoding, inverse decoding and saturated-highlight recovery are compared
 * to known source values. Identity reconstruction isolates transport arithmetic;
 * FP16 quantisation determines the round-trip tolerance.
 */
#include <rescaleframe/colour_transport.h>
#include <d3d11.h>
#include <DirectXPackedVector.h>
#include <wrl/client.h>
#include <cmath>
#include <cstdio>
#include <cstring>
using Microsoft::WRL::ComPtr;
namespace {
ComPtr<ID3D11Device> device; ComPtr<ID3D11DeviceContext> context;
ComPtr<ID3D11Texture2D> texture(uint32_t width, DXGI_FORMAT format, const void* data, UINT pitch, UINT bind)
{
    D3D11_TEXTURE2D_DESC desc{};
    desc.Width = width; desc.Height = 1; desc.MipLevels = desc.ArraySize = desc.SampleDesc.Count = 1;
    desc.Format = format; desc.BindFlags = bind;
    D3D11_SUBRESOURCE_DATA initial{}; initial.pSysMem = data; initial.SysMemPitch = pitch;
    ComPtr<ID3D11Texture2D> result; device->CreateTexture2D(&desc, data ? &initial : nullptr, &result);
    return result;
}
// Red channel of each texel, from FP16 or FP32 RGBA.
void read_red(ID3D11Texture2D* source, float* out, uint32_t count)
{
    D3D11_TEXTURE2D_DESC desc{}; source->GetDesc(&desc);
    const bool half = desc.Format == DXGI_FORMAT_R16G16B16A16_FLOAT;
    desc.BindFlags = 0; desc.Usage = D3D11_USAGE_STAGING; desc.CPUAccessFlags = D3D11_CPU_ACCESS_READ;
    ComPtr<ID3D11Texture2D> staging; device->CreateTexture2D(&desc, nullptr, &staging);
    context->CopyResource(staging.Get(), source);
    D3D11_MAPPED_SUBRESOURCE mapped{}; context->Map(staging.Get(), 0, D3D11_MAP_READ, 0, &mapped);
    for (uint32_t i = 0; i < count; ++i)
        out[i] = half ? DirectX::PackedVector::XMConvertHalfToFloat(static_cast<const uint16_t*>(mapped.pData)[i * 4])
                      : static_cast<const float*>(mapped.pData)[i * 4];
    context->Unmap(staging.Get(), 0);
}
}
int main()
{
    if (FAILED(D3D11CreateDevice(nullptr, D3D_DRIVER_TYPE_WARP, nullptr, 0, nullptr, 0,
                                 D3D11_SDK_VERSION, &device, nullptr, &context))) return 77;
    constexpr uint32_t count = 6;
    constexpr float values[count] = {0.0f, 0.001f, 0.02f, 0.18f, 1.0f, 10.0f};
    const float exposure_value = 5;
    float source[count * 4]{};
    for (uint32_t i = 0; i < count; ++i) { source[i * 4] = source[i * 4 + 1] = source[i * 4 + 2] = values[i]; source[i * 4 + 3] = 1; }
    auto colour = texture(count, DXGI_FORMAT_R32G32B32A32_FLOAT, source, sizeof(source), D3D11_BIND_SHADER_RESOURCE);
    auto exposure = texture(1, DXGI_FORMAT_R32_FLOAT, &exposure_value, 4, D3D11_BIND_SHADER_RESOURCE);
    rsf_colour_transport* transport = nullptr;
    void* encoded = nullptr;
    if (!rsf_colour_transport_create(device.Get(), &transport) ||
        !rsf_colour_transport_encode(transport, context.Get(), colour.Get(), exposure.Get(), count, 1, &encoded)) {
        std::puts("colour_transport FAILED to create or encode"); return 1;
    }
    bool passed = true;
    float display[count]{}; read_red(static_cast<ID3D11Texture2D*>(encoded), display, count);
    for (uint32_t i = 0; i < count; ++i) {
        const float scaled = values[i] * exposure_value;
        const float expected = std::pow(scaled / (scaled + 4.0f), 1 / 2.2f);
        passed &= std::fabs(display[i] - expected) < 1e-3f && display[i] >= 0 && display[i] < 1;
    }
    // Stand in for the reconstruction with an identity: copy the encoded image into an FP32 target.
    float staged[count * 4]{};
    for (uint32_t i = 0; i < count; ++i) { staged[i * 4] = staged[i * 4 + 1] = staged[i * 4 + 2] = display[i]; staged[i * 4 + 3] = 1; }
    auto output = texture(count, DXGI_FORMAT_R32G32B32A32_FLOAT, staged, sizeof(staged), D3D11_BIND_SHADER_RESOURCE | D3D11_BIND_UNORDERED_ACCESS);
    passed &= rsf_colour_transport_decode(transport, context.Get(), output.Get(), exposure.Get(), count, 1,
        nullptr, 0, 0, nullptr) != 0;
    float decoded[count]{}; read_red(output.Get(), decoded, count);
    for (uint32_t i = 0; i < count; ++i) {
        std::fprintf(stderr, "%g -> %g -> %g\n", values[i], display[i], decoded[i]);
        // FP16 storage of the encoded value bounds the round trip: within 2% up to 50 exposed units.
        passed &= std::fabs(decoded[i] - values[i]) <= 0.02f * values[i] + 1e-5f;
    }
    // A reconstruction saturated at 1 recovers the scene's own highlight, never darker.
    const float saturated[4]{1, 1, 1, 1}, bright[4]{500, 500, 500, 1};
    auto pixel = texture(1, DXGI_FORMAT_R32G32B32A32_FLOAT, saturated, 16, D3D11_BIND_SHADER_RESOURCE | D3D11_BIND_UNORDERED_ACCESS);
    auto scene = texture(1, DXGI_FORMAT_R32G32B32A32_FLOAT, bright, 16, D3D11_BIND_SHADER_RESOURCE);
    const float jitter[2]{};
    passed &= rsf_colour_transport_decode(transport, context.Get(), pixel.Get(), exposure.Get(), 1, 1,
        scene.Get(), 1, 1, jitter) != 0;
    float recovered = 0; read_red(pixel.Get(), &recovered, 1);
    std::fprintf(stderr, "saturated -> %g\n", recovered);
    passed &= std::fabs(recovered - 500) < 0.5f;
    rsf_colour_transport_destroy(transport);
    std::puts(passed ? "colour_transport passed" : "colour_transport FAILED");
    return passed ? 0 : 1;
}
