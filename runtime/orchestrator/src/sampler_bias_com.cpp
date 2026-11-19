// SPDX-License-Identifier: MIT
#include <d3d11.h>
#include <cstdint>

static_assert(D3D11_COMMONSHADER_SAMPLER_SLOT_COUNT == 16);
static_assert(D3D11_MIP_LOD_BIAS_MIN == -16.0f && D3D11_MIP_LOD_BIAS_MAX == 15.99f);

extern "C" int32_t rsf_sampler_bias_com_is_immediate(void* context)
{
    return static_cast<ID3D11DeviceContext*>(context)->GetType() == D3D11_DEVICE_CONTEXT_IMMEDIATE;
}

extern "C" void* rsf_sampler_bias_com_device(void* child)
{
    // Both the context and sampler interfaces inherit ID3D11DeviceChild.
    ID3D11Device* device = nullptr;
    static_cast<ID3D11DeviceChild*>(child)->GetDevice(&device);
    return device;
}

extern "C" void rsf_sampler_bias_com_read_slots(void* context, void** out)
{
    constexpr auto count = D3D11_COMMONSHADER_SAMPLER_SLOT_COUNT;
    ID3D11SamplerState* samplers[count]{};
    static_cast<ID3D11DeviceContext*>(context)->PSGetSamplers(0, count, samplers);
    for (int slot = 0; slot < count; ++slot) out[slot] = samplers[slot];
}

extern "C" void rsf_sampler_bias_com_write_slot(void* context, uint32_t slot, void* sampler)
{
    auto* state = static_cast<ID3D11SamplerState*>(sampler);
    static_cast<ID3D11DeviceContext*>(context)->PSSetSamplers(slot, 1, &state);
}

extern "C" float rsf_sampler_bias_com_read_bias(void* sampler)
{
    D3D11_SAMPLER_DESC desc{};
    static_cast<ID3D11SamplerState*>(sampler)->GetDesc(&desc);
    return desc.MipLODBias;
}

extern "C" void* rsf_sampler_bias_com_clone(void* device, void* original, float bias)
{
    D3D11_SAMPLER_DESC desc{};
    static_cast<ID3D11SamplerState*>(original)->GetDesc(&desc);
    desc.MipLODBias = bias;
    ID3D11SamplerState* replacement = nullptr;
    if (FAILED(static_cast<ID3D11Device*>(device)->CreateSamplerState(&desc, &replacement))) {
        if (replacement) replacement->Release();
        return nullptr;
    }
    return replacement;
}

extern "C" int32_t rsf_sampler_bias_com_device_ready(void* device)
{
    return SUCCEEDED(static_cast<ID3D11Device*>(device)->GetDeviceRemovedReason());
}
