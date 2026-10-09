/* SPDX-License-Identifier: GPL-3.0-only */
#pragma once
/* Shader resource views on textures that are the same from one frame to the next.

   The passes read engine targets (scene colour, depth, motion, exposure) that live in a pool and come
   back as the same few textures every frame. Creating a view on each of them per pass per frame and
   releasing it again is pure driver work, so a pass keeps one of these and asks it for the view.

   An entry holds a reference to its texture. That is what makes the pointer a safe key: a texture
   that is still referenced cannot be freed and have its address handed to a different one. It also
   keeps the texture alive a little past the engine's release of it, which the capacity and the idle
   limit bound: an entry nobody asked for within `idle_ms` is dropped on the next request, so a
   resolution change does not leave full size targets pinned.

   Not thread safe, like the passes that own it. The returned view belongs to the cache and stays
   valid until the cache is asked for more views than it has room for; callers use a handful per
   dispatch, well inside the capacity. */
#include <windows.h>
#include <d3d11.h>
#include <wrl/client.h>
#include <cstddef>
#include <cstdint>

namespace rsf {
template <size_t Capacity = 8>
class SrvCache {
public:
    /* A view of `texture` as `format`. DXGI_FORMAT_UNKNOWN asks for the texture's own format, which
       is only valid for a typed texture. Null when the view cannot be created. */
    ID3D11ShaderResourceView* get(ID3D11Device* device, ID3D11Texture2D* texture,
                                  DXGI_FORMAT format = DXGI_FORMAT_UNKNOWN)
    {
        if (!device || !texture) {
            return nullptr;
        }
        const ULONGLONG now = GetTickCount64();
        Entry* match = nullptr;
        Entry* spare = nullptr;
        for (Entry& entry : entries) {
            if (entry.view && now - entry.used > idle_ms) {
                entry = Entry{};
            }
            if (entry.view && entry.texture.Get() == texture && entry.format == format) {
                match = &entry;
            } else if (!entry.view && !spare) {
                spare = &entry;
            }
        }
        if (match) {
            match->used = now;
            return match->view.Get();
        }
        if (!spare) {
            spare = &entries[0];
            for (Entry& entry : entries) {
                if (entry.used < spare->used) {
                    spare = &entry;
                }
            }
            *spare = Entry{};
        }
        D3D11_SHADER_RESOURCE_VIEW_DESC description{};
        description.Format = format;
        description.ViewDimension = D3D11_SRV_DIMENSION_TEXTURE2D;
        description.Texture2D.MipLevels = 1;
        if (FAILED(device->CreateShaderResourceView(texture, format == DXGI_FORMAT_UNKNOWN ? nullptr : &description,
                                                    &spare->view)) ||
            !spare->view) {
            *spare = Entry{};
            return nullptr;
        }
        spare->texture = texture;
        spare->format = format;
        spare->used = now;
        return spare->view.Get();
    }

    void clear()
    {
        for (Entry& entry : entries) {
            entry = Entry{};
        }
    }

private:
    static constexpr ULONGLONG idle_ms = 2000;
    struct Entry {
        Microsoft::WRL::ComPtr<ID3D11Texture2D> texture;
        Microsoft::WRL::ComPtr<ID3D11ShaderResourceView> view;
        DXGI_FORMAT format = DXGI_FORMAT_UNKNOWN;
        ULONGLONG used = 0;
    };
    Entry entries[Capacity];
};
}
