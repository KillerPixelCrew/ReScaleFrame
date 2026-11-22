// SPDX-License-Identifier: MIT
#pragma once
#include "motion_draw_fixture.h"
#include <d3d11.h>
#include <d3d11shader.h>
#include <wrl/client.h>
#include <cstddef>
#include <cstdint>
#include <stdexcept>
#include <vector>

namespace rsf::anomaly::motion {

struct FixtureTargets {
    Microsoft::WRL::ComPtr<ID3D11Texture2D> position, albedo, depth;
    Microsoft::WRL::ComPtr<ID3D11RenderTargetView> position_rtv, albedo_rtv;
    Microsoft::WRL::ComPtr<ID3D11DepthStencilView> dsv;
    Microsoft::WRL::ComPtr<ID3D11DepthStencilState> less;
    Microsoft::WRL::ComPtr<ID3D11RasterizerState> raster;

    FixtureTargets() = default;
    FixtureTargets(ID3D11Device* device, uint32_t size, bool stencil)
    {
        rsf_anomaly_motion_fixture_targets raw;
        if (const auto* error = rsf_anomaly_motion_fixture_make_targets(device, size, stencil, &raw))
            throw std::runtime_error(error);
        position.Attach(static_cast<ID3D11Texture2D*>(raw.position_texture));
        position_rtv.Attach(static_cast<ID3D11RenderTargetView*>(raw.position_rtv));
        albedo.Attach(static_cast<ID3D11Texture2D*>(raw.albedo_texture));
        albedo_rtv.Attach(static_cast<ID3D11RenderTargetView*>(raw.albedo_rtv));
        depth.Attach(static_cast<ID3D11Texture2D*>(raw.depth_texture));
        dsv.Attach(static_cast<ID3D11DepthStencilView*>(raw.depth_view));
        less.Attach(static_cast<ID3D11DepthStencilState*>(raw.depth_state));
        raster.Attach(static_cast<ID3D11RasterizerState*>(raw.raster));
    }
};

inline std::vector<D3D11_SIGNATURE_PARAMETER_DESC> fixture_signature(ID3D11ShaderReflection* reflection)
{
    D3D11_SHADER_DESC desc{};
    if (FAILED(reflection->GetDesc(&desc))) throw std::runtime_error("reflect VS");
    std::vector<D3D11_SIGNATURE_PARAMETER_DESC> rows(desc.InputParameters);
    for (UINT index = 0; index < desc.InputParameters; ++index)
        if (FAILED(reflection->GetInputParameterDesc(index, &rows[index]))) throw std::runtime_error("reflect VS input");
    return rows;
}

inline Microsoft::WRL::ComPtr<ID3D11InputLayout> fixture_layout(ID3D11Device* device,
    const std::vector<D3D11_SIGNATURE_PARAMETER_DESC>& signature, const std::vector<std::byte>& stock)
{
    void* raw{};
    if (const auto* error = rsf_anomaly_motion_fixture_make_layout(device, signature.data(),
        static_cast<uint32_t>(signature.size()), stock.data(), stock.size(), &raw))
        throw std::runtime_error(error);
    Microsoft::WRL::ComPtr<ID3D11InputLayout> layout;
    layout.Attach(static_cast<ID3D11InputLayout*>(raw));
    return layout;
}

template<class T>
std::vector<T> fixture_readback(ID3D11Device* device, ID3D11DeviceContext* context, ID3D11Texture2D* texture, uint32_t size)
{
    std::vector<T> out(static_cast<size_t>(size) * size);
    if (!rsf_anomaly_motion_fixture_read(device, context, texture, out.data(), out.size() * sizeof(T)))
        throw std::runtime_error("staging readback failed");
    return out;
}

}
