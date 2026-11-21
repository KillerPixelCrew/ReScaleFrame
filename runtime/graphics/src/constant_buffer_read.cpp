// SPDX-License-Identifier: MIT
#include <rescaleframe/constant_buffer_read.h>
#include <rescaleframe/d3d11_state.h>

#include <d3d11.h>
#include <wrl/client.h>

struct rsf_constant_read_platform {
    uint32_t (*buffer_size)(void*, uint32_t*);
    void* (*staging)(void*, uint32_t);
    void (*copy)(void*, void*, void*);
    uint32_t (*map)(void*, void*, const uint8_t**);
    void (*unmap)(void*, void*);
};

static_assert(sizeof(rsf_constant_read_platform) == 5 * sizeof(void*));
static_assert(RSF_CONSTANT_BUFFER_OK == 0 && RSF_CONSTANT_BUFFER_ERROR_INVALID_ARGUMENT == -1 &&
    RSF_CONSTANT_BUFFER_ERROR_TOO_SMALL == -3 && RSF_CONSTANT_BUFFER_ERROR_STAGING_FAILED == -4 &&
    RSF_CONSTANT_BUFFER_ERROR_MAP_FAILED == -5 && RSF_CONSTANT_BUFFER_ERROR_FOREIGN_DEVICE == -6);

namespace {
uint32_t buffer_size(void* raw, uint32_t* bytes)
{
    auto* resource = static_cast<ID3D11Resource*>(raw);
    D3D11_RESOURCE_DIMENSION dimension = D3D11_RESOURCE_DIMENSION_UNKNOWN;
    resource->GetType(&dimension);
    // Only buffers write a D3D11_BUFFER_DESC through the GetDesc slot.
    if (dimension != D3D11_RESOURCE_DIMENSION_BUFFER) return 0;
    D3D11_BUFFER_DESC desc{};
    static_cast<ID3D11Buffer*>(resource)->GetDesc(&desc);
    *bytes = desc.ByteWidth;
    return 1;
}

void* make_staging(void* device, uint32_t bytes)
{
    D3D11_BUFFER_DESC desc{};
    desc.ByteWidth = bytes;
    desc.Usage = D3D11_USAGE_STAGING;
    desc.CPUAccessFlags = D3D11_CPU_ACCESS_READ;
    Microsoft::WRL::ComPtr<ID3D11Buffer> buffer;
    if (FAILED(static_cast<ID3D11Device*>(device)->CreateBuffer(&desc, nullptr, &buffer))) {
        return nullptr;
    }
    return buffer.Detach();
}

uint32_t map_read(void* context, void* buffer, const uint8_t** out)
{
    D3D11_MAPPED_SUBRESOURCE mapped{};
    if (FAILED(static_cast<ID3D11DeviceContext*>(context)->Map(
        static_cast<ID3D11Buffer*>(buffer), 0, D3D11_MAP_READ, 0, &mapped))) {
        return 0;
    }
    *out = static_cast<const uint8_t*>(mapped.pData);
    return 1;
}

void unmap(void* context, void* buffer)
{
    static_cast<ID3D11DeviceContext*>(context)->Unmap(static_cast<ID3D11Buffer*>(buffer), 0);
}

constexpr rsf_constant_read_platform platform{
    buffer_size, make_staging, rsf_d3d11_copy_resource, map_read, unmap,
};
}

extern "C" rsf_constant_buffer_result rsf_core_read_constant_buffer(
    void*, void*, void*, void*, uint32_t, const rsf_constant_read_platform*);

extern "C" rsf_constant_buffer_result rsf_read_constant_buffer(
    void* device, void* context, void* buffer, void* destination, uint32_t bytes)
{
    return rsf_core_read_constant_buffer(device, context, buffer, destination, bytes, &platform);
}
