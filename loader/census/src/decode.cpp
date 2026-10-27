// SPDX-License-Identifier: GPL-3.0-only
// Decoders shared by several vendors: DXGI formats, D3D11 and D3D12 resource descriptions and
// string arguments. Every read of game memory goes through SEH.
#include <d3d11.h>
#include <d3d12.h>

#include "census_internal.h"

namespace rsf_census {

const char* dxgi_format_name(uint32_t f) noexcept {
    switch (f) {
    case 0: return "UNKNOWN";
    case 1: return "R32G32B32A32_TYPELESS";
    case 2: return "R32G32B32A32_FLOAT";
    case 3: return "R32G32B32A32_UINT";
    case 6: return "R32G32B32_FLOAT";
    case 9: return "R16G16B16A16_TYPELESS";
    case 10: return "R16G16B16A16_FLOAT";
    case 11: return "R16G16B16A16_UNORM";
    case 13: return "R16G16B16A16_SNORM";
    case 15: return "R32G32_TYPELESS";
    case 16: return "R32G32_FLOAT";
    case 19: return "R32G8X24_TYPELESS";
    case 20: return "D32_FLOAT_S8X24_UINT";
    case 21: return "R32_FLOAT_X8X24_TYPELESS";
    case 23: return "R10G10B10A2_TYPELESS";
    case 24: return "R10G10B10A2_UNORM";
    case 26: return "R11G11B10_FLOAT";
    case 27: return "R8G8B8A8_TYPELESS";
    case 28: return "R8G8B8A8_UNORM";
    case 29: return "R8G8B8A8_UNORM_SRGB";
    case 31: return "R8G8B8A8_SNORM";
    case 33: return "R16G16_TYPELESS";
    case 34: return "R16G16_FLOAT";
    case 35: return "R16G16_UNORM";
    case 37: return "R16G16_SNORM";
    case 39: return "R32_TYPELESS";
    case 40: return "D32_FLOAT";
    case 41: return "R32_FLOAT";
    case 42: return "R32_UINT";
    case 44: return "R24G8_TYPELESS";
    case 45: return "D24_UNORM_S8_UINT";
    case 46: return "R24_UNORM_X8_TYPELESS";
    case 48: return "R8G8_TYPELESS";
    case 49: return "R8G8_UNORM";
    case 53: return "R16_TYPELESS";
    case 54: return "R16_FLOAT";
    case 55: return "D16_UNORM";
    case 56: return "R16_UNORM";
    case 60: return "R8_TYPELESS";
    case 61: return "R8_UNORM";
    case 67: return "R9G9B9E5_SHAREDEXP";
    case 87: return "B8G8R8A8_UNORM";
    case 88: return "B8G8R8X8_UNORM";
    case 90: return "B8G8R8A8_TYPELESS";
    case 91: return "B8G8R8A8_UNORM_SRGB";
    default: return "other";
    }
}

void json_ptr_or_null(Json& j, const char* key, const void* p) noexcept {
    if (p) {
        j.hex(key, reinterpret_cast<uint64_t>(p));
    } else {
        j.null(key);
    }
}

void json_d3d12_resource(Json& j, const char* key, const void* resource) noexcept {
    if (!resource) {
        j.null(key);
        return;
    }
    j.obj(key);
    j.hex("ptr", reinterpret_cast<uint64_t>(resource));
    D3D12_RESOURCE_DESC d{};
    bool ok = false;
    if (vtable_slot(resource, 10)) {
        ok = guard([&] {
            d = const_cast<ID3D12Resource*>(static_cast<const ID3D12Resource*>(resource))->GetDesc();
        });
    }
    if (ok) {
        j.u64("dimension", static_cast<uint64_t>(d.Dimension));
        j.u64("format", static_cast<uint64_t>(d.Format));
        j.str("format_name", dxgi_format_name(static_cast<uint32_t>(d.Format)));
        j.u64("width", d.Width);
        j.u64("height", d.Height);
        j.u64("depth_or_layers", d.DepthOrArraySize);
        j.u64("mips", d.MipLevels);
        j.u64("samples", d.SampleDesc.Count);
        j.hex("flags", static_cast<uint64_t>(d.Flags));
    } else {
        j.boolean("desc_unreadable", true);
    }
    j.end_obj();
}

void json_d3d11_resource(Json& j, const char* key, const void* resource) noexcept {
    if (!resource) {
        j.null(key);
        return;
    }
    j.obj(key);
    j.hex("ptr", reinterpret_cast<uint64_t>(resource));
    bool ok = false;
    if (vtable_slot(resource, 10)) {
        ok = guard([&] {
            auto* r = const_cast<ID3D11Resource*>(static_cast<const ID3D11Resource*>(resource));
            D3D11_RESOURCE_DIMENSION dim{};
            r->GetType(&dim);
            j.u64("dimension", static_cast<uint64_t>(dim));
            if (dim == D3D11_RESOURCE_DIMENSION_TEXTURE2D) {
                D3D11_TEXTURE2D_DESC d{};
                static_cast<ID3D11Texture2D*>(r)->GetDesc(&d);
                j.u64("format", static_cast<uint64_t>(d.Format));
                j.str("format_name", dxgi_format_name(static_cast<uint32_t>(d.Format)));
                j.u64("width", d.Width);
                j.u64("height", d.Height);
                j.u64("mips", d.MipLevels);
                j.u64("array_size", d.ArraySize);
                j.hex("bind_flags", d.BindFlags);
            }
        });
    }
    if (!ok) j.boolean("desc_unreadable", true);
    j.end_obj();
}

bool read_arg_string(const void* p, char* out, size_t cap) noexcept {
    out[0] = 0;
    return plausible_ptr(p) && safe_read_cstr(static_cast<const char*>(p), out, cap);
}

bool read_arg_wstring(const void* p, wchar_t* out, size_t cap) noexcept {
    out[0] = 0;
    return plausible_ptr(p) && safe_read_wstr(static_cast<const wchar_t*>(p), out, cap);
}

} // namespace rsf_census
