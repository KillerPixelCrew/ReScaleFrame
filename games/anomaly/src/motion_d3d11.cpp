// SPDX-License-Identifier: GPL-3.0-only
#include "motion_d3d11.h"
#include <rescaleframe/shader_compile.h>
#include <cstring>
#include <vector>

namespace rsf::anomaly::motion {
namespace {
ComPtr<ID3D11Buffer> dynamic_buffer(ID3D11Device* device, UINT size) noexcept
{
    D3D11_BUFFER_DESC desc{};
    desc.ByteWidth = size;
    desc.Usage = D3D11_USAGE_DYNAMIC;
    desc.BindFlags = D3D11_BIND_CONSTANT_BUFFER;
    desc.CPUAccessFlags = D3D11_CPU_ACCESS_WRITE;
    ComPtr<ID3D11Buffer> buffer;
    if (FAILED(device->CreateBuffer(&desc, nullptr, &buffer))) return {};
    return buffer;
}
}

bool MotionDevice::create(ID3D11Device* device, LogFn log, void* user) noexcept try {
    reset();
    if (!device) return false;
    device_ = device;
    core_vs_ = dynamic_buffer(device, static_cast<UINT>(core_vs_bytes));
    foliage_vs_ = dynamic_buffer(device, static_cast<UINT>(foliage_vs_bytes));
    pixel_ = dynamic_buffer(device, static_cast<UINT>(ps_bytes));
    std::vector<uint8_t> code;
    if (!rsf::compile_shader(zero_motion_hlsl, sizeof(zero_motion_hlsl) - 1, "rsf_anomaly_zero_motion", "main", "ps_5_0",
            code, log, user) ||
        FAILED(device->CreatePixelShader(code.data(), code.size(), nullptr, &zero_ps_))) {
        reset();
        return false;
    }
    D3D11_DEPTH_STENCIL_DESC desc{};
    desc.DepthEnable = FALSE;
    desc.DepthWriteMask = D3D11_DEPTH_WRITE_MASK_ZERO;
    desc.DepthFunc = D3D11_COMPARISON_ALWAYS;
    D3D11_DEPTH_STENCIL_DESC equal{};
    equal.DepthEnable = TRUE;
    equal.DepthWriteMask = D3D11_DEPTH_WRITE_MASK_ZERO;
    equal.DepthFunc = D3D11_COMPARISON_EQUAL;
    equal.StencilEnable = FALSE;
    if (FAILED(device->CreateDepthStencilState(&desc, &no_depth_)) || FAILED(device->CreateDepthStencilState(&equal, &equal_)) ||
        !core_vs_ || !foliage_vs_ || !pixel_) {
        reset();
        return false;
    }
    return true;
} catch (...) { reset(); return false; }

void MotionDevice::reset() noexcept
{
    texture_.Reset(); rtv_.Reset(); width_ = height_ = 0;
    core_vs_.Reset(); foliage_vs_.Reset(); pixel_.Reset();
    have_core_ = have_foliage_ = have_pixel_ = false;
    zero_ps_.Reset(); no_depth_.Reset(); equal_.Reset();
    device_.Reset();
}

TargetResult MotionDevice::ensure_target(const D3D11_TEXTURE2D_DESC& rt0) noexcept
{
    if (!device_) return TargetResult::Failed;
    if (rt0.SampleDesc.Count != 1 || rt0.ArraySize != 1) return TargetResult::Multisampled;
    if (texture_ && width_ == rt0.Width && height_ == rt0.Height) return TargetResult::Ready;
    texture_.Reset(); rtv_.Reset(); width_ = height_ = 0;
    D3D11_TEXTURE2D_DESC desc{};
    desc.Width = rt0.Width; desc.Height = rt0.Height; desc.MipLevels = 1; desc.ArraySize = 1;
    desc.Format = DXGI_FORMAT_R16G16B16A16_FLOAT; desc.SampleDesc.Count = 1;
    desc.Usage = D3D11_USAGE_DEFAULT; desc.BindFlags = D3D11_BIND_RENDER_TARGET | D3D11_BIND_SHADER_RESOURCE;
    if (FAILED(device_->CreateTexture2D(&desc, nullptr, &texture_)) ||
        FAILED(device_->CreateRenderTargetView(texture_.Get(), nullptr, &rtv_))) {
        texture_.Reset(); rtv_.Reset();
        return TargetResult::Failed;
    }
    width_ = rt0.Width; height_ = rt0.Height;
    return TargetResult::Created;
}

void MotionDevice::clear(ID3D11DeviceContext* context) noexcept
{
    static constexpr float zero[4]{};
    if (context && rtv_) context->ClearRenderTargetView(rtv_.Get(), zero);
}

bool MotionDevice::upload(ID3D11DeviceContext* context, ID3D11Buffer* buffer, const void* data, std::size_t size,
    std::byte* last, bool& have) noexcept
{
    if (!context || !buffer) return false;
    if (have && std::memcmp(last, data, size) == 0) { ++upload_skips_; return true; }
    D3D11_MAPPED_SUBRESOURCE mapped{};
    if (FAILED(context->Map(buffer, 0, D3D11_MAP_WRITE_DISCARD, 0, &mapped)) || !mapped.pData) { have = false; return false; }
    std::memcpy(mapped.pData, data, size);
    context->Unmap(buffer, 0);
    std::memcpy(last, data, size);
    have = true;
    ++uploads_;
    return true;
}

bool MotionDevice::set_core_vertex(ID3D11DeviceContext* context, const CoreVertexBytes& bytes) noexcept
{
    if (!upload(context, core_vs_.Get(), bytes.data(), bytes.size(), last_core_.data(), have_core_)) return false;
    ID3D11Buffer* buffer = core_vs_.Get();
    context->VSSetConstantBuffers(13, 1, &buffer);
    return true;
}
bool MotionDevice::set_foliage_vertex(ID3D11DeviceContext* context, const FoliageVertexBytes& bytes) noexcept
{
    if (!upload(context, foliage_vs_.Get(), bytes.data(), bytes.size(), last_foliage_.data(), have_foliage_)) return false;
    ID3D11Buffer* buffer = foliage_vs_.Get();
    context->VSSetConstantBuffers(13, 1, &buffer);
    return true;
}
bool MotionDevice::set_pixel(ID3D11DeviceContext* context, const PixelBytes& bytes) noexcept
{
    if (!upload(context, pixel_.Get(), bytes.data(), bytes.size(), last_pixel_.data(), have_pixel_)) return false;
    ID3D11Buffer* buffer = pixel_.Get();
    context->PSSetConstantBuffers(13, 1, &buffer);
    return true;
}

ReplayResult MotionDevice::begin_replay(ID3D11DeviceContext* context, Saved& s) noexcept try {
    if (!context || !rtv_ || !zero_ps_) return ReplayResult::Failed;
    context->OMGetRenderTargets(D3D11_SIMULTANEOUS_RENDER_TARGET_COUNT, s.targets, &s.depth);
    context->OMGetDepthStencilState(&s.depth_state, &s.reference);
    context->OMGetBlendState(&s.blend, s.factor, &s.mask);
    s.class_count = D3D11_SHADER_MAX_INTERFACES;
    context->PSGetShader(&s.shader, s.classes, &s.class_count);
    s.taken = true;
    ID3D11DepthStencilState* state = no_depth_.Get();
    D3D11_DEPTH_STENCIL_DESC desc{};
    if (s.depth_state) s.depth_state->GetDesc(&desc);
    else { desc.DepthEnable = TRUE; desc.DepthWriteMask = D3D11_DEPTH_WRITE_MASK_ALL; desc.DepthFunc = D3D11_COMPARISON_LESS; }
    if (s.depth && desc.DepthEnable) {
        if (desc.DepthWriteMask == D3D11_DEPTH_WRITE_MASK_ZERO) return ReplayResult::SkippedNoDepthWrite;
        state = equal_.Get();
    }
    ID3D11RenderTargetView* targets[3]{nullptr, nullptr, rtv_.Get()};
    context->OMSetRenderTargets(3, targets, s.depth);
    context->OMSetDepthStencilState(state, 0);
    context->OMSetBlendState(nullptr, nullptr, 0xffffffffu);
    context->PSSetShader(zero_ps_.Get(), nullptr, 0);
    return ReplayResult::Done;
} catch (...) { return ReplayResult::Failed; }

void MotionDevice::end_replay(ID3D11DeviceContext* context, Saved& s) noexcept
{
    if (!s.taken || !context) return;
    context->OMSetRenderTargets(D3D11_SIMULTANEOUS_RENDER_TARGET_COUNT, s.targets, s.depth);
    context->OMSetDepthStencilState(s.depth_state, s.reference);
    context->OMSetBlendState(s.blend, s.factor, s.mask);
    context->PSSetShader(s.shader, s.classes, s.class_count);
    for (auto* target : s.targets) if (target) target->Release();
    for (UINT i = 0; i < s.class_count; ++i) if (s.classes[i]) s.classes[i]->Release();
    if (s.depth) s.depth->Release();
    if (s.depth_state) s.depth_state->Release();
    if (s.blend) s.blend->Release();
    if (s.shader) s.shader->Release();
    s = {};
}
}
