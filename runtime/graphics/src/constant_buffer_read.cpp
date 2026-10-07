// SPDX-License-Identifier: GPL-3.0-only

#include <rescaleframe/constant_buffer_read.h>

#include <windows.h>

#include <d3d11.h>

#include <cstring>

extern "C" rsf_constant_buffer_result rsf_read_constant_buffer(void* device_pointer,
                                                               void* context_pointer,
                                                               void* buffer_pointer,
                                                               void* destination, uint32_t bytes)
{
    if (!device_pointer || !context_pointer || !buffer_pointer || !destination || bytes == 0) {
        return RSF_CONSTANT_BUFFER_ERROR_INVALID_ARGUMENT;
    }

    auto* device = static_cast<ID3D11Device*>(device_pointer);
    auto* context = static_cast<ID3D11DeviceContext*>(context_pointer);
    auto* buffer = static_cast<ID3D11Buffer*>(buffer_pointer);

    // Validate through ID3D11Resource before invoking the buffer-specific GetDesc slot.
    // Other resource types write different descriptor sizes through their own GetDesc method.
    D3D11_RESOURCE_DIMENSION dimension = D3D11_RESOURCE_DIMENSION_UNKNOWN;
    buffer->GetType(&dimension);
    if (dimension != D3D11_RESOURCE_DIMENSION_BUFFER) {
        return RSF_CONSTANT_BUFFER_ERROR_INVALID_ARGUMENT;
    }

    // Both the copied buffer and the context must belong to the staging device. D3D11 copies
    // return no HRESULT, so reject cross-device work before issuing the command.
    ID3D11Device* buffer_owner = nullptr;
    buffer->GetDevice(&buffer_owner);
    ID3D11Device* context_owner = nullptr;
    context->GetDevice(&context_owner);
    const bool same_device = buffer_owner == device && context_owner == device;
    if (buffer_owner) {
        buffer_owner->Release();
    }
    if (context_owner) {
        context_owner->Release();
    }
    if (!same_device) {
        return RSF_CONSTANT_BUFFER_ERROR_FOREIGN_DEVICE;
    }

    D3D11_BUFFER_DESC description{};
    buffer->GetDesc(&description);
    if (description.ByteWidth < bytes) {
        return RSF_CONSTANT_BUFFER_ERROR_TOO_SMALL;
    }

    // Described from scratch rather than copied from the source, so no bind flag or misc flag of
    // the original can make a staging buffer that D3D refuses to create. CopyResource between two
    // buffers only asks that the byte widths agree.
    D3D11_BUFFER_DESC staging{};
    staging.ByteWidth = description.ByteWidth;
    staging.Usage = D3D11_USAGE_STAGING;
    staging.BindFlags = 0;
    staging.CPUAccessFlags = D3D11_CPU_ACCESS_READ;
    staging.MiscFlags = 0;
    staging.StructureByteStride = 0;

    ID3D11Buffer* readable = nullptr;
    if (FAILED(device->CreateBuffer(&staging, nullptr, &readable)) || !readable) {
        return RSF_CONSTANT_BUFFER_ERROR_STAGING_FAILED;
    }

    // Map(READ) synchronizes with the queued copy. No bindings or source bytes are changed.
    context->CopyResource(readable, buffer);

    D3D11_MAPPED_SUBRESOURCE mapped{};
    if (FAILED(context->Map(readable, 0, D3D11_MAP_READ, 0, &mapped)) || !mapped.pData) {
        readable->Release();
        return RSF_CONSTANT_BUFFER_ERROR_MAP_FAILED;
    }

    std::memcpy(destination, mapped.pData, bytes);
    context->Unmap(readable, 0);
    readable->Release();
    return RSF_CONSTANT_BUFFER_OK;
}
