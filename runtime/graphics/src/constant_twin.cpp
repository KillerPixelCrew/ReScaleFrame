// SPDX-License-Identifier: MIT
#include "device_owner.h"

#include <rescaleframe/constant_twin.h>

#include <cstring>
#include <wrl/client.h>

struct rsf_constant_twins_platform {
    uint32_t (*owns_context)(void*, void*);
    void* (*make_buffer)(void*, uint32_t);
    uint32_t (*upload)(void*, void*, const void*, uint32_t);
    uint32_t (*matches_size)(const void*, uint32_t);
};

static_assert(RSF_CONSTANT_TWINS == 32);
static_assert(sizeof(rsf_constant_twins_platform) == 4 * sizeof(void*));

namespace {
uint32_t owns_context(void* device, void* context)
{
    return rsf::same_device(static_cast<ID3D11DeviceContext*>(context),
        static_cast<ID3D11Device*>(device));
}

void* make_buffer(void* raw_device, uint32_t bytes)
{
    D3D11_BUFFER_DESC desc{};
    desc.ByteWidth = bytes;
    desc.Usage = D3D11_USAGE_DYNAMIC;
    desc.BindFlags = D3D11_BIND_CONSTANT_BUFFER;
    desc.CPUAccessFlags = D3D11_CPU_ACCESS_WRITE;
    auto* device = static_cast<ID3D11Device*>(raw_device);
    Microsoft::WRL::ComPtr<ID3D11Buffer> buffer;
    if (FAILED(device->CreateBuffer(&desc, nullptr, &buffer))) return nullptr;
    return buffer.Detach();
}

uint32_t upload(void* raw_context, void* raw_buffer, const void* contents, uint32_t bytes)
{
    auto* context = static_cast<ID3D11DeviceContext*>(raw_context);
    auto* buffer = static_cast<ID3D11Buffer*>(raw_buffer);
    D3D11_MAPPED_SUBRESOURCE mapped{};
    if (FAILED(context->Map(buffer, 0, D3D11_MAP_WRITE_DISCARD, 0, &mapped))) return 0;
    if (mapped.pData) std::memcpy(mapped.pData, contents, bytes);
    context->Unmap(buffer, 0);
    return mapped.pData != nullptr;
}

uint32_t matches_size(const void* original, uint32_t bytes)
{
    D3D11_BUFFER_DESC desc{};
    static_cast<ID3D11Buffer*>(const_cast<void*>(original))->GetDesc(&desc);
    return desc.ByteWidth == bytes && (desc.BindFlags & D3D11_BIND_CONSTANT_BUFFER) != 0;
}

constexpr rsf_constant_twins_platform platform{owns_context, make_buffer, upload, matches_size};
}

extern "C" {
rsf_constant_twins* rsf_core_constant_twins_create(void*, uint32_t, const rsf_constant_twins_platform*);
void rsf_core_constant_twins_destroy(rsf_constant_twins*);
void* rsf_core_constant_twins_write(const rsf_constant_twins*, void*, const void*, const void*);
void* rsf_core_constant_twins_find(const rsf_constant_twins*, const void*);
void rsf_core_constant_twins_forget(const rsf_constant_twins*, const void*);

rsf_constant_twins* rsf_constant_twins_create(void* device, uint32_t bytes)
{
    return rsf_core_constant_twins_create(device, bytes, &platform);
}

void rsf_constant_twins_destroy(rsf_constant_twins* twins)
{
    rsf_core_constant_twins_destroy(twins);
}

void* rsf_constant_twins_write(rsf_constant_twins* twins, void* context, void* original,
                               const void* contents)
{
    return rsf_core_constant_twins_write(twins, context, original, contents);
}

void* rsf_constant_twins_find(const rsf_constant_twins* twins, const void* original)
{
    return rsf_core_constant_twins_find(twins, original);
}

void rsf_constant_twins_forget(rsf_constant_twins* twins, const void* original)
{
    rsf_core_constant_twins_forget(twins, original);
}
}
