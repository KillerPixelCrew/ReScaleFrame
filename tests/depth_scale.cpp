// SPDX-License-Identifier: MIT
#include <rescaleframe/depth_scale.h>
#include "../runtime/graphics/src/depth_scale_private.h"
#include <d3d11.h>
#include <wrl/client.h>
#include <algorithm>
#include <array>
#include <cmath>
#include <cstdio>
#include <cstring>
#include <limits>
#include <thread>
#include <vector>

namespace {
using Microsoft::WRL::ComPtr;
int failures{};
void check(bool value, const char* name) { if (!value) { ++failures; std::fprintf(stderr, "FAIL %s\n", name); } }
void log(void*, const char* message) { std::fprintf(stderr, "%s\n", message); }
std::vector<uint32_t> read(ID3D11Device* device, ID3D11DeviceContext* context, ID3D11Texture2D* source) {
    D3D11_TEXTURE2D_DESC d{}; source->GetDesc(&d);
    d.Usage = D3D11_USAGE_STAGING; d.BindFlags = d.MiscFlags = 0; d.CPUAccessFlags = D3D11_CPU_ACCESS_READ;
    ComPtr<ID3D11Texture2D> staging;
    if (FAILED(device->CreateTexture2D(&d, nullptr, &staging))) return {};
    context->CopyResource(staging.Get(), source);
    D3D11_MAPPED_SUBRESOURCE m{};
    if (FAILED(context->Map(staging.Get(), 0, D3D11_MAP_READ, 0, &m))) return {};
    std::vector<uint32_t> result(size_t(d.Width) * d.Height);
    for (UINT y = 0; y < d.Height; ++y)
        std::memcpy(result.data() + size_t(y) * d.Width, static_cast<const unsigned char*>(m.pData) + size_t(y) * m.RowPitch,
            size_t(d.Width) * sizeof(uint32_t));
    context->Unmap(staging.Get(), 0); return result;
}
}
int main() {
    ComPtr<ID3D11Device> device; ComPtr<ID3D11DeviceContext> context;
    const D3D_FEATURE_LEVEL level = D3D_FEATURE_LEVEL_11_0;
    if (FAILED(D3D11CreateDevice(nullptr, D3D_DRIVER_TYPE_WARP, nullptr, 0, &level, 1, D3D11_SDK_VERSION,
        &device, nullptr, &context))) return 77;
    std::array<uint32_t, 256> pixels{};
    for (uint32_t i = 0; i < pixels.size(); ++i) pixels[i] = (i << 24) | ((i * 104729u) & 0xffffff);
    pixels[1] = 0x01ffffff; pixels[2] = 0x02800000; pixels[3] = 0x037fffff; pixels[4] = 0x04fffffe;
    D3D11_TEXTURE2D_DESC d{}; d.Width = d.Height = 16; d.Format = DXGI_FORMAT_R24G8_TYPELESS;
    d.MipLevels = d.ArraySize = d.SampleDesc.Count = 1; d.BindFlags = D3D11_BIND_DEPTH_STENCIL | D3D11_BIND_SHADER_RESOURCE;
    D3D11_SUBRESOURCE_DATA initial{pixels.data(), 16 * sizeof(uint32_t), 0};
    ComPtr<ID3D11Texture2D> texture;
    check(SUCCEEDED(device->CreateTexture2D(&d, &initial, &texture)), "packed depth/stencil fixture");
    if (!texture) return 1;
    const std::vector<uint32_t> original(pixels.begin(), pixels.end());
    rsf_depth_scale* scale{};
    rsf_depth_scale_setup setup{sizeof(setup), RSF_DEPTH_SCALE_ABI_VERSION, log, nullptr};
    setup.struct_size = sizeof(setup) - 1;
    check(rsf_depth_scale_create(context.Get(), &setup, &scale) == RSF_DEPTH_SCALE_INVALID_ARGUMENT && !scale, "short setup refused");
    setup.struct_size = sizeof(setup); setup.abi_version++;
    check(rsf_depth_scale_create(context.Get(), &setup, &scale) == RSF_DEPTH_SCALE_ABI_MISMATCH && !scale, "setup ABI refused");
    setup.abi_version = RSF_DEPTH_SCALE_ABI_VERSION;
    ComPtr<ID3D11DeviceContext> deferred; device->CreateDeferredContext(0, &deferred);
    check(rsf_depth_scale_create(deferred.Get(), &setup, &scale) == RSF_DEPTH_SCALE_INVALID_ARGUMENT && !scale, "deferred context refused");
    check(rsf_depth_scale_create(context.Get(), &setup, &scale) == 0 && scale, "create transaction");
    if (!scale) return 1;
    const D3D11_VIEWPORT reserve_viewport{1,2,7,9,.2f,.8f}; context->RSSetViewports(1,&reserve_viewport);
    check(rsf_depth_scale_reserve(scale,texture.Get()),"allocation-only depth readiness");
    D3D11_VIEWPORT unchanged{}; UINT viewport_count=1; context->RSGetViewports(&viewport_count,&unchanged);
    check(!std::memcmp(&unchanged,&reserve_viewport,sizeof(unchanged)) && read(device.Get(),context.Get(),texture.Get())==original,
        "reserve leaves native packed contents and viewport unchanged");
    auto invalid_desc=d; invalid_desc.Format=DXGI_FORMAT_R32_TYPELESS;
    ComPtr<ID3D11Texture2D> unsupported;
    check(SUCCEEDED(device->CreateTexture2D(&invalid_desc,nullptr,&unsupported)),"unsupported depth reserve fixture");
    check(!rsf_depth_scale_reserve(scale,unsupported.Get()) && read(device.Get(),context.Get(),texture.Get())==original,
        "unsupported reserve refuses without changing native contents");
    rsf_depth_scale_frame frame{sizeof(frame), RSF_DEPTH_SCALE_ABI_VERSION, texture.Get(), {0, 0, 16, 16}, {0, 0, 16, 16}, {0, 0}};
    check(rsf_depth_scale_commit(scale) == RSF_DEPTH_SCALE_INVALID_ARGUMENT, "commit requires prepared image");
    frame.struct_size--;
    check(rsf_depth_scale_prepare(scale, &frame) == RSF_DEPTH_SCALE_INVALID_ARGUMENT, "short frame refused");
    frame.struct_size++; frame.abi_version++;
    check(rsf_depth_scale_prepare(scale, &frame) == RSF_DEPTH_SCALE_ABI_MISMATCH, "frame ABI refused");
    frame.abi_version--; frame.source_rect[2] = 17;
    check(rsf_depth_scale_prepare(scale, &frame) == RSF_DEPTH_SCALE_INVALID_ARGUMENT, "out-of-bounds source refused");
    frame.source_rect[2] = 16; frame.jitter_pixels[0] = std::numeric_limits<float>::quiet_NaN();
    check(rsf_depth_scale_prepare(scale, &frame) == RSF_DEPTH_SCALE_INVALID_ARGUMENT, "nonfinite sampling refused");
    frame.jitter_pixels[0] = 0;
    check(read(device.Get(), context.Get(), texture.Get()) == original, "refusal leaves native allocation unchanged");

    // Bound native depth, predicate and an append counter expose destructive inherited state.
    ComPtr<ID3D11DepthStencilView> dsv;
    D3D11_DEPTH_STENCIL_VIEW_DESC vd{}; vd.Format = DXGI_FORMAT_D24_UNORM_S8_UINT; vd.ViewDimension = D3D11_DSV_DIMENSION_TEXTURE2D;
    check(SUCCEEDED(device->CreateDepthStencilView(texture.Get(), &vd, &dsv)), "native DSV");
    ComPtr<ID3D11Buffer> append, count, count_read;
    D3D11_BUFFER_DESC bd{}; bd.ByteWidth = 64; bd.Usage = D3D11_USAGE_DEFAULT;
    bd.BindFlags = D3D11_BIND_UNORDERED_ACCESS; bd.MiscFlags = D3D11_RESOURCE_MISC_BUFFER_STRUCTURED; bd.StructureByteStride = 4;
    check(SUCCEEDED(device->CreateBuffer(&bd, nullptr, &append)), "append buffer");
    D3D11_UNORDERED_ACCESS_VIEW_DESC ud{}; ud.Format = DXGI_FORMAT_UNKNOWN; ud.ViewDimension = D3D11_UAV_DIMENSION_BUFFER;
    ud.Buffer.NumElements = 16; ud.Buffer.Flags = D3D11_BUFFER_UAV_FLAG_APPEND;
    ComPtr<ID3D11UnorderedAccessView> uav;
    check(SUCCEEDED(device->CreateUnorderedAccessView(append.Get(), &ud, &uav)), "append UAV");
    bd = {}; bd.ByteWidth = 4; bd.Usage = D3D11_USAGE_DEFAULT;
    check(SUCCEEDED(device->CreateBuffer(&bd, nullptr, &count)), "counter copy destination");
    bd.Usage = D3D11_USAGE_STAGING; bd.CPUAccessFlags = D3D11_CPU_ACCESS_READ;
    check(SUCCEEDED(device->CreateBuffer(&bd, nullptr, &count_read)), "counter readback");
    ComPtr<ID3D11Predicate> predicate;
    const D3D11_QUERY_DESC qd{D3D11_QUERY_OCCLUSION_PREDICATE, 0};
    check(SUCCEEDED(device->CreatePredicate(&qd, &predicate)), "empty occlusion predicate");
    if (!dsv || !uav || !count || !count_read || !predicate) { rsf_depth_scale_destroy(scale); return 1; }
    context->Begin(predicate.Get()); context->End(predicate.Get()); context->SetPredication(predicate.Get(), TRUE);
    context->OMSetRenderTargets(0, nullptr, dsv.Get());
    const D3D11_VIEWPORT vp{2, 4, 3, 5, .2f, .8f}; context->RSSetViewports(1, &vp);
    const UINT counter = 7; auto* raw_uav = uav.Get(); context->CSSetUnorderedAccessViews(4, 1, &raw_uav, &counter);
    context->IASetPrimitiveTopology(D3D11_PRIMITIVE_TOPOLOGY_POINTLIST);

    check(rsf_depth_scale_prepare(scale, &frame) == 0, "identity prepare");
    check(rsf_depth_scale_prepare(scale, &frame) == RSF_DEPTH_SCALE_BUSY, "overlapping preparation refused");
    check(read(device.Get(), context.Get(), texture.Get()) == original, "prepare writes scratch only");
    check(rsf_depth_scale_commit(scale) == 0, "identity commit");
    check(read(device.Get(), context.Get(), texture.Get()) == original, "all 256 stencil values and 24-bit depths survive rendering");
    check(rsf_depth_scale_commit(scale) == RSF_DEPTH_SCALE_BUSY, "double commit refused");
    ComPtr<ID3D11DepthStencilView> current_dsv; context->OMGetRenderTargets(0, nullptr, &current_dsv);
    ComPtr<ID3D11UnorderedAccessView> current_uav; context->CSGetUnorderedAccessViews(4, 1, &current_uav);
    ComPtr<ID3D11Predicate> current_predicate; BOOL predicate_value{}; context->GetPredication(&current_predicate, &predicate_value);
    D3D11_VIEWPORT current_vp{}; UINT viewports = 1; context->RSGetViewports(&viewports, &current_vp);
    D3D11_PRIMITIVE_TOPOLOGY topology{}; context->IAGetPrimitiveTopology(&topology);
    check(current_dsv.Get() == dsv.Get() && current_uav.Get() == uav.Get() && current_predicate.Get() == predicate.Get() && predicate_value,
        "native depth/UAV/predication bindings restored");
    check(!std::memcmp(&vp, &current_vp, sizeof(vp)) && topology == D3D11_PRIMITIVE_TOPOLOGY_POINTLIST, "viewport and topology restored");
    context->SetPredication(nullptr, FALSE);
    context->CopyStructureCount(count.Get(), 0, uav.Get()); context->CopyResource(count_read.Get(), count.Get());
    D3D11_MAPPED_SUBRESOURCE mapped{};
    if (SUCCEEDED(context->Map(count_read.Get(), 0, D3D11_MAP_READ, 0, &mapped))) {
        check(*static_cast<const uint32_t*>(mapped.pData) == counter, "append counter retained"); context->Unmap(count_read.Get(), 0);
    } else check(false, "counter readback completed");
    context->ClearState();
    check(rsf_depth_scale_restore(scale) == 0, "identity restore");

    // Offset rectangles, unequal axes and opposite jitter signs expose mapping and edge clamping.
    const int32_t src[]{2, 3, 14, 13}, dst[]{1, 1, 16, 15};
    std::memcpy(frame.source_rect, src, sizeof(src)); std::memcpy(frame.output_rect, dst, sizeof(dst));
    frame.jitter_pixels[0] = .35f; frame.jitter_pixels[1] = -.6f;
    check(rsf_depth_scale_prepare(scale, &frame) == 0 && rsf_depth_scale_commit(scale) == 0, "scaled depth/stencil commit");
    auto scaled = read(device.Get(), context.Get(), texture.Get());
    check(scaled.size() == original.size(), "scaled readback");
    if (scaled.size() == original.size()) for (int y = 0; y < 16; ++y) for (int x = 0; x < 16; ++x) {
        uint32_t expected = original[size_t(y) * 16 + x];
        if (x >= dst[0] && x < dst[2] && y >= dst[1] && y < dst[3]) {
            const int sx = std::clamp(int(std::floor(src[0] + (float(x) + .5f - dst[0]) / float(dst[2] - dst[0]) * float(src[2] - src[0]) - frame.jitter_pixels[0])), src[0], src[2] - 1);
            const int sy = std::clamp(int(std::floor(src[1] + (float(y) + .5f - dst[1]) / float(dst[3] - dst[1]) * float(src[3] - src[1]) - frame.jitter_pixels[1])), src[1], src[3] - 1);
            expected = original[size_t(sy) * 16 + sx];
        }
        check(scaled[size_t(y) * 16 + x] == expected, "nearest depth/stencil mapping and untouched outside pixels");
    }
    std::thread other([&] {
        check(rsf_depth_scale_prepare(scale, &frame) == RSF_DEPTH_SCALE_BUSY, "foreign prepare refused");
        check(rsf_depth_scale_commit(scale) == RSF_DEPTH_SCALE_BUSY, "foreign commit refused");
        check(rsf_depth_scale_restore(scale) == RSF_DEPTH_SCALE_BUSY, "foreign restore refused");
        check(rsf_depth_scale_destroy(scale) == RSF_DEPTH_SCALE_BUSY, "foreign destroy retains transaction");
    }); other.join();
    check(rsf_depth_scale_restore(scale) == 0 && read(device.Get(), context.Get(), texture.Get()) == original,
        "original contents restored bit for bit after scaled use");
    check(rsf_depth_scale_prepare(scale, &frame) == 0 && rsf_depth_scale_restore(scale) == 0 &&
        read(device.Get(), context.Get(), texture.Get()) == original, "uncommitted preparation cancels without writes");
    check(rsf_depth_scale_prepare(scale, &frame) == 0 && rsf_depth_scale_commit(scale) == 0 && rsf_depth_scale_destroy(scale) == 0,
        "destroy restores a committed transaction");
    check(read(device.Get(), context.Get(), texture.Get()) == original, "destroy restoration preserves original contents");
    return failures ? 1 : 0;
}
