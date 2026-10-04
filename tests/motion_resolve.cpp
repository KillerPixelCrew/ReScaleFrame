// SPDX-License-Identifier: GPL-3.0-only
#include <rescaleframe/motion_resolve.h>
#include <d3d11.h>
#include <DirectXPackedVector.h>
#include <wrl/client.h>
#include <cmath>
#include <cstdio>
#include <initializer_list>
#include <limits>
using Microsoft::WRL::ComPtr;
int main()
{
    ComPtr<ID3D11Device> device; ComPtr<ID3D11DeviceContext> context;
    if (FAILED(D3D11CreateDevice(nullptr, D3D_DRIVER_TYPE_WARP, nullptr, 0, nullptr, 0,
                                 D3D11_SDK_VERSION, &device, nullptr, &context))) return 77;
    float motion[8 * 8 * 2], depth[8 * 8];
    for (uint32_t i = 0; i < 64; ++i) { motion[i * 2] = motion[i * 2 + 1] = -1000; depth[i] = 0.5f; }
    motion[0] = motion[1] = 0;
    motion[2] = 0.5f; motion[3] = -0.5f;
    ComPtr<ID3D11Texture2D> sources[2];
    for (uint32_t i = 0; i < 2; ++i) {
        D3D11_TEXTURE2D_DESC desc{};
        desc.Width = desc.Height = 8; desc.MipLevels = desc.ArraySize = desc.SampleDesc.Count = 1;
        desc.Format = i == 0 ? DXGI_FORMAT_R32G32_FLOAT : DXGI_FORMAT_R32_FLOAT;
        desc.BindFlags = D3D11_BIND_SHADER_RESOURCE;
        D3D11_SUBRESOURCE_DATA data{}; data.pSysMem = i == 0 ? motion : depth; data.SysMemPitch = i == 0 ? 64 : 32;
        if (FAILED(device->CreateTexture2D(&desc, &data, &sources[i]))) return 1;
    }
    rsf_motion_resolve* pass = nullptr;
    if (!rsf_motion_resolve_create(device.Get(), 8, 8, &pass)) { std::puts("create failed"); return 1; }
    rsf_motion_resolve_params params{}; params.struct_size = sizeof(params);
    params.clip_to_previous[0] = params.clip_to_previous[5] = params.clip_to_previous[10] = params.clip_to_previous[15] = 1;
    params.clip_to_previous[12] = 0.25f;
    params.decoded_to_pixels[0] = 4; params.decoded_to_pixels[1] = -4;
    params.sentinel = -1000; params.has_sentinel = 1;
    bool passed = rsf_motion_resolve_run(pass, context.Get(), sources[0].Get(), sources[1].Get(), &params) != 0;
    for (uint32_t i = 0; i < 2; ++i) {
        auto* output = static_cast<ID3D11Texture2D*>(i == 0 ? rsf_motion_resolve_motion(pass) : rsf_motion_resolve_depth(pass));
        D3D11_TEXTURE2D_DESC desc{}; output->GetDesc(&desc);
        desc.BindFlags = 0; desc.Usage = D3D11_USAGE_STAGING; desc.CPUAccessFlags = D3D11_CPU_ACCESS_READ;
        ComPtr<ID3D11Texture2D> staging;
        if (FAILED(device->CreateTexture2D(&desc, nullptr, &staging))) return 1;
        context->CopyResource(staging.Get(), output);
        D3D11_MAPPED_SUBRESOURCE mapped{};
        if (FAILED(context->Map(staging.Get(), 0, D3D11_MAP_READ, 0, &mapped))) return 1;
        if (i == 0) {
            float values[6];
            for (uint32_t v = 0; v < 6; ++v)
                values[v] = DirectX::PackedVector::XMConvertHalfToFloat(static_cast<const uint16_t*>(mapped.pData)[v]);
            std::fprintf(stderr, "motion: %g %g / %g %g / %g %g\n",
                values[0], values[1], values[2], values[3], values[4], values[5]);
            // Valid zero remains zero; written motion becomes (2,2); the unwritten pixel uses
            // the +0.25 NDC camera translation, becoming (1,0).
            passed &= values[0] == 0 && values[1] == 0 && values[2] == 2 && values[3] == 2 &&
                      values[4] == 1 && values[5] == 0;
        } else passed &= static_cast<const float*>(mapped.pData)[0] == 0.5f;
        context->Unmap(staging.Get(), 0);
    }
    // A depth layer: previous.x = ndc.x + 0.5 * depth, so only the reprojection depth moves a
    // pixel. Scene depth is 0 (sky). Pixel 2 has a nearer layer at 0.5 and moves one pixel;
    // pixel 3 has none and stays; pixel 0 is written and ignores the layer.
    float sky[64]{}, layer[64]{};
    layer[0] = layer[2] = 0.5f;
    ComPtr<ID3D11Texture2D> depths[2]; // sky, layer
    for (uint32_t i = 0; i < 2; ++i) {
        D3D11_TEXTURE2D_DESC desc{};
        desc.Width = desc.Height = 8; desc.MipLevels = desc.ArraySize = desc.SampleDesc.Count = 1;
        desc.Format = DXGI_FORMAT_R32_FLOAT; desc.BindFlags = D3D11_BIND_SHADER_RESOURCE;
        D3D11_SUBRESOURCE_DATA data{}; data.pSysMem = i == 0 ? sky : layer; data.SysMemPitch = 32;
        if (FAILED(device->CreateTexture2D(&desc, &data, &depths[i]))) return 1;
    }
    rsf_motion_resolve_params layered = params;
    layered.clip_to_previous[12] = 0; layered.clip_to_previous[8] = 0.5f;
    layered.depth_layer = depths[1].Get();
    passed &= rsf_motion_resolve_run(pass, context.Get(), sources[0].Get(), depths[0].Get(), &layered) != 0;
    {
        auto* output = static_cast<ID3D11Texture2D*>(rsf_motion_resolve_motion(pass));
        D3D11_TEXTURE2D_DESC desc{}; output->GetDesc(&desc);
        desc.BindFlags = 0; desc.Usage = D3D11_USAGE_STAGING; desc.CPUAccessFlags = D3D11_CPU_ACCESS_READ;
        ComPtr<ID3D11Texture2D> staging;
        if (FAILED(device->CreateTexture2D(&desc, nullptr, &staging))) return 1;
        context->CopyResource(staging.Get(), output);
        D3D11_MAPPED_SUBRESOURCE mapped{};
        if (FAILED(context->Map(staging.Get(), 0, D3D11_MAP_READ, 0, &mapped))) return 1;
        float values[8];
        for (uint32_t v = 0; v < 8; ++v)
            values[v] = DirectX::PackedVector::XMConvertHalfToFloat(static_cast<const uint16_t*>(mapped.pData)[v]);
        std::fprintf(stderr, "layered: %g %g / %g %g / %g %g\n", values[0], values[1], values[4], values[5], values[6], values[7]);
        passed &= values[0] == 0 && values[1] == 0 && std::fabs(values[4] - 1) < 1e-5f && values[5] == 0 &&
                  values[6] == 0 && values[7] == 0;
        context->Unmap(staging.Get(), 0);
    }
    params.clip_to_previous[0] = std::numeric_limits<float>::quiet_NaN();
    passed &= !rsf_motion_resolve_run(pass, context.Get(), sources[0].Get(), sources[1].Get(), &params);
    rsf_motion_resolve_destroy(pass);
    std::puts(passed ? "motion_resolve passed" : "motion_resolve FAILED");
    return passed ? 0 : 1;
}
