/* SPDX-License-Identifier: MIT */
#pragma once

#include "motion_d3d11_platform.h"
#include <rescaleframe/shader_compile.h>

struct rsf_colour11_platform {
    void* (*compile)(void*, const char*, const char*, const char*);
    void* (*constant_buffer)(void*, uint32_t);
    void* (*sampler)(void*, uint32_t);
    void* (*target_view)(void*, void*, uint32_t);
    decltype(&rsf::motion11::source_view) source_view;
    decltype(&rsf_d3d11_state_save) save;
    decltype(&rsf_d3d11_state_restore) restore;
    void (*prepare_transport)(void*);
    void (*bind_parameters)(void*, void*, void*, const void*);
    void (*dispatch)(void*, void*, void* const*, uint32_t, void*, uint32_t, uint32_t, uint32_t);
};
static_assert(sizeof(rsf_colour11_platform) == 10 * sizeof(void*));
static_assert(D3D11_BIND_UNORDERED_ACCESS == 0x80);

namespace rsf::colour11 {

inline void* compile(void* device, const char* source, const char* name, const char* entry) noexcept try
{
    Microsoft::WRL::ComPtr<ID3D11ComputeShader> shader;
    if (!compile_compute(static_cast<ID3D11Device*>(device), source, name, entry, &shader)) return nullptr;
    return shader.Detach();
} catch (...) { return nullptr; }

inline void* constant_buffer(void* device, uint32_t bytes)
{
    D3D11_BUFFER_DESC desc{};
    desc.ByteWidth = bytes;
    desc.Usage = D3D11_USAGE_DEFAULT;
    desc.BindFlags = D3D11_BIND_CONSTANT_BUFFER;
    Microsoft::WRL::ComPtr<ID3D11Buffer> buffer;
    if (FAILED(static_cast<ID3D11Device*>(device)->CreateBuffer(&desc, nullptr, &buffer))) return nullptr;
    return buffer.Detach();
}

inline void* sampler(void* device, uint32_t mip_linear)
{
    D3D11_SAMPLER_DESC desc{};
    desc.Filter = mip_linear ? D3D11_FILTER_MIN_MAG_MIP_LINEAR : D3D11_FILTER_MIN_MAG_LINEAR_MIP_POINT;
    desc.AddressU = desc.AddressV = desc.AddressW = D3D11_TEXTURE_ADDRESS_CLAMP;
    desc.MaxLOD = mip_linear ? D3D11_FLOAT32_MAX : 0.0f;
    Microsoft::WRL::ComPtr<ID3D11SamplerState> sampler;
    if (FAILED(static_cast<ID3D11Device*>(device)->CreateSamplerState(&desc, &sampler))) return nullptr;
    return sampler.Detach();
}

inline void* target_view(void* device, void* texture, uint32_t format)
{
    D3D11_UNORDERED_ACCESS_VIEW_DESC desc{};
    desc.Format = static_cast<DXGI_FORMAT>(format);
    desc.ViewDimension = D3D11_UAV_DIMENSION_TEXTURE2D;
    Microsoft::WRL::ComPtr<ID3D11UnorderedAccessView> target;
    if (FAILED(static_cast<ID3D11Device*>(device)->CreateUnorderedAccessView(
        static_cast<ID3D11Texture2D*>(texture), format == DXGI_FORMAT_UNKNOWN ? nullptr : &desc, &target))) {
        return nullptr;
    }
    return target.Detach();
}

inline void prepare_transport(void* raw_context)
{
    auto* context = static_cast<ID3D11DeviceContext*>(raw_context);
    context->SetPredication(nullptr, FALSE);
    context->OMSetRenderTargets(0, nullptr, nullptr);
}

inline void bind_parameters(void* raw_context, void* raw_buffer, void* raw_sampler, const void* constants)
{
    auto* context = static_cast<ID3D11DeviceContext*>(raw_context);
    auto* buffer = static_cast<ID3D11Buffer*>(raw_buffer);
    auto* sampler = static_cast<ID3D11SamplerState*>(raw_sampler);
    context->UpdateSubresource(buffer, 0, nullptr, constants, 0, 0);
    context->CSSetConstantBuffers(0, 1, &buffer);
    context->CSSetSamplers(0, 1, &sampler);
}

inline void dispatch(void* raw_context, void* shader, void* const* raw_sources, uint32_t count,
    void* raw_target, uint32_t groups_x, uint32_t groups_y, uint32_t clear_sources)
{
    auto* context = static_cast<ID3D11DeviceContext*>(raw_context);
    auto* target = static_cast<ID3D11UnorderedAccessView*>(raw_target);
    ID3D11ShaderResourceView* sources[4]{};
    for (uint32_t i = 0; i < count; ++i) {
        sources[i] = static_cast<ID3D11ShaderResourceView*>(raw_sources[i]);
    }
    context->CSSetShader(static_cast<ID3D11ComputeShader*>(shader), nullptr, 0);
    context->CSSetShaderResources(0, count, sources);
    context->CSSetUnorderedAccessViews(0, 1, &target, nullptr);
    context->Dispatch(groups_x, groups_y, 1);
    target = nullptr;
    context->CSSetUnorderedAccessViews(0, 1, &target, nullptr);
    if (clear_sources) {
        ID3D11ShaderResourceView* empty[4]{};
        context->CSSetShaderResources(0, count, empty);
    }
}

inline constexpr rsf_colour11_platform platform{
    compile, constant_buffer, sampler, target_view, rsf::motion11::source_view,
    rsf_d3d11_state_save, rsf_d3d11_state_restore, prepare_transport, bind_parameters, dispatch,
};

} // namespace rsf::colour11
