// SPDX-License-Identifier: MIT
// One D3D11 call per operation for native_translucency.rs.
#include <d3d11.h>
#include <cstdint>

namespace {
struct Dispatch {
    void* shader;
    void* constants;
    void* sampler;
    void* views[3];
    void* targets[3];
    uint32_t groups[2];
};
static_assert(sizeof(Dispatch) == 80, "Rust translucency dispatch layout");
}

extern "C" void* rsf_translucency_create_sampler(void* device)
{
    D3D11_SAMPLER_DESC desc{};
    desc.Filter = D3D11_FILTER_MIN_MAG_MIP_LINEAR;
    desc.AddressU = desc.AddressV = desc.AddressW = D3D11_TEXTURE_ADDRESS_CLAMP;
    desc.MaxLOD = D3D11_FLOAT32_MAX;
    ID3D11SamplerState* result = nullptr;
    if (FAILED(static_cast<ID3D11Device*>(device)->CreateSamplerState(&desc, &result))) return nullptr;
    return result;
}

extern "C" void rsf_translucency_clear_predication(void* context)
{
    static_cast<ID3D11DeviceContext*>(context)->SetPredication(nullptr, FALSE);
}

extern "C" void rsf_translucency_dispatch(void* context, const Dispatch* pass)
{
    auto* ctx = static_cast<ID3D11DeviceContext*>(context);
    ID3D11ShaderResourceView* views[3]{};
    ID3D11UnorderedAccessView* targets[3]{};
    for (int i = 0; i < 3; ++i) {
        views[i] = static_cast<ID3D11ShaderResourceView*>(pass->views[i]);
        targets[i] = static_cast<ID3D11UnorderedAccessView*>(pass->targets[i]);
    }
    auto* buffer = static_cast<ID3D11Buffer*>(pass->constants);
    auto* sampler = static_cast<ID3D11SamplerState*>(pass->sampler);
    ctx->CSSetShader(static_cast<ID3D11ComputeShader*>(pass->shader), nullptr, 0);
    ctx->CSSetShaderResources(0, 3, views);
    ctx->CSSetUnorderedAccessViews(0, 3, targets, nullptr);
    ctx->CSSetConstantBuffers(0, 1, &buffer);
    ctx->CSSetSamplers(0, 1, &sampler);
    ctx->Dispatch(pass->groups[0], pass->groups[1], 1);
    ID3D11UnorderedAccessView* empty[3]{};
    ctx->CSSetUnorderedAccessViews(0, 3, empty, nullptr);
}
