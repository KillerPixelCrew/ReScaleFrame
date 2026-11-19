// SPDX-License-Identifier: MIT
#include <d3d11.h>
#include <cstdint>

namespace {
struct Dispatch {
    void* shader;
    void* constants;
    void* views[3];
    void* targets[2];
    uint32_t view_count, target_count, groups[2];
};
static_assert(sizeof(Dispatch) == 72, "Rust region dispatch layout");
}

extern "C" void* rsf_regions_device_identity(void* device)
{
    IUnknown* identity = nullptr;
    if (FAILED(static_cast<ID3D11Device*>(device)->QueryInterface(IID_PPV_ARGS(&identity)))) return nullptr;
    return identity;
}

extern "C" void* rsf_regions_create_constants(void* device, uint32_t size)
{
    D3D11_BUFFER_DESC desc{};
    desc.ByteWidth = size;
    desc.Usage = D3D11_USAGE_DEFAULT;
    desc.BindFlags = D3D11_BIND_CONSTANT_BUFFER;
    ID3D11Buffer* result = nullptr;
    if (FAILED(static_cast<ID3D11Device*>(device)->CreateBuffer(&desc, nullptr, &result))) return nullptr;
    return result;
}

extern "C" void* rsf_regions_create_uav(void* device, void* texture)
{
    ID3D11UnorderedAccessView* result = nullptr;
    if (FAILED(static_cast<ID3D11Device*>(device)->CreateUnorderedAccessView(
            static_cast<ID3D11Texture2D*>(texture), nullptr, &result))) return nullptr;
    return result;
}

extern "C" void rsf_regions_crop(void* context, void* target, void* source, const uint32_t bounds[4])
{
    const D3D11_BOX box{bounds[0], bounds[1], 0, bounds[2], bounds[3], 1};
    static_cast<ID3D11DeviceContext*>(context)->CopySubresourceRegion(
        static_cast<ID3D11Texture2D*>(target), 0, 0, 0, 0, static_cast<ID3D11Texture2D*>(source), 0, &box);
}

extern "C" void rsf_regions_update_constants(void* context, void* buffer, const void* data)
{
    static_cast<ID3D11DeviceContext*>(context)->UpdateSubresource(
        static_cast<ID3D11Buffer*>(buffer), 0, nullptr, data, 0, 0);
}

extern "C" void rsf_regions_dispatch(void* context, const Dispatch* pass)
{
    auto* ctx = static_cast<ID3D11DeviceContext*>(context);
    auto* cb = static_cast<ID3D11Buffer*>(pass->constants);
    ID3D11ShaderResourceView* views[]{static_cast<ID3D11ShaderResourceView*>(pass->views[0]),
        static_cast<ID3D11ShaderResourceView*>(pass->views[1]), static_cast<ID3D11ShaderResourceView*>(pass->views[2])};
    ID3D11UnorderedAccessView* targets[]{static_cast<ID3D11UnorderedAccessView*>(pass->targets[0]),
        static_cast<ID3D11UnorderedAccessView*>(pass->targets[1])};
    ctx->CSSetShader(static_cast<ID3D11ComputeShader*>(pass->shader), nullptr, 0);
    ctx->CSSetShaderResources(0, pass->view_count, views);
    ctx->CSSetUnorderedAccessViews(0, pass->target_count, targets, nullptr);
    if (cb) ctx->CSSetConstantBuffers(0, 1, &cb);
    ctx->Dispatch(pass->groups[0], pass->groups[1], 1);
    for (auto& view : views) view = nullptr;
    for (auto& target : targets) target = nullptr;
    ctx->CSSetShaderResources(0, pass->view_count, views);
    ctx->CSSetUnorderedAccessViews(0, pass->target_count, targets, nullptr);
}
