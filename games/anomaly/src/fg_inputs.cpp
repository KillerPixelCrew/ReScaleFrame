// SPDX-License-Identifier: GPL-3.0-only
#include "fg_inputs.h"
#include "sr_pass.h"
#include <rescaleframe/d3d11_state.h>
#include <rescaleframe/shader_compile.h>
#include <cstring>
#include <memory>

namespace rsf::anomaly::fg {
namespace {
using Microsoft::WRL::ComPtr;
// RSF-authored. Depth passes through; motion follows the decode in sr_pass.h (validity channel 2, threshold 0.5) and
// the first-person rule; everything else is camera motion from depth.
constexpr char source[] = R"hlsl(
Texture2D<float> depth_in : register(t0);
Texture2D<float4> motion_in : register(t1);
RWTexture2D<float> depth_out : register(u0);
RWTexture2D<float2> motion_out : register(u1);
cbuffer Constants : register(b0) {
    row_major float4x4 clip_to_previous;
    float2 size;
    float has_motion;
    float first_person_limit;
    float validity_threshold;
    float3 padding;
};
[numthreads(8, 8, 1)]
void rsf_fg_inputs_cs(uint3 id : SV_DispatchThreadID) {
    if (id.x >= (uint)size.x || id.y >= (uint)size.y) return;
    const float depth = depth_in[id.xy];
    depth_out[id.xy] = depth;
    const float2 uv = (float2(id.xy) + 0.5) / size;
    const float4 stored = has_motion > 0.5 ? motion_in[id.xy] : float4(0, 0, 0, 0);
    float2 motion;
    if (stored.z > validity_threshold) {
        motion = -stored.xy * size;                  // current-minus-previous UV -> previous-minus-current pixels
    } else if (depth < first_person_limit) {
        motion = float2(0, 0);                       // first-person geometry has its own projection
    } else {
        const float4 previous = mul(float4(uv.x * 2 - 1, 1 - uv.y * 2, depth, 1), clip_to_previous);
        const float2 ndc = previous.xy / previous.w;
        motion = (float2(ndc.x * 0.5 + 0.5, 0.5 - ndc.y * 0.5) - uv) * size;
    }
    motion_out[id.xy] = motion;
}
)hlsl";
struct alignas(16) Constants {
    float clip_to_previous[16];
    float size[2];
    float has_motion, first_person_limit, validity_threshold, padding[3];
};
static_assert(sizeof(Constants) % 16 == 0);
bool depth_format(DXGI_FORMAT format) { return format == DXGI_FORMAT_D24_UNORM_S8_UINT || format == DXGI_FORMAT_R24G8_TYPELESS; }
}

bool InputPass::build(ID3D11Device* device, void (*log)(void*, const char*), void* user) noexcept {
    shader_.Reset(); constants_.Reset();
    if (!rsf::compile_compute(device, source, "rsf_fg_inputs.hlsl", "rsf_fg_inputs_cs", &shader_, log, user)) return false;
    D3D11_BUFFER_DESC desc{sizeof(Constants), D3D11_USAGE_DYNAMIC, D3D11_BIND_CONSTANT_BUFFER, D3D11_CPU_ACCESS_WRITE, 0, 0};
    return SUCCEEDED(device->CreateBuffer(&desc, nullptr, &constants_));
}

bool InputPass::size(ID3D11Device* device, uint32_t width, uint32_t height) noexcept {
    if (width == width_ && height == height_ && depth_out_) return true;
    depth_copy_.Reset(); depth_out_.Reset(); motion_out_.Reset(); depth_view_.Reset(); depth_uav_.Reset(); motion_uav_.Reset();
    width_ = height_ = 0;
    const auto make = [&](DXGI_FORMAT format, UINT bind, ComPtr<ID3D11Texture2D>& out) {
        D3D11_TEXTURE2D_DESC desc{width, height, 1, 1, format, {1, 0}, D3D11_USAGE_DEFAULT, bind, 0, 0};
        return SUCCEEDED(device->CreateTexture2D(&desc, nullptr, &out));
    };
    if (!make(DXGI_FORMAT_R24G8_TYPELESS, D3D11_BIND_SHADER_RESOURCE, depth_copy_) ||
        !make(DXGI_FORMAT_R32_FLOAT, D3D11_BIND_SHADER_RESOURCE | D3D11_BIND_UNORDERED_ACCESS, depth_out_) ||
        !make(DXGI_FORMAT_R16G16_FLOAT, D3D11_BIND_SHADER_RESOURCE | D3D11_BIND_UNORDERED_ACCESS, motion_out_)) return false;
    D3D11_SHADER_RESOURCE_VIEW_DESC view{};
    view.Format = DXGI_FORMAT_R24_UNORM_X8_TYPELESS; view.ViewDimension = D3D11_SRV_DIMENSION_TEXTURE2D; view.Texture2D.MipLevels = 1;
    if (FAILED(device->CreateShaderResourceView(depth_copy_.Get(), &view, &depth_view_)) ||
        FAILED(device->CreateUnorderedAccessView(depth_out_.Get(), nullptr, &depth_uav_)) ||
        FAILED(device->CreateUnorderedAccessView(motion_out_.Get(), nullptr, &motion_uav_))) return false;
    width_ = width; height_ = height;
    return true;
}

InputPass::Result InputPass::run(ID3D11DeviceContext* context, const InputFrame& frame, void (*log)(void*, const char*),
    void* user) noexcept {
    if (!context || !frame.depth || !frame.width || !frame.height) return Result::Resources;
    D3D11_TEXTURE2D_DESC depth{};
    frame.depth->GetDesc(&depth);
    if (!depth_format(depth.Format) || depth.SampleDesc.Count != 1 || depth.ArraySize != 1) return Result::Format;
    if (depth.Width != frame.width || depth.Height != frame.height) return Result::Size;
    ComPtr<ID3D11ShaderResourceView> motion_view;
    ComPtr<ID3D11Device> device;
    context->GetDevice(&device);
    if (device_ != device) { release(); device_ = device; failed_ = false; }
    if (failed_) return Result::Shader;
    if (!shader_ && !build(device.Get(), log, user)) { failed_ = true; return Result::Shader; }
    if (!size(device.Get(), frame.width, frame.height)) return Result::Resources;
    if (frame.motion) {
        D3D11_TEXTURE2D_DESC motion{};
        frame.motion->GetDesc(&motion);
        if (motion.Format != DXGI_FORMAT_R16G16B16A16_FLOAT || motion.Width != frame.width || motion.Height != frame.height ||
            FAILED(device->CreateShaderResourceView(frame.motion, nullptr, &motion_view))) motion_view.Reset();
    }
    Constants constants{};
    std::memcpy(constants.clip_to_previous, frame.clip_to_previous_clip, sizeof(constants.clip_to_previous));
    constants.size[0] = float(frame.width); constants.size[1] = float(frame.height);
    constants.has_motion = motion_view ? 1.0f : 0.0f;
    constants.first_person_limit = sr::first_person_depth_limit;
    constants.validity_threshold = sr::motion_validity_threshold;
    D3D11_MAPPED_SUBRESOURCE mapped{};
    if (FAILED(context->Map(constants_.Get(), 0, D3D11_MAP_WRITE_DISCARD, 0, &mapped))) return Result::Resources;
    std::memcpy(mapped.pData, &constants, sizeof(constants));
    context->Unmap(constants_.Get(), 0);
    context->CopyResource(depth_copy_.Get(), frame.depth);
    auto state = std::make_unique<rsf_d3d11_state>();
    if (!rsf_d3d11_state_save(context, state.get())) return Result::Resources;
    ID3D11ShaderResourceView* views[]{depth_view_.Get(), motion_view.Get()};
    ID3D11UnorderedAccessView* uavs[]{depth_uav_.Get(), motion_uav_.Get()};
    ID3D11Buffer* buffers[]{constants_.Get()};
    context->CSSetShader(shader_.Get(), nullptr, 0);
    context->CSSetShaderResources(0, 2, views);
    context->CSSetUnorderedAccessViews(0, 2, uavs, nullptr);
    context->CSSetConstantBuffers(0, 1, buffers);
    context->Dispatch((frame.width + 7) / 8, (frame.height + 7) / 8, 1);
    ID3D11UnorderedAccessView* no_uavs[2]{};
    ID3D11ShaderResourceView* no_views[2]{};
    context->CSSetUnorderedAccessViews(0, 2, no_uavs, nullptr);
    context->CSSetShaderResources(0, 2, no_views);
    rsf_d3d11_state_restore(context, state.get());
    return Result::Ok;
}

void InputPass::release() noexcept {
    shader_.Reset(); constants_.Reset(); depth_copy_.Reset(); depth_out_.Reset(); motion_out_.Reset();
    depth_view_.Reset(); depth_uav_.Reset(); motion_uav_.Reset(); device_.Reset();
    width_ = height_ = 0;
}
}
