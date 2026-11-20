// SPDX-License-Identifier: MIT
#include "motion_d3d11.h"
#include <rescaleframe/shader_compile.h>
#include <cstring>
#include <vector>

namespace rsf::anomaly::motion {
namespace {
rsf_shader_patch_api runtime_api{};

void* dynamic_buffer(void* raw_device, UINT size) noexcept
{
    auto* device = static_cast<ID3D11Device*>(raw_device);
    ComPtr<ID3D11Buffer> buffer;
    if (runtime_api.constant_buffer) {
        if (runtime_api.constant_buffer(device, size, reinterpret_cast<void**>(buffer.GetAddressOf())) != 0) return nullptr;
    } else {
        D3D11_BUFFER_DESC desc{};
        desc.ByteWidth = size;
        desc.Usage = D3D11_USAGE_DYNAMIC;
        desc.BindFlags = D3D11_BIND_CONSTANT_BUFFER;
        desc.CPUAccessFlags = D3D11_CPU_ACCESS_WRITE;
        if (FAILED(device->CreateBuffer(&desc, nullptr, &buffer))) return nullptr;
    }
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
    if (runtime_api.upload_constant) return runtime_api.upload_constant(context, buffer, {data, size}) == 0;
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
    if (runtime_api.bind_constant) return runtime_api.bind_constant(context, stage, 13, buffer) == 0;
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
}

const rsf_anomaly_motion_device_platform& motion_device_platform() noexcept { return platform; }

void set_shader_runtime(const rsf_shader_patch_api& api) noexcept { runtime_api = api; }

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

ReplayResult MotionDevice::begin_replay(const rsf_anomaly_motion_device_view& resources,
    ID3D11DeviceContext* context, Saved& s) noexcept try {
    if (!context || !resources.rtv || !resources.zero_shader) return ReplayResult::Failed;
    context->OMGetRenderTargets(D3D11_SIMULTANEOUS_RENDER_TARGET_COUNT, s.targets, &s.depth);
    context->OMGetDepthStencilState(&s.depth_state, &s.reference);
    auto* state = static_cast<ID3D11DepthStencilState*>(resources.no_depth);
    D3D11_DEPTH_STENCIL_DESC desc{};
    if (s.depth_state) s.depth_state->GetDesc(&desc);
    else { desc.DepthEnable = TRUE; desc.DepthWriteMask = D3D11_DEPTH_WRITE_MASK_ALL; desc.DepthFunc = D3D11_COMPARISON_LESS; }
    if (s.depth && desc.DepthEnable) {
        if (desc.DepthWriteMask == D3D11_DEPTH_WRITE_MASK_ZERO) return ReplayResult::SkippedNoDepthWrite;
        state = static_cast<ID3D11DepthStencilState*>(resources.equal_depth);
    }
    context->OMGetBlendState(&s.blend, s.factor, &s.mask);
    s.class_count = D3D11_SHADER_MAX_INTERFACES;
    context->PSGetShader(&s.shader, s.classes, &s.class_count);
    s.modified = true;
    ID3D11RenderTargetView* targets[3]{nullptr, nullptr, static_cast<ID3D11RenderTargetView*>(resources.rtv)};
    context->OMSetRenderTargets(3, targets, s.depth);
    context->OMSetDepthStencilState(state, 0);
    context->OMSetBlendState(nullptr, nullptr, 0xffffffffu);
    context->PSSetShader(static_cast<ID3D11PixelShader*>(resources.zero_shader), nullptr, 0);
    return ReplayResult::Done;
} catch (...) { return ReplayResult::Failed; }

void MotionDevice::end_replay(ID3D11DeviceContext* context, Saved& s) noexcept
{
    if (!context) return;
    if (s.modified) {
        context->OMSetRenderTargets(D3D11_SIMULTANEOUS_RENDER_TARGET_COUNT, s.targets, s.depth);
        context->OMSetDepthStencilState(s.depth_state, s.reference);
        context->OMSetBlendState(s.blend, s.factor, s.mask);
        context->PSSetShader(s.shader, s.classes, s.class_count);
    }
    for (auto* target : s.targets) if (target) target->Release();
    for (UINT i = 0; i < s.class_count; ++i) if (s.classes[i]) s.classes[i]->Release();
    if (s.depth) s.depth->Release();
    if (s.depth_state) s.depth_state->Release();
    if (s.blend) s.blend->Release();
    if (s.shader) s.shader->Release();
}
}
