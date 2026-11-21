/* SPDX-License-Identifier: MIT */
#pragma once
#include "device_owner.h"
#include <rescaleframe/d3d11_state.h>
#include <rescaleframe/srv_format.h>
#include <wrl/client.h>
#include <stddef.h>

struct rsf_motion_source_info { uint32_t width, height, slices, samples, format, view_format; };

namespace rsf::motion11 {
inline int32_t context_matches(void* device, void* raw)
{
    auto* context = static_cast<ID3D11DeviceContext*>(raw);
    return context->GetType() == D3D11_DEVICE_CONTEXT_IMMEDIATE &&
        same_device(context, static_cast<ID3D11Device*>(device));
}

inline int32_t describe_source(void* device, void* source, rsf_motion_source_info* info)
{
    auto* texture = static_cast<ID3D11Texture2D*>(source);
    if (!same_device(texture, static_cast<ID3D11Device*>(device))) return 0;
    D3D11_TEXTURE2D_DESC desc{};
    texture->GetDesc(&desc);
    *info = {desc.Width, desc.Height, desc.ArraySize, desc.SampleDesc.Count,
        uint32_t(desc.Format), uint32_t(srv_format(desc.Format))};
    return 1;
}

inline void* source_view(void* device, void* source, uint32_t format)
{
    if (format == DXGI_FORMAT_UNKNOWN) return rsf_d3d11_create_shader_view(device, source);
    D3D11_SHADER_RESOURCE_VIEW_DESC desc{};
    desc.Format = static_cast<DXGI_FORMAT>(format);
    desc.ViewDimension = D3D11_SRV_DIMENSION_TEXTURE2D;
    desc.Texture2D.MipLevels = 1;
    Microsoft::WRL::ComPtr<ID3D11ShaderResourceView> view;
    if (FAILED(static_cast<ID3D11Device*>(device)->CreateShaderResourceView(
        static_cast<ID3D11Texture2D*>(source), &desc, &view))) return nullptr;
    return view.Detach();
}

template<size_t S, size_t T>
int32_t dispatch(ID3D11DeviceContext* context, ID3D11ComputeShader* shader, ID3D11Buffer* constants,
    ID3D11ShaderResourceView* const (&sources)[S], ID3D11UnorderedAccessView* const (&targets)[T],
    const UINT* initial_counts, uint32_t groups_x, uint32_t groups_y)
{
    // The persistent output can still be an inherited SRV in any stage.
    rsf_d3d11_state saved{};
    if (!rsf_d3d11_state_save(context, &saved)) return 0;
    struct Restore {
        ID3D11DeviceContext* context;
        rsf_d3d11_state& state;
        ~Restore() { rsf_d3d11_state_restore(context, &state); }
    } restore{context, saved};
    context->SetPredication(nullptr, FALSE);
    context->OMSetRenderTargets(0, nullptr, nullptr);
    context->CSSetShader(shader, nullptr, 0);
    context->CSSetShaderResources(0, static_cast<UINT>(S), sources);
    context->CSSetUnorderedAccessViews(0, static_cast<UINT>(T), targets, initial_counts);
    context->CSSetConstantBuffers(0, 1, &constants);
    context->Dispatch(groups_x, groups_y, 1);
    ID3D11UnorderedAccessView* empty[T]{};
    context->CSSetUnorderedAccessViews(0, static_cast<UINT>(T), empty, initial_counts);
    return 1;
}
}
