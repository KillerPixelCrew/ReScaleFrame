// SPDX-License-Identifier: GPL-3.0-only
#include "../runtime/orchestrator/src/auxiliary_sr.h"
#include "../runtime/orchestrator/src/native_regions.h"
#include <wrl/client.h>
#include <array>
#include <vector>
#include <cmath>
#include <algorithm>
#include <cstring>
#include <cstdio>

namespace {
using Microsoft::WRL::ComPtr;
int failures{};
void check(bool value, const char* label) { if (!value) { ++failures; std::fprintf(stderr, "FAIL %s\n", label); } }
ComPtr<ID3D11Texture2D> texture(ID3D11Device* device, DXGI_FORMAT format, const void* pixels, UINT stride, UINT bind,
    D3D11_USAGE usage = D3D11_USAGE_DEFAULT) {
    D3D11_TEXTURE2D_DESC d{}; d.Width = 8; d.Height = 6; d.Format = format; d.MipLevels = d.ArraySize = d.SampleDesc.Count = 1;
    d.BindFlags = bind; d.Usage = usage; const D3D11_SUBRESOURCE_DATA data{pixels, 8 * stride, 0};
    ComPtr<ID3D11Texture2D> result; check(SUCCEEDED(device->CreateTexture2D(&d, &data, &result)), "fixture texture"); return result;
}
std::vector<unsigned char> read(ID3D11Device* device, ID3D11DeviceContext* context, ID3D11Texture2D* source, UINT stride) {
    D3D11_TEXTURE2D_DESC d{}; source->GetDesc(&d); d.Usage = D3D11_USAGE_STAGING; d.BindFlags = d.MiscFlags = 0; d.CPUAccessFlags = D3D11_CPU_ACCESS_READ;
    ComPtr<ID3D11Texture2D> staging; if (FAILED(device->CreateTexture2D(&d, nullptr, &staging))) return {};
    context->CopyResource(staging.Get(), source); D3D11_MAPPED_SUBRESOURCE m{};
    if (FAILED(context->Map(staging.Get(), 0, D3D11_MAP_READ, 0, &m))) return {};
    std::vector<unsigned char> bytes(size_t(d.Width) * d.Height * stride);
    for (UINT y = 0; y < d.Height; ++y) std::memcpy(bytes.data() + size_t(y) * d.Width * stride,
        static_cast<const unsigned char*>(m.pData) + size_t(y) * m.RowPitch, size_t(d.Width) * stride);
    context->Unmap(staging.Get(), 0); return bytes;
}
}
int main() {
    ComPtr<ID3D11Device> device; ComPtr<ID3D11DeviceContext> context; const D3D_FEATURE_LEVEL level = D3D_FEATURE_LEVEL_11_0;
    if (FAILED(D3D11CreateDevice(nullptr, D3D_DRIVER_TYPE_WARP, nullptr, 0, &level, 1, D3D11_SDK_VERSION, &device, nullptr, &context))) return 77;
    std::array<uint32_t, 48> depths{}, pixels{}; std::array<float, 192> colour{}; colour.fill(.25f);
    for (uint32_t i = 0; i < depths.size(); ++i) {
        depths[i] = (((i * 17) & 255) << 24) | ((i * 237113) & 0xffffff);
        pixels[i] = i | ((i * 3) << 8) | ((i * 5) << 16) | ((255 - i * 2) << 24);
    }
    auto depth = texture(device.Get(), DXGI_FORMAT_R24G8_TYPELESS, depths.data(), 4, D3D11_BIND_DEPTH_STENCIL);
    auto aux = texture(device.Get(), DXGI_FORMAT_R8G8B8A8_UNORM, pixels.data(), 4, D3D11_BIND_RENDER_TARGET);
    auto main = texture(device.Get(), DXGI_FORMAT_R32G32B32A32_FLOAT, colour.data(), 16, D3D11_BIND_RENDER_TARGET);
    auto immutable = texture(device.Get(), DXGI_FORMAT_R32G32B32A32_FLOAT, colour.data(), 16, D3D11_BIND_SHADER_RESOURCE, D3D11_USAGE_IMMUTABLE);
    if (!depth || !aux || !main || !immutable) return 1;
    const auto depth_before = read(device.Get(), context.Get(), depth.Get(), 4), aux_before = read(device.Get(), context.Get(), aux.Get(), 4);
    const auto colour_before = read(device.Get(), context.Get(), main.Get(), 16);
    rsf_game_render_pass pass{}; pass.color_output = main.Get(); pass.session_id = 1; pass.native_frame = 7; pass.view_key = 91;
    std::array<rsf_d3d11_auxiliary_surface, 2> surfaces{};
    surfaces[0] = {sizeof(surfaces[0]), RSF_D3D11_AUXILIARY_DEPTH_STENCIL, depth.Get(), {0,0,4,3}, {0,0,8,6}, {.25f,-.125f}};
    surfaces[1] = {sizeof(surfaces[1]), RSF_D3D11_AUXILIARY_COLOUR, aux.Get(), {.8f,1.2f,5.75f,4.8f}, {1,0,8,6}, {.35f,-.4f}};
    surfaces[0].flags = surfaces[1].flags = RSF_D3D11_AUXILIARY_RESTORE_CONTENTS;
    const rsf_d3d11_auxiliary_pass request{sizeof(request), RSF_D3D11_HOST_ABI_VERSION, &pass, 2, surfaces.data()};
    AuxiliarySurfaces transaction;
    const D3D11_VIEWPORT viewport{2,1,3,4,0,1}; context->RSSetViewports(1, &viewport);
    context->IASetPrimitiveTopology(D3D11_PRIMITIVE_TOPOLOGY_POINTLIST);
    check(transaction.prepare(context.Get(), pass, request, nullptr, nullptr), "prepare depth and fractional colour region");
    check(transaction.pending() && !transaction.prepare(context.Get(), pass, request, nullptr, nullptr), "overlap refused without cancelling the lease");
    check(read(device.Get(), context.Get(), depth.Get(), 4) == depth_before && read(device.Get(), context.Get(), aux.Get(), 4) == aux_before &&
        read(device.Get(), context.Get(), main.Get(), 16) == colour_before, "preparation leaves every native resource untouched");
    D3D11_VIEWPORT saved{}; UINT count = 1; context->RSGetViewports(&count, &saved);
    D3D11_PRIMITIVE_TOPOLOGY topology{}; context->IAGetPrimitiveTopology(&topology);
    check(!std::memcmp(&saved, &viewport, sizeof(saved)) && topology == D3D11_PRIMITIVE_TOPOLOGY_POINTLIST, "preparation restores native bindings");
    rsf_d3d11_auxiliary_scope scope{sizeof(scope), RSF_D3D11_HOST_ABI_VERSION, 1, 7, 91};
    check(transaction.matches(scope), "lease identity matches"); ++scope.native_frame;
    check(!transaction.matches(scope), "different native frame refused"); --scope.native_frame;
    ComPtr<ID3D11RenderTargetView> target; check(SUCCEEDED(device->CreateRenderTargetView(main.Get(), nullptr, &target)), "SR destination RTV");
    const float reconstructed[]{7,3,2,.5f}; context->ClearRenderTargetView(target.Get(), reconstructed);
    check(transaction.commit(), "commit complete SR auxiliaries");
    const auto scaled_depth = read(device.Get(), context.Get(), depth.Get(), 4), scaled_aux = read(device.Get(), context.Get(), aux.Get(), 4);
    check(scaled_depth.size() == 192 && scaled_aux.size() == 192, "committed readbacks");
    if (scaled_depth.size() == 192 && scaled_aux.size() == 192) for (int y = 0; y < 6; ++y) for (int x = 0; x < 8; ++x) {
        const int dx = std::clamp(int(std::floor((float(x)+.5f)/8*4-.25f)), 0, 3);
        const int dy = std::clamp(int(std::floor((float(y)+.5f)/6*3+.125f)), 0, 2);
        uint32_t actual_depth{}, actual_aux{};
        std::memcpy(&actual_depth, scaled_depth.data() + (y*8+x)*4, 4); std::memcpy(&actual_aux, scaled_aux.data() + (y*8+x)*4, 4);
        check(actual_depth == depths[size_t(dy)*8+dx], "depth and stencil point mapping");
        uint32_t expected = pixels[size_t(y)*8+x];
        if (x >= 1) {
            const int sx = std::clamp(int(std::floor(.8f+(float(x)+.5f-1)/7*4.95f-.35f)), 1, 5);
            const int sy = std::clamp(int(std::floor(1.2f+(float(y)+.5f)/6*3.6f+.4f)), 1, 4);
            expected = pixels[size_t(sy)*8+sx];
        }
        check(actual_aux == expected, "fractional viewport, jitter, alpha and outside pixels");
    }
    const auto sr_colour = read(device.Get(), context.Get(), main.Get(), 16);
    check(transaction.restore() && !transaction.pending(), "restore lease after final consumer");
    check(read(device.Get(), context.Get(), depth.Get(), 4) == depth_before && read(device.Get(), context.Get(), aux.Get(), 4) == aux_before,
        "every original auxiliary restored bit for bit");
    check(read(device.Get(), context.Get(), main.Get(), 16) == sr_colour, "successful SR colour survives auxiliary restoration");
    check(transaction.prepare(context.Get(), pass, request, nullptr, nullptr), "prepare failed-SR rollback");
    const float failed_colour[]{1,0,0,0}; context->ClearRenderTargetView(target.Get(), failed_colour);
    check(transaction.restore() && read(device.Get(), context.Get(), main.Get(), 16) == sr_colour, "failed SR restores original colour");
    pass.color_output_readable = immutable.Get();
    check(!transaction.prepare(context.Get(), pass, request, nullptr, nullptr) &&
        read(device.Get(), context.Get(), main.Get(), 16) == sr_colour, "invalid second destination cannot copy an uninitialized snapshot over native colour");
    pass.color_output_readable = nullptr; surfaces[1].flags = 0;
    check(transaction.prepare(context.Get(), pass, request, nullptr, nullptr) && transaction.commit(), "prepare current-frame auxiliary retirement");
    ComPtr<ID3D11RenderTargetView> aux_target; device->CreateRenderTargetView(aux.Get(), nullptr, &aux_target);
    const float cleared[]{0,0,0,0}; context->ClearRenderTargetView(aux_target.Get(), cleared);
    check(transaction.restore() && read(device.Get(), context.Get(), aux.Get(), 4) == std::vector<unsigned char>(192),
        "later native consumer clear survives auxiliary retirement");
    // A non-origin rectangle in a two-channel UNORM mask must become local R32,
    // with adapter weights and existing independent reactive/bias hints preserved.
    std::array<uint8_t, 96> mask_values{};
    for (size_t i = 0; i < 48; ++i) {
        mask_values[i*2] = uint8_t((i*19)%256);
        mask_values[i*2+1] = uint8_t((i*7)%256);
    }
    auto temporal = texture(device.Get(), DXGI_FORMAT_R8G8_UNORM, mask_values.data(), 2, 0);
    if (!temporal) return 1;
    const int32_t mask_rect[]{2,1,6,4}; const float mask_weights[]{.1f,1,0,0};
    ID3D11Texture2D *reactive = nullptr, *bias = nullptr;
    const bool normalized = rsf_native_regions_temporal_mask(context.Get(), temporal.Get(), mask_rect, mask_weights,
        nullptr, nullptr, &reactive, &bias);
    check(normalized, "normalize native temporal mask");
    if (normalized) {
        D3D11_TEXTURE2D_DESC d{}; reactive->GetDesc(&d);
        check(d.Width == 4 && d.Height == 3 && d.Format == DXGI_FORMAT_R32_FLOAT, "mask render-local R32 format and size");
        auto values = read(device.Get(), context.Get(), reactive, 4);
        check(values == read(device.Get(), context.Get(), bias, 4) && values.size() == 48, "same explicit hint feeds reactive and bias");
        for (size_t y = 0; y < 3; ++y) for (size_t x = 0; x < 4; ++x) {
            const auto native = ((y+1)*8+x+2)*2;
            const float expected = std::min(1.f, .1f*float(mask_values[native])/255.f+float(mask_values[native+1])/255.f);
            float value{};
            if (values.size() == 48) std::memcpy(&value, values.data()+(y*4+x)*4, 4);
            check(std::abs(value-expected) < 1e-5f, "mask channels and crop origin have native numeric semantics");
        }
        // Independent hints must not disappear when an explicit mask is added.
        D3D11_TEXTURE2D_DESC scalar{}; scalar.Width = 4; scalar.Height = 3;
        scalar.MipLevels = scalar.ArraySize = scalar.SampleDesc.Count = 1;
        scalar.Format = DXGI_FORMAT_R32_FLOAT; scalar.BindFlags = D3D11_BIND_SHADER_RESOURCE;
        std::array<float,12> previous_reactive{}, previous_bias{}; previous_reactive.fill(.8f); previous_bias.fill(.6f);
        const D3D11_SUBRESOURCE_DATA rdata{previous_reactive.data(),16,0}, bdata{previous_bias.data(),16,0};
        ComPtr<ID3D11Texture2D> existing_reactive, existing_bias;
        check(SUCCEEDED(device->CreateTexture2D(&scalar,&rdata,&existing_reactive)) &&
            SUCCEEDED(device->CreateTexture2D(&scalar,&bdata,&existing_bias)), "existing temporal hint fixtures");
        const bool combined = rsf_native_regions_temporal_mask(context.Get(), temporal.Get(), mask_rect, mask_weights,
            existing_reactive.Get(),existing_bias.Get(), &reactive,&bias);
        check(combined, "compose explicit and existing temporal hints");
        if (combined) {
            const auto combined_reactive = read(device.Get(),context.Get(),reactive,4);
            const auto combined_bias = read(device.Get(),context.Get(),bias,4);
            check(combined_reactive.size() == 48 && combined_bias.size() == 48, "combined hints readable");
            for (size_t i = 0; i < 12; ++i) if (values.size() == 48 && combined_reactive.size() == 48 && combined_bias.size() == 48) {
                float original{}, rvalue{}, bvalue{};
                std::memcpy(&original,values.data()+i*4,4); std::memcpy(&rvalue,combined_reactive.data()+i*4,4);
                std::memcpy(&bvalue,combined_bias.data()+i*4,4);
                check(std::abs(rvalue-std::max(original,.8f)) < 1e-5f && std::abs(bvalue-std::max(original,.6f)) < 1e-5f,
                    "existing reactive and bias compose independently by maximum");
            }
        }
    }
    const int32_t outside[]{2,1,9,4};
    check(!rsf_native_regions_temporal_mask(context.Get(),temporal.Get(),outside,mask_weights,nullptr,nullptr,&reactive,&bias),
        "out-of-resource temporal mask rectangle refused");
    ComPtr<ID3D11Device> other_device;
    const bool second_device = SUCCEEDED(D3D11CreateDevice(nullptr, D3D_DRIVER_TYPE_WARP, nullptr, 0,
        &level, 1, D3D11_SDK_VERSION, &other_device, nullptr, nullptr));
    check(second_device, "second device for foreign temporal resource checks");
    if (second_device) {
        auto foreign_mask = texture(other_device.Get(), DXGI_FORMAT_R8G8_UNORM, mask_values.data(), 2, 0);
        D3D11_TEXTURE2D_DESC scalar{}; scalar.Width = 4; scalar.Height = 3;
        scalar.MipLevels = scalar.ArraySize = scalar.SampleDesc.Count = 1;
        scalar.Format = DXGI_FORMAT_R32_FLOAT; scalar.BindFlags = D3D11_BIND_SHADER_RESOURCE;
        ComPtr<ID3D11Texture2D> foreign_hint;
        check(SUCCEEDED(other_device->CreateTexture2D(&scalar,nullptr,&foreign_hint)), "foreign scalar hint fixture");
        auto* bound = aux_target.Get(); context->OMSetRenderTargets(1,&bound,nullptr);
        auto* saved_reactive = reactive; auto* saved_bias = bias;
        if (foreign_mask && foreign_hint) {
            check(!rsf_native_regions_temporal_mask(context.Get(),foreign_mask.Get(),mask_rect,mask_weights,
                nullptr,nullptr,&reactive,&bias), "foreign non-SRV mask refused before copy");
            check(!rsf_native_regions_temporal_mask(context.Get(),temporal.Get(),mask_rect,mask_weights,
                foreign_hint.Get(),nullptr,&reactive,&bias), "foreign reactive hint refused");
            check(!rsf_native_regions_temporal_mask(context.Get(),temporal.Get(),mask_rect,mask_weights,
                nullptr,foreign_hint.Get(),&reactive,&bias), "foreign bias hint refused");
        }
        ComPtr<ID3D11RenderTargetView> retained; context->OMGetRenderTargets(1,&retained,nullptr);
        check(retained.Get() == bound && reactive == saved_reactive && bias == saved_bias,
            "foreign resources leave bindings and output pointers unchanged");
        context->OMSetRenderTargets(0,nullptr,nullptr);
    }
    rsf_native_regions_release();
    return failures ? 1 : 0;
}
