// SPDX-License-Identifier: MIT
#include "motion_d3d11.h"
#include <rescaleframe/shader_compile.h>
#include <cstring>
#include <vector>

namespace rsf::anomaly::motion {
namespace {
void* dynamic_buffer(void* raw_device, UINT size) noexcept
{
    auto* device = static_cast<ID3D11Device*>(raw_device);
    ComPtr<ID3D11Buffer> buffer;
    D3D11_BUFFER_DESC desc{};
    desc.ByteWidth = size;
    desc.Usage = D3D11_USAGE_DYNAMIC;
    desc.BindFlags = D3D11_BIND_CONSTANT_BUFFER;
    desc.CPUAccessFlags = D3D11_CPU_ACCESS_WRITE;
    if (FAILED(device->CreateBuffer(&desc, nullptr, &buffer))) return nullptr;
    return buffer.Detach();
}

void* zero_shader(void* raw_device, LogFn log, void* user) noexcept try {
    auto* device = static_cast<ID3D11Device*>(raw_device);
    std::vector<uint8_t> code;
    ComPtr<ID3D11PixelShader> shader;
    if (!rsf::compile_shader(zero_motion_hlsl, sizeof(zero_motion_hlsl) - 1, "rsf_anomaly_zero_motion", "main", "ps_5_0",
            code, log, user) ||
        FAILED(device->CreatePixelShader(code.data(), code.size(), nullptr, &shader))) return nullptr;
    return shader.Detach();
} catch (...) { return nullptr; }

void* depth_state(void* raw_device, uint32_t equal) noexcept
{
    auto* device = static_cast<ID3D11Device*>(raw_device);
    D3D11_DEPTH_STENCIL_DESC desc{};
    desc.DepthEnable = equal != 0;
    desc.DepthWriteMask = D3D11_DEPTH_WRITE_MASK_ZERO;
    desc.DepthFunc = equal ? D3D11_COMPARISON_EQUAL : D3D11_COMPARISON_ALWAYS;
    ComPtr<ID3D11DepthStencilState> state;
    if (FAILED(device->CreateDepthStencilState(&desc, &state))) return nullptr;
    return state.Detach();
}

void* texture(void* raw_device, uint32_t width, uint32_t height) noexcept
{
    auto* device = static_cast<ID3D11Device*>(raw_device);
    D3D11_TEXTURE2D_DESC desc{};
    desc.Width = width; desc.Height = height; desc.MipLevels = 1; desc.ArraySize = 1;
    desc.Format = DXGI_FORMAT_R16G16B16A16_FLOAT; desc.SampleDesc.Count = 1;
    desc.Usage = D3D11_USAGE_DEFAULT; desc.BindFlags = D3D11_BIND_RENDER_TARGET | D3D11_BIND_SHADER_RESOURCE;
    ComPtr<ID3D11Texture2D> resource;
    if (FAILED(device->CreateTexture2D(&desc, nullptr, &resource))) return nullptr;
    return resource.Detach();
}

void* target_view(void* device, void* texture) noexcept
{
    ComPtr<ID3D11RenderTargetView> view;
    if (FAILED(static_cast<ID3D11Device*>(device)->CreateRenderTargetView(
            static_cast<ID3D11Texture2D*>(texture), nullptr, &view))) return nullptr;
    return view.Detach();
}

uint32_t upload(void* raw_context, void* raw_buffer, const void* data, std::size_t size) noexcept
{
    auto* context = static_cast<ID3D11DeviceContext*>(raw_context);
    auto* buffer = static_cast<ID3D11Buffer*>(raw_buffer);
    D3D11_MAPPED_SUBRESOURCE mapped{};
    if (FAILED(context->Map(buffer, 0, D3D11_MAP_WRITE_DISCARD, 0, &mapped))) return 0;
    if (mapped.pData) std::memcpy(mapped.pData, data, size);
    context->Unmap(buffer, 0);
    return mapped.pData != nullptr;
}

uint32_t bind(void* raw_context, void* raw_buffer, char stage) noexcept
{
    auto* context = static_cast<ID3D11DeviceContext*>(raw_context);
    auto* buffer = static_cast<ID3D11Buffer*>(raw_buffer);
    switch (stage) {
    case 'v': context->VSSetConstantBuffers(13, 1, &buffer); return 1;
    case 'p': context->PSSetConstantBuffers(13, 1, &buffer); return 1;
    case 'd': context->DSSetConstantBuffers(13, 1, &buffer); return 1;
    default: return 0;
    }
}

constexpr rsf_anomaly_motion_device_platform platform{
    [](void* resource) { static_cast<IUnknown*>(resource)->AddRef(); },
    [](void* resource) { static_cast<IUnknown*>(resource)->Release(); },
    dynamic_buffer, zero_shader, depth_state, texture, target_view, upload, bind
};

static_assert(D3D11_SIMULTANEOUS_RENDER_TARGET_COUNT == 8 && D3D11_SHADER_MAX_INTERFACES == 253);
static_assert(sizeof(rsf_anomaly_motion_replay_depth) == 88 && sizeof(rsf_anomaly_motion_replay_pixel) == 2072 &&
    offsetof(rsf_anomaly_motion_replay_pixel, classes) == 40 && sizeof(rsf_anomaly_motion_replay_platform) == 56);
constexpr rsf_anomaly_motion_replay_platform replay_platform{
    platform.release,
    [](void* raw, rsf_anomaly_motion_replay_depth* out) noexcept {
        auto* context = static_cast<ID3D11DeviceContext*>(raw);
        context->OMGetRenderTargets(D3D11_SIMULTANEOUS_RENDER_TARGET_COUNT,
            reinterpret_cast<ID3D11RenderTargetView**>(out->targets),
            reinterpret_cast<ID3D11DepthStencilView**>(&out->view));
        ID3D11DepthStencilState* state{};
        context->OMGetDepthStencilState(&state, &out->reference);
        out->state = state;
        D3D11_DEPTH_STENCIL_DESC desc{};
        if (state) state->GetDesc(&desc);
        else { desc.DepthEnable = TRUE; desc.DepthWriteMask = D3D11_DEPTH_WRITE_MASK_ALL; }
        out->enabled = desc.DepthEnable != FALSE;
        out->writable = desc.DepthWriteMask != D3D11_DEPTH_WRITE_MASK_ZERO;
    },
    [](void* raw, rsf_anomaly_motion_replay_pixel* out) noexcept {
        auto* context = static_cast<ID3D11DeviceContext*>(raw);
        context->OMGetBlendState(reinterpret_cast<ID3D11BlendState**>(&out->blend), out->factor, &out->mask);
        out->count = D3D11_SHADER_MAX_INTERFACES;
        context->PSGetShader(reinterpret_cast<ID3D11PixelShader**>(&out->shader),
            reinterpret_cast<ID3D11ClassInstance**>(out->classes), &out->count);
    },
    [](void* raw, uint32_t count, void* const* targets, void* depth) noexcept {
        static_cast<ID3D11DeviceContext*>(raw)->OMSetRenderTargets(count,
            reinterpret_cast<ID3D11RenderTargetView* const*>(targets), static_cast<ID3D11DepthStencilView*>(depth));
    },
    [](void* raw, void* depth, uint32_t reference) noexcept {
        static_cast<ID3D11DeviceContext*>(raw)->OMSetDepthStencilState(static_cast<ID3D11DepthStencilState*>(depth), reference);
    },
    [](void* raw, void* blend, const float* factor, uint32_t mask) noexcept {
        static_cast<ID3D11DeviceContext*>(raw)->OMSetBlendState(static_cast<ID3D11BlendState*>(blend), factor, mask);
    },
    [](void* raw, void* shader, void* const* classes, uint32_t count) noexcept {
        static_cast<ID3D11DeviceContext*>(raw)->PSSetShader(static_cast<ID3D11PixelShader*>(shader),
            reinterpret_cast<ID3D11ClassInstance* const*>(classes), count);
    },
};
}

const rsf_anomaly_motion_device_platform& motion_device_platform() noexcept { return platform; }
const rsf_anomaly_motion_replay_platform& motion_replay_platform() noexcept { return replay_platform; }

MotionDevice::~MotionDevice() { rsf_anomaly_motion_device_drop(owner_); }

bool MotionDevice::create(ID3D11Device* device, LogFn log, void* user) noexcept
{
    if (!owner_) owner_ = rsf_anomaly_motion_device_new(&platform);
    return rsf_anomaly_motion_device_create(owner_, device, log, user) != 0;
}

void MotionDevice::reset() noexcept { rsf_anomaly_motion_device_reset(owner_); }

TargetResult MotionDevice::ensure_target(const D3D11_TEXTURE2D_DESC& rt0) noexcept
{
    return static_cast<TargetResult>(rsf_anomaly_motion_device_target(owner_, rt0.Width, rt0.Height,
        rt0.SampleDesc.Count, rt0.ArraySize));
}

void MotionDevice::clear(ID3D11DeviceContext* context) noexcept
{
    static constexpr float zero[4]{};
    if (auto* target = rtv(); context && target) context->ClearRenderTargetView(target, zero);
}

bool MotionDevice::set_core_vertex(ID3D11DeviceContext* context, const CoreVertexBytes& bytes) noexcept
{
    return rsf_anomaly_motion_device_upload(owner_, context, RSF_MOTION_CORE, bytes.data(), bytes.size()) != 0;
}

bool MotionDevice::set_foliage_vertex(ID3D11DeviceContext* context, const FoliageVertexBytes& bytes) noexcept
{
    return rsf_anomaly_motion_device_upload(owner_, context, RSF_MOTION_FOLIAGE, bytes.data(), bytes.size()) != 0;
}

bool MotionDevice::set_pixel(ID3D11DeviceContext* context, const PixelBytes& bytes) noexcept
{
    return rsf_anomaly_motion_device_upload(owner_, context, RSF_MOTION_PIXEL, bytes.data(), bytes.size()) != 0;
}

bool MotionDevice::set_tess_domain(ID3D11DeviceContext* context, const TessDomainBytes& bytes) noexcept
{
    return rsf_anomaly_motion_device_upload(owner_, context, RSF_MOTION_TESS, bytes.data(), bytes.size()) != 0;
}

}
