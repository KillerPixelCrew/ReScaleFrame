// SPDX-License-Identifier: GPL-3.0-only

#include <rescaleframe/constant_twin.h>

#include <windows.h>

#include <d3d11.h>

#include <cstring>
#include <new>

struct rsf_constant_twins {
    ID3D11Device* device = nullptr;
    uint32_t bytes = 0;
    struct Entry {
        const void* original = nullptr;
        ID3D11Buffer* twin = nullptr;
        uint64_t written = 0;
    } entries[RSF_CONSTANT_TWINS];
    uint64_t clock = 0;
};

extern "C" rsf_constant_twins* rsf_constant_twins_create(void* device, uint32_t bytes)
{
    if (!device || bytes == 0 || (bytes % 16) != 0) {
        return nullptr;
    }
    auto* twins = new (std::nothrow) rsf_constant_twins();
    if (!twins) {
        return nullptr;
    }
    twins->device = static_cast<ID3D11Device*>(device);
    twins->device->AddRef();
    twins->bytes = bytes;
    return twins;
}

extern "C" void rsf_constant_twins_destroy(rsf_constant_twins* twins)
{
    if (!twins) {
        return;
    }
    for (auto& entry : twins->entries) {
        if (entry.twin) {
            entry.twin->Release();
        }
    }
    twins->device->Release();
    delete twins;
}

extern "C" void* rsf_constant_twins_write(rsf_constant_twins* twins, void* context_pointer,
                                          void* original, const void* contents)
{
    if (!twins || !context_pointer || !original || !contents) {
        return nullptr;
    }
    auto* context = static_cast<ID3D11DeviceContext*>(context_pointer);
    rsf_constant_twins::Entry* slot = nullptr;
    rsf_constant_twins::Entry* oldest = &twins->entries[0];
    for (auto& entry : twins->entries) {
        if (entry.original == original) {
            slot = &entry;
            break;
        }
        if (entry.written < oldest->written) {
            oldest = &entry;
        }
    }
    if (!slot) {
        // A twin already made is reused for the new original; only its owner changes.
        slot = oldest;
        slot->original = original;
    }
    if (!slot->twin) {
        D3D11_BUFFER_DESC description{};
        description.ByteWidth = twins->bytes;
        description.Usage = D3D11_USAGE_DYNAMIC;
        description.BindFlags = D3D11_BIND_CONSTANT_BUFFER;
        description.CPUAccessFlags = D3D11_CPU_ACCESS_WRITE;
        if (FAILED(twins->device->CreateBuffer(&description, nullptr, &slot->twin)) ||
            !slot->twin) {
            slot->original = nullptr;
            return nullptr;
        }
    }
    D3D11_MAPPED_SUBRESOURCE mapped{};
    if (FAILED(context->Map(slot->twin, 0, D3D11_MAP_WRITE_DISCARD, 0, &mapped)) || !mapped.pData) {
        return nullptr;
    }
    std::memcpy(mapped.pData, contents, twins->bytes);
    context->Unmap(slot->twin, 0);
    slot->written = ++twins->clock;
    return slot->twin;
}

extern "C" void* rsf_constant_twins_find(const rsf_constant_twins* twins, const void* original)
{
    if (!twins || !original) {
        return nullptr;
    }
    for (const auto& entry : twins->entries) {
        if (entry.original == original) {
            return entry.twin;
        }
    }
    return nullptr;
}

extern "C" void rsf_constant_twins_forget(rsf_constant_twins* twins, const void* original)
{
    if (!twins || !original) {
        return;
    }
    for (auto& entry : twins->entries) {
        if (entry.original == original) {
            // The buffer stays with the slot for the next original; only the pairing goes.
            entry.original = nullptr;
            entry.written = 0;
        }
    }
}
