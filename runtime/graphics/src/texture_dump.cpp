// SPDX-License-Identifier: MIT

#include "texture_dump_platform.h"

#include <d3d11.h>
#include <wincodec.h>
#include <wrl/client.h>

#include <cstddef>
#include <cstdio>
#include <string>

namespace {
using Microsoft::WRL::ComPtr;

static_assert(sizeof(rsf_texture_dump_options) == 40 && sizeof(rsf_dump_pixels) == 40 &&
    sizeof(rsf_texture_dump_report) == 40);
static_assert(sizeof(rsf_texture_dump_desc) == 36 && sizeof(rsf_texture_dump_mapping) == 16 &&
    sizeof(rsf_texture_dump_platform) == 80);
static_assert(offsetof(rsf_texture_dump_mapping, row_pitch) == 8 &&
    offsetof(rsf_texture_dump_platform, describe) == 8 && offsetof(rsf_texture_dump_platform, write_png) == 72);
static_assert(RSF_TEXTURE_DUMP_ABI_VERSION == 2 &&
    RSF_TEXTURE_OK == 0 && RSF_TEXTURE_ERROR_INVALID_ARGUMENT == -1 &&
    RSF_TEXTURE_ERROR_ABI_MISMATCH == -2 && RSF_TEXTURE_ERROR_UNSUPPORTED_FORMAT == -3 &&
    RSF_TEXTURE_ERROR_STAGING_FAILED == -4 && RSF_TEXTURE_ERROR_MAP_FAILED == -5 &&
    RSF_TEXTURE_ERROR_WRITE_FAILED == -6 && RSF_TEXTURE_ERROR_FOREIGN_DEVICE == -7);
static_assert(RSF_TEXTURE_OUTPUT_TARGA == 0 && RSF_TEXTURE_OUTPUT_PNG == 1 && RSF_TEXTURE_OUTPUT_RAW == 2);
static_assert(RSF_DUMP_VIEW_VELOCITY == 1 && RSF_DUMP_VIEW_DECODED_MOTION == 2 &&
    RSF_DUMP_VIEW_HDR == 3 && RSF_DUMP_VIEW_DEPTH == 4 && RSF_DUMP_VIEW_SCALAR == 5 &&
    RSF_DUMP_VIEW_ALPHA == 6 && RSF_DUMP_VIEW_DENSE_MOTION == 7 &&
    RSF_DUMP_DECODED_SENTINEL_THRESHOLD == -100.0f);
static_assert(DXGI_FORMAT_R8_TYPELESS == 60 && DXGI_FORMAT_R8_UNORM == 61 &&
    DXGI_FORMAT_R8G8_UNORM == 49 && DXGI_FORMAT_R16_FLOAT == 54 &&
    DXGI_FORMAT_R16_TYPELESS == 53 && DXGI_FORMAT_R16_UNORM == 56 &&
    DXGI_FORMAT_R24G8_TYPELESS == 44 && DXGI_FORMAT_D24_UNORM_S8_UINT == 45 &&
    DXGI_FORMAT_R32_TYPELESS == 39 && DXGI_FORMAT_D32_FLOAT == 40 && DXGI_FORMAT_R32_FLOAT == 41 &&
    DXGI_FORMAT_R32G8X24_TYPELESS == 19 && DXGI_FORMAT_D32_FLOAT_S8X24_UINT == 20 &&
    DXGI_FORMAT_R10G10B10A2_TYPELESS == 23 && DXGI_FORMAT_R10G10B10A2_UNORM == 24 &&
    DXGI_FORMAT_R8G8B8A8_TYPELESS == 27 && DXGI_FORMAT_R8G8B8A8_UNORM == 28 &&
    DXGI_FORMAT_R8G8B8A8_UNORM_SRGB == 29 && DXGI_FORMAT_B8G8R8A8_UNORM == 87 &&
    DXGI_FORMAT_B8G8R8A8_TYPELESS == 90 && DXGI_FORMAT_B8G8R8A8_UNORM_SRGB == 91 &&
    DXGI_FORMAT_R16G16_FLOAT == 34 && DXGI_FORMAT_R16G16_UNORM == 35 &&
    DXGI_FORMAT_R32G32_FLOAT == 16 && DXGI_FORMAT_R11G11B10_FLOAT == 26 &&
    DXGI_FORMAT_R16G16B16A16_FLOAT == 10 && DXGI_FORMAT_R32G32B32A32_FLOAT == 2);

uint32_t describe(void* device, void* context, void* source, rsf_texture_dump_desc* out)
{
    auto* immediate = static_cast<ID3D11DeviceContext*>(context);
    auto* resource = static_cast<ID3D11Resource*>(source);
    D3D11_RESOURCE_DIMENSION dimension{};
    resource->GetType(&dimension);
    if (dimension != D3D11_RESOURCE_DIMENSION_TEXTURE2D) return 0;
    D3D11_TEXTURE2D_DESC desc{};
    static_cast<ID3D11Texture2D*>(resource)->GetDesc(&desc);
    ComPtr<ID3D11Device> context_owner, texture_owner;
    immediate->GetDevice(&context_owner);
    resource->GetDevice(&texture_owner);
    *out = {desc.Width, desc.Height, uint32_t(desc.Format), desc.MipLevels, desc.ArraySize,
        desc.SampleDesc.Count, uint32_t(immediate->GetType() == D3D11_DEVICE_CONTEXT_IMMEDIATE),
        uint32_t(context_owner.Get() == device), uint32_t(texture_owner.Get() == device)};
    return 1;
}

void* stage(void* device, const rsf_texture_dump_desc* info)
{
    D3D11_TEXTURE2D_DESC desc{};
    desc.Width = info->width;
    desc.Height = info->height;
    desc.Format = DXGI_FORMAT(info->format);
    desc.MipLevels = 1;
    desc.ArraySize = 1;
    desc.SampleDesc.Count = 1;
    desc.Usage = D3D11_USAGE_STAGING;
    desc.CPUAccessFlags = D3D11_CPU_ACCESS_READ;
    ComPtr<ID3D11Texture2D> texture;
    return SUCCEEDED(static_cast<ID3D11Device*>(device)->CreateTexture2D(&desc, nullptr, &texture))
        ? texture.Detach() : nullptr;
}

void copy(void* context, void* dest, void* source, uint32_t top_subresource)
{
    auto* immediate = static_cast<ID3D11DeviceContext*>(context);
    auto* readable = static_cast<ID3D11Texture2D*>(dest);
    auto* texture = static_cast<ID3D11Resource*>(source);
    if (top_subresource) {
        immediate->CopySubresourceRegion(readable, 0, 0, 0, 0, texture, 0, nullptr);
    } else {
        immediate->CopyResource(readable, texture);
    }
}

uint32_t map(void* context, void* texture, rsf_texture_dump_mapping* out)
{
    D3D11_MAPPED_SUBRESOURCE mapped{};
    if (FAILED(static_cast<ID3D11DeviceContext*>(context)->Map(
        static_cast<ID3D11Texture2D*>(texture), 0, D3D11_MAP_READ, 0, &mapped))) return 0;
    *out = {static_cast<const uint8_t*>(mapped.pData), mapped.RowPitch};
    return 1;
}

void unmap(void* context, void* texture)
{
    static_cast<ID3D11DeviceContext*>(context)->Unmap(static_cast<ID3D11Texture2D*>(texture), 0);
}

std::wstring wide_path(const char* path)
{
    const int length = MultiByteToWideChar(CP_UTF8, MB_ERR_INVALID_CHARS, path, -1, nullptr, 0);
    if (!length) return {};
    std::wstring filename(size_t(length), L'\0');
    if (!MultiByteToWideChar(CP_UTF8, MB_ERR_INVALID_CHARS, path, -1, filename.data(), length)) return {};
    return filename;
}

void* open_file(const char* path) try
{
    const auto filename = wide_path(path);
    return filename.empty() ? nullptr : _wfopen(filename.c_str(), L"wb");
}
catch (...) { return nullptr; }

uint32_t write_file(void* file, const uint8_t* data, size_t bytes)
{
    return std::fwrite(data, 1, bytes, static_cast<std::FILE*>(file)) == bytes;
}

uint32_t close_file(void* file)
{
    return std::fclose(static_cast<std::FILE*>(file)) == 0;
}

uint32_t write_png(const char* path, uint32_t width, uint32_t height, uint32_t pitch,
    uint32_t bytes, uint8_t* bgra) try
{
    const HRESULT initialized = CoInitializeEx(nullptr, COINIT_MULTITHREADED);
    if (FAILED(initialized) && initialized != RPC_E_CHANGED_MODE) return 0;
    struct Apartment {
        bool owned;
        ~Apartment() { if (owned) CoUninitialize(); }
    } apartment{SUCCEEDED(initialized)};
    const auto filename = wide_path(path);
    if (filename.empty()) return 0;
    ComPtr<IWICImagingFactory> factory;
    ComPtr<IWICStream> stream;
    ComPtr<IWICBitmapEncoder> encoder;
    ComPtr<IWICBitmapFrameEncode> frame;
    WICPixelFormatGUID format = GUID_WICPixelFormat32bppBGRA;
    return SUCCEEDED(CoCreateInstance(CLSID_WICImagingFactory, nullptr, CLSCTX_INPROC_SERVER, IID_PPV_ARGS(&factory))) &&
        SUCCEEDED(factory->CreateStream(&stream)) && SUCCEEDED(stream->InitializeFromFilename(filename.c_str(), GENERIC_WRITE)) &&
        SUCCEEDED(factory->CreateEncoder(GUID_ContainerFormatPng, nullptr, &encoder)) &&
        SUCCEEDED(encoder->Initialize(stream.Get(), WICBitmapEncoderNoCache)) &&
        SUCCEEDED(encoder->CreateNewFrame(&frame, nullptr)) && SUCCEEDED(frame->Initialize(nullptr)) &&
        SUCCEEDED(frame->SetSize(width, height)) && SUCCEEDED(frame->SetPixelFormat(&format)) &&
        IsEqualGUID(format, GUID_WICPixelFormat32bppBGRA) &&
        SUCCEEDED(frame->WritePixels(height, pitch, bytes, bgra)) &&
        SUCCEEDED(frame->Commit()) && SUCCEEDED(encoder->Commit());
}
catch (...) { return 0; }

const rsf_texture_dump_platform platform{sizeof(platform), describe, stage, copy, map, unmap,
    open_file, write_file, close_file, write_png};
} // namespace

extern "C" rsf_dump_texture_result rsf_dump_texture(void* device, void* context, void* texture,
    const rsf_texture_dump_options* options, rsf_texture_dump_report* report)
{
    return rsf_core_texture_dump_texture(&platform, device, context, texture, options, report, RSF_TEXTURE_OUTPUT_TARGA);
}

extern "C" rsf_dump_texture_result rsf_dump_texture_png(void* device, void* context, void* texture,
    const rsf_texture_dump_options* options, rsf_texture_dump_report* report)
{
    return rsf_core_texture_dump_texture(&platform, device, context, texture, options, report, RSF_TEXTURE_OUTPUT_PNG);
}

extern "C" rsf_dump_texture_result rsf_dump_texture_bytes(void* device, void* context, void* texture,
    const rsf_texture_dump_options* options)
{
    return rsf_core_texture_dump_texture(&platform, device, context, texture, options, nullptr, RSF_TEXTURE_OUTPUT_RAW);
}

extern "C" rsf_dump_texture_result rsf_dump_pixels_png(const rsf_dump_pixels* pixels,
    const rsf_texture_dump_options* options, rsf_texture_dump_report* report)
{
    return rsf_core_texture_dump_pixels(&platform, pixels, options, report);
}
