// SPDX-License-Identifier: MIT
// Native leaves of the Rust depth-domain owner (depth_domains.rs). Rust owns the guarded engine
// reads, the code guards, source and owner admission, producer raster and target admission, the
// camera math, the composed domains and every refusal decision. This file keeps what needs C++ or
// COM: the TLS slot array, canonical object identity, the D3D11 context, target, rasterizer and
// view facts, and the bounded refusal receipts with their original text. The leaves only report;
// none of them decides admission, and none of them holds a reference after it returns.
#include "depth_domains_platform.h"
#include <windows.h>
#include <d3d11.h>
#include <wrl/client.h>
#include <intrin.h>
#include <cstdio>
#include <cstring>

namespace rsf::fallout4 {
namespace {
using Microsoft::WRL::ComPtr;
// depth_domains.rs compares these enumerators by value.
static_assert(DXGI_FORMAT_R24G8_TYPELESS == 44 && DXGI_FORMAT_D24_UNORM_S8_UINT == 45 &&
    DXGI_FORMAT_R24_UNORM_X8_TYPELESS == 46);
static_assert(D3D11_DSV_DIMENSION_TEXTURE2D == 3 && D3D11_RTV_DIMENSION_TEXTURE2D == 4 &&
    D3D11_SRV_DIMENSION_TEXTURE2D == 4);

uint64_t identity_of(IUnknown* object) noexcept {
    ComPtr<IUnknown> canonical;
    return object && SUCCEEDED(object->QueryInterface(IID_PPV_ARGS(&canonical)))
        ? reinterpret_cast<uint64_t>(canonical.Get()) : 0;
}
void describe(ID3D11Texture2D* texture, NativeDepthTextureFacts& output) noexcept {
    output.texture = 1;
    D3D11_TEXTURE2D_DESC allocation{};
    texture->GetDesc(&allocation);
    output.width = allocation.Width; output.height = allocation.Height;
    output.format = allocation.Format; output.mip_levels = allocation.MipLevels;
    output.array_size = allocation.ArraySize;
    output.sample_count = allocation.SampleDesc.Count; output.sample_quality = allocation.SampleDesc.Quality;
    ComPtr<ID3D11Device> device;
    texture->GetDevice(&device);
    output.device = identity_of(device.Get());
}
void resource(ID3D11Resource* value, NativeDepthTextureFacts& output) noexcept {
    output.resource = identity_of(value);
    ComPtr<ID3D11Texture2D> texture;
    if (value && SUCCEEDED(value->QueryInterface(IID_PPV_ARGS(&texture)))) describe(texture.Get(), output);
}

uintptr_t tls_slots() noexcept { return static_cast<uintptr_t>(__readgsqword(0x58)); }
uint64_t identity(uint64_t object) noexcept { return identity_of(reinterpret_cast<IUnknown*>(object)); }
void context(uint64_t key, NativeDepthContextFacts* output) noexcept {
    *output = {};
    auto* value = reinterpret_cast<ID3D11DeviceContext*>(key);
    if (!value) return;
    output->immediate = value->GetType() == D3D11_DEVICE_CONTEXT_IMMEDIATE;
    ComPtr<ID3D11Device> device;
    value->GetDevice(&device);
    output->device = identity_of(device.Get());
    UINT count{};
    value->RSGetViewports(&count, nullptr);
    output->viewport_count = count;
    if (count != 1) return;
    D3D11_VIEWPORT actual{};
    value->RSGetViewports(&count, &actual);
    output->viewport_copied = count;
    const float values[]{actual.TopLeftX, actual.TopLeftY, actual.Width, actual.Height, actual.MinDepth, actual.MaxDepth};
    std::memcpy(output->viewport, values, sizeof(values));
}
void targets(uint64_t key, NativeDepthTargetFacts* output) noexcept {
    *output = {};
    auto* value = reinterpret_cast<ID3D11DeviceContext*>(key);
    if (!value) return;
    ComPtr<ID3D11RenderTargetView> rtv;
    ComPtr<ID3D11DepthStencilView> dsv;
    value->OMGetRenderTargets(1, &rtv, &dsv);
    output->rtv = reinterpret_cast<uint64_t>(rtv.Get());
    output->dsv = reinterpret_cast<uint64_t>(dsv.Get());
    if (dsv) {
        ComPtr<ID3D11Resource> depth;
        dsv->GetResource(&depth);
        resource(depth.Get(), output->depth);
        D3D11_DEPTH_STENCIL_VIEW_DESC depth_view{};
        dsv->GetDesc(&depth_view);
        output->dsv_format = depth_view.Format; output->dsv_dimension = depth_view.ViewDimension;
        output->dsv_mip = depth_view.Texture2D.MipSlice;
    }
    if (rtv) {
        output->rtv_identity = identity_of(rtv.Get());
        ComPtr<ID3D11Resource> colour;
        rtv->GetResource(&colour);
        resource(colour.Get(), output->colour);
        D3D11_RENDER_TARGET_VIEW_DESC colour_view{};
        rtv->GetDesc(&colour_view);
        output->rtv_dimension = colour_view.ViewDimension; output->rtv_mip = colour_view.Texture2D.MipSlice;
    }
}
void scissor(uint64_t key, NativeDepthScissorFacts* output) noexcept {
    *output = {};
    auto* value = reinterpret_cast<ID3D11DeviceContext*>(key);
    if (!value) return;
    ComPtr<ID3D11RasterizerState> rasterizer;
    value->RSGetState(&rasterizer);
    D3D11_RASTERIZER_DESC state{};
    if (rasterizer) rasterizer->GetDesc(&state);
    output->enabled = state.ScissorEnable != FALSE;
    if (!output->enabled) return;
    UINT count{};
    value->RSGetScissorRects(&count, nullptr);
    output->count = count;
    if (count != 1) return;
    D3D11_RECT rectangle{};
    value->RSGetScissorRects(&count, &rectangle);
    output->copied = count;
    output->rect[0] = rectangle.left; output->rect[1] = rectangle.top;
    output->rect[2] = rectangle.right; output->rect[3] = rectangle.bottom;
}
void texture(uint64_t key, NativeDepthTextureFacts* output) noexcept {
    *output = {};
    auto* value = reinterpret_cast<ID3D11Texture2D*>(key);
    if (!value) return;
    output->resource = identity_of(value);
    describe(value, *output);
}
void view(uint64_t key, NativeDepthViewFacts* output) noexcept {
    *output = {};
    auto* value = reinterpret_cast<ID3D11ShaderResourceView*>(key);
    if (!value) return;
    D3D11_SHADER_RESOURCE_VIEW_DESC description{};
    value->GetDesc(&description);
    output->format = description.Format; output->dimension = description.ViewDimension;
    output->most_detailed_mip = description.Texture2D.MostDetailedMip;
    output->mip_levels = description.Texture2D.MipLevels;
    ComPtr<ID3D11Resource> viewed;
    value->GetResource(&viewed);
    output->resource = identity_of(viewed.Get());
    ComPtr<ID3D11Device> device;
    value->GetDevice(&device);
    output->device = identity_of(device.Get());
}
// The text the former refuse() wrote; Rust decides the stage and its once-per-stage receipt.
void refusal(NativeDepthReport report, const char* stage, const NativeDepthSnapshot* snapshot,
    const NativeDepthMathFailure* detail, uint32_t math) noexcept {
    if (!report || !stage || !snapshot) return;
    const auto& s = *snapshot;
    try {
        char text[800]{};
        std::snprintf(text,sizeof(text),
            "Fallout4 depth stage=%s source=%llu submission=%llu native=%llu mode=%u range=%g,%g/%g,%g raster=%g,%g,%g,%g shadow=%g,%g,%g,%g dirty=%u selectors=%u,%u,%u descriptor=%ux%u descriptor_read=%u physical=%u/%u,%u ratio=%g,%g default=%u dynamic=%u",
            stage,static_cast<unsigned long long>(s.identity.source),
            static_cast<unsigned long long>(s.identity.submission),static_cast<unsigned long long>(s.native_frame),
            unsigned(s.mode),double(s.intervals[0]),double(s.intervals[1]),double(s.intervals[2]),double(s.intervals[3]),
            double(s.actual_raster[0]),double(s.actual_raster[1]),double(s.actual_raster[2]),double(s.actual_raster[3]),
            double(s.shadow_raster[0]),double(s.shadow_raster[1]),double(s.shadow_raster[2]),double(s.shadow_raster[3]),
            s.shadow_dirty,s.colour_selector,s.depth_selector,s.cube_selector,s.descriptor_width,s.descriptor_height,
            unsigned(s.descriptor_read),
            s.colour_slot,s.bound_colour_slot,s.bound_depth_slot,
            double(s.ratio[0]),double(s.ratio[1]),unsigned(s.default_viewport),unsigned(s.dynamic_viewport));
        report(text);
        if (detail && math) {
            char math_text[520]{};
            if (math == 1) {
                std::snprintf(math_text,sizeof(math_text),
                    "Fallout4 depth math stage=%s source=%llu native=%llu mismatch_index=%u actual=%.9g expected=%.9g relative_error=%.9g tolerance=0.0005",
                    stage,static_cast<unsigned long long>(s.identity.source),static_cast<unsigned long long>(s.native_frame),
                    detail->index,double(detail->actual),double(detail->expected),double(detail->relative_error));
            } else {
                std::snprintf(math_text,sizeof(math_text),
                    "Fallout4 depth math stage=%s source=%llu native=%llu W=%.9g,%.9g,%.9g,%.9g minimum=%.12g range=%.9g near_origin=%.9g,%.9g,%.9g world_origin=%.9g,%.9g,%.9g",
                    stage,static_cast<unsigned long long>(s.identity.source),static_cast<unsigned long long>(s.native_frame),
                    detail->w[0],detail->w[1],detail->w[2],detail->w[3],detail->minimum_w,detail->range,
                    double(s.near_camera.origin[0]),double(s.near_camera.origin[1]),double(s.near_camera.origin[2]),
                    double(s.world_camera.origin[0]),double(s.world_camera.origin[1]),double(s.world_camera.origin[2]));
            }
            report(math_text);
        }
    } catch (...) {}
}
// The text the former lease check wrote; Rust keeps its budget of sixteen.
void lease(NativeDepthReport report, const NativeDepthSnapshot* snapshot, const NativeDepthSnapshot* current,
    uint32_t reached, uint32_t passed, uint32_t stage, uint32_t changed) noexcept {
    if (!report || !snapshot || !current) return;
    try {
        char text[600]{};
        std::snprintf(text,sizeof(text),
            "Fallout4 depth lease source=%llu native=%llu/%llu reached=0x%x passed=0x%x sample_stage=%u changed=0x%x mode=%u/%u default=%u/%u dynamic=%u/%u jitter=%g,%g/%g,%g ratio=%g,%g/%g,%g",
            static_cast<unsigned long long>(snapshot->identity.source),static_cast<unsigned long long>(snapshot->native_frame),
            static_cast<unsigned long long>(current->native_frame),reached,passed,stage,changed,
            unsigned(snapshot->mode),unsigned(current->mode),unsigned(snapshot->default_viewport),unsigned(current->default_viewport),
            unsigned(snapshot->dynamic_viewport),unsigned(current->dynamic_viewport),double(snapshot->jitter[0]),double(snapshot->jitter[1]),
            double(current->jitter[0]),double(current->jitter[1]),double(snapshot->ratio[0]),double(snapshot->ratio[1]),
            double(current->ratio[0]),double(current->ratio[1]));
        report(text);
    } catch (...) {}
}

constexpr NativeDepthLeaves leaves{tls_slots, identity, context, targets, scissor, texture, view, refusal, lease};
}
const NativeDepthLeaves* native_depth_leaves() noexcept { return &leaves; }
}
