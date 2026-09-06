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

    // GetDesc below is reached through the vtable of whatever was really passed, and every D3D11
    // resource type has one at that slot writing a differently sized descriptor. A texture arriving
    // here would write a D3D11_TEXTURE2D_DESC across a D3D11_BUFFER_DESC on this stack and take the
    // process down with no message. GetType is inherited from ID3D11Resource and so is identical
    // for every resource type, which makes it the one call that is safe to make first.
    D3D11_RESOURCE_DIMENSION dimension = D3D11_RESOURCE_DIMENSION_UNKNOWN;
    buffer->GetType(&dimension);
    if (dimension != D3D11_RESOURCE_DIMENSION_BUFFER) {
        return RSF_CONSTANT_BUFFER_ERROR_INVALID_ARGUMENT;
    }

    // Same rule as the texture dump and the motion decode: a resource from another device takes the
    // process down rather than failing the copy, and this project has already hit it. The observer
    // creates a throwaway device to reach the vtables, and any overlay in the process may add one.
    //
    // The context is checked as well as the buffer. The staging copy is created on `device` but the
    // copy is issued on `context`, so a context belonging to a second device is the same fault by
    // another route, and it is the easier of the two mistakes to make: device and context arrive as
    // separate arguments and a hook usually picks them up from different places.
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
