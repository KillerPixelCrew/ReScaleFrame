// SPDX-License-Identifier: MIT
#include "fg_inputs.h"
#include "sr_pass.h"
#include "../../../runtime/graphics/src/device_owner.h"
#include <rescaleframe/d3d11_state.h>
#include <rescaleframe/shader_compile.h>
#include <wrl/client.h>
#include <cstring>
#include <memory>
#include <utility>

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
struct Pipeline {
    ComPtr<ID3D11ComputeShader> shader;
    ComPtr<ID3D11Buffer> constants;
};

struct Targets {
    ComPtr<ID3D11Texture2D> depth_copy, depth, motion;
    ComPtr<ID3D11ShaderResourceView> depth_view;
    ComPtr<ID3D11UnorderedAccessView> depth_uav, motion_uav;
};

static_assert(sizeof(rsf_anomaly_input_constants) == 96);
static_assert(DXGI_FORMAT_D24_UNORM_S8_UINT == 45 && DXGI_FORMAT_R24G8_TYPELESS == 44 &&
    DXGI_FORMAT_R16G16B16A16_FLOAT == 10);
static_assert(static_cast<uint32_t>(InputPass::Result::Ok) == 0 &&
    static_cast<uint32_t>(InputPass::Result::Shader) == 1 &&
    static_cast<uint32_t>(InputPass::Result::Format) == 2 &&
    static_cast<uint32_t>(InputPass::Result::Size) == 3 &&
    static_cast<uint32_t>(InputPass::Result::Resources) == 4);

void* get_device(void* raw_context) noexcept {
    auto* context = static_cast<ID3D11DeviceContext*>(raw_context);
    if (context->GetType() != D3D11_DEVICE_CONTEXT_IMMEDIATE) return nullptr;
    ID3D11Device* device{};
    context->GetDevice(&device);
    return device;
}

void describe(void* texture, rsf_anomaly_input_desc* out) noexcept {
    D3D11_TEXTURE2D_DESC desc{};
    static_cast<ID3D11Texture2D*>(texture)->GetDesc(&desc);
    *out = {static_cast<uint32_t>(desc.Format), desc.Width, desc.Height, desc.SampleDesc.Count,
        desc.ArraySize, desc.MipLevels};
}

void* build(void* raw_device, rsf_anomaly_input_log log, void* user) noexcept try {
    auto* device = static_cast<ID3D11Device*>(raw_device);
    auto pipeline = std::make_unique<Pipeline>();
    if (!rsf::compile_compute(device, source, "rsf_fg_inputs.hlsl", "rsf_fg_inputs_cs",
        &pipeline->shader, log, user)) return nullptr;
    D3D11_BUFFER_DESC desc{sizeof(rsf_anomaly_input_constants), D3D11_USAGE_DYNAMIC,
        D3D11_BIND_CONSTANT_BUFFER, D3D11_CPU_ACCESS_WRITE, 0, 0};
    if (FAILED(device->CreateBuffer(&desc, nullptr, &pipeline->constants))) return nullptr;
    return pipeline.release();
} catch (...) { return nullptr; }

void make_targets(void* raw_device, uint32_t width, uint32_t height, rsf_anomaly_input_targets* out) noexcept try {
    auto* device = static_cast<ID3D11Device*>(raw_device);
    auto targets = std::make_unique<Targets>();
    const auto make = [&](DXGI_FORMAT format, UINT bind, ComPtr<ID3D11Texture2D>& texture) {
        D3D11_TEXTURE2D_DESC desc{width, height, 1, 1, format, {1, 0}, D3D11_USAGE_DEFAULT, bind, 0, 0};
        return SUCCEEDED(device->CreateTexture2D(&desc, nullptr, &texture));
    };
    constexpr UINT output_bind = D3D11_BIND_SHADER_RESOURCE | D3D11_BIND_UNORDERED_ACCESS;
    if (!make(DXGI_FORMAT_R24G8_TYPELESS, D3D11_BIND_SHADER_RESOURCE, targets->depth_copy) ||
        !make(DXGI_FORMAT_R32_FLOAT, output_bind, targets->depth) ||
        !make(DXGI_FORMAT_R16G16_FLOAT, output_bind, targets->motion)) return;
    D3D11_SHADER_RESOURCE_VIEW_DESC view{};
    view.Format = DXGI_FORMAT_R24_UNORM_X8_TYPELESS;
    view.ViewDimension = D3D11_SRV_DIMENSION_TEXTURE2D;
    view.Texture2D.MipLevels = 1;
    if (FAILED(device->CreateShaderResourceView(targets->depth_copy.Get(), &view, &targets->depth_view)) ||
        FAILED(device->CreateUnorderedAccessView(targets->depth.Get(), nullptr, &targets->depth_uav)) ||
        FAILED(device->CreateUnorderedAccessView(targets->motion.Get(), nullptr, &targets->motion_uav))) return;
    const auto depth = targets->depth.Get();
    const auto motion = targets->motion.Get();
    *out = {targets.release(), depth, motion};
} catch (...) {}

uint32_t dispatch(void* raw_context, void* raw_pipeline, void* raw_targets, void* depth,
    void* motion_view, const rsf_anomaly_input_constants* values, uint32_t groups_x, uint32_t groups_y) noexcept {
    auto* context = static_cast<ID3D11DeviceContext*>(raw_context);
    const auto& pipeline = *static_cast<const Pipeline*>(raw_pipeline);
    const auto& targets = *static_cast<const Targets*>(raw_targets);
    rsf_d3d11_state state{};
    if (!rsf_d3d11_state_save(context, &state)) return 0;
    D3D11_MAPPED_SUBRESOURCE mapped{};
    if (FAILED(context->Map(pipeline.constants.Get(), 0, D3D11_MAP_WRITE_DISCARD, 0, &mapped))) {
        rsf_d3d11_state_restore(context, &state);
        return 0;
    }
    std::memcpy(mapped.pData, values, sizeof(*values));
    context->Unmap(pipeline.constants.Get(), 0);
    context->SetPredication(nullptr, FALSE);
    context->CopyResource(targets.depth_copy.Get(), static_cast<ID3D11Texture2D*>(depth));
    ID3D11ShaderResourceView* views[]{targets.depth_view.Get(), static_cast<ID3D11ShaderResourceView*>(motion_view)};
    ID3D11UnorderedAccessView* uavs[]{targets.depth_uav.Get(), targets.motion_uav.Get()};
    ID3D11Buffer* buffers[]{pipeline.constants.Get()};
    context->CSSetShader(pipeline.shader.Get(), nullptr, 0);
    context->CSSetShaderResources(0, 2, views);
    context->CSSetUnorderedAccessViews(0, 2, uavs, nullptr);
    context->CSSetConstantBuffers(0, 1, buffers);
    context->Dispatch(groups_x, groups_y, 1);
    ID3D11UnorderedAccessView* no_uavs[2]{};
    ID3D11ShaderResourceView* no_views[2]{};
    context->CSSetUnorderedAccessViews(0, 2, no_uavs, nullptr);
    context->CSSetShaderResources(0, 2, no_views);
    rsf_d3d11_state_restore(context, &state);
    return 1;
}

constexpr rsf_anomaly_input_platform platform{
    get_device,
    [](void* object) { static_cast<IUnknown*>(object)->Release(); },
    describe,
    [](void* texture, void* device) { return uint32_t(rsf::same_device(
        static_cast<ID3D11Texture2D*>(texture), static_cast<ID3D11Device*>(device))); },
    build,
    [](void* pipeline) { delete static_cast<Pipeline*>(pipeline); },
    make_targets,
    [](void* targets) { delete static_cast<Targets*>(targets); },
    rsf_d3d11_create_shader_view,
    dispatch,
    sr::first_person_depth_limit,
    sr::motion_validity_threshold,
};
}

InputPass::Result InputPass::run(ID3D11DeviceContext* context, const InputFrame& frame,
    void (*log)(void*, const char*), void* user) noexcept {
    rsf_anomaly_input_frame input{frame.depth, frame.motion, {}, frame.width, frame.height};
    std::memcpy(input.clip_to_previous_clip, frame.clip_to_previous_clip, sizeof(input.clip_to_previous_clip));
    return static_cast<Result>(rsf_anomaly_input_run(&owner_, &platform, context, &input, log, user));
}

ID3D11Texture2D* InputPass::depth() const noexcept {
    return static_cast<ID3D11Texture2D*>(rsf_anomaly_input_output(owner_, 0));
}

ID3D11Texture2D* InputPass::motion() const noexcept {
    return static_cast<ID3D11Texture2D*>(rsf_anomaly_input_output(owner_, 1));
}

void InputPass::release() noexcept {
    rsf_anomaly_input_release(std::exchange(owner_, nullptr));
}
}
