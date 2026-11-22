// SPDX-License-Identifier: MIT
// Keep the SDK-specific COM calls and their structured exception guards on the native stack.
#include <d3d11.h>
#include <d3d12.h>

#include "census_internal.h"

namespace rsf_census {
namespace {

extern "C" bool read_d3d12(const void* resource, rsf_census_resource_desc* out) noexcept {
    if (!vtable_slot(resource, 10)) return false;
    return guard([&] {
        auto* r = static_cast<ID3D12Resource*>(const_cast<void*>(resource));
        const D3D12_RESOURCE_DESC d = r->GetDesc();
        *out = {d.Width, d.Height, static_cast<uint32_t>(d.Dimension),
                static_cast<uint32_t>(d.Format), d.DepthOrArraySize, d.MipLevels,
                d.SampleDesc.Count, static_cast<uint32_t>(d.Flags), true, true};
    });
}

extern "C" bool read_d3d11(const void* resource, rsf_census_resource_desc* out) noexcept {
    if (!vtable_slot(resource, 10)) return false;
    return guard([&] {
        auto* r = static_cast<ID3D11Resource*>(const_cast<void*>(resource));
        D3D11_RESOURCE_DIMENSION dim{};
        r->GetType(&dim);
        out->dimension = static_cast<uint32_t>(dim);
        out->dimension_read = true;
        if (dim == D3D11_RESOURCE_DIMENSION_TEXTURE2D) {
            D3D11_TEXTURE2D_DESC d{};
            static_cast<ID3D11Texture2D*>(r)->GetDesc(&d);
            *out = {d.Width, d.Height, static_cast<uint32_t>(dim), static_cast<uint32_t>(d.Format),
                    d.ArraySize, d.MipLevels, 0, d.BindFlags, true, true};
        }
    });
}

} // namespace

void json_d3d12_resource(Json& j, const char* key, const void* resource) noexcept {
    j.resource(key, resource, true, read_d3d12);
}

void json_d3d11_resource(Json& j, const char* key, const void* resource) noexcept {
    j.resource(key, resource, false, read_d3d11);
}

} // namespace rsf_census
