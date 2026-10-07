// SPDX-License-Identifier: GPL-3.0-only
#include <rescaleframe/texture_dump.h>
#include <d3d11.h>
#include <wrl/client.h>
#include <cstdio>
#include <string>

using Microsoft::WRL::ComPtr;
namespace {
uint32_t pixel_bytes(DXGI_FORMAT format)
{
    switch (format) {
    case DXGI_FORMAT_R8_UNORM: case DXGI_FORMAT_R8_TYPELESS: return 1;
    case DXGI_FORMAT_R16_FLOAT: case DXGI_FORMAT_R16_UNORM: case DXGI_FORMAT_R16_TYPELESS: return 2;
    case DXGI_FORMAT_R16G16_UNORM: case DXGI_FORMAT_R16G16_FLOAT:
    case DXGI_FORMAT_R10G10B10A2_UNORM: case DXGI_FORMAT_R10G10B10A2_TYPELESS:
    case DXGI_FORMAT_R11G11B10_FLOAT: case DXGI_FORMAT_R32_FLOAT: case DXGI_FORMAT_R32_TYPELESS:
    case DXGI_FORMAT_D32_FLOAT: case DXGI_FORMAT_R24G8_TYPELESS: case DXGI_FORMAT_D24_UNORM_S8_UINT:
    case DXGI_FORMAT_R8G8B8A8_TYPELESS: case DXGI_FORMAT_R8G8B8A8_UNORM:
    case DXGI_FORMAT_R8G8B8A8_UNORM_SRGB: case DXGI_FORMAT_B8G8R8A8_TYPELESS:
    case DXGI_FORMAT_B8G8R8A8_UNORM: case DXGI_FORMAT_B8G8R8A8_UNORM_SRGB: return 4;
    case DXGI_FORMAT_R16G16B16A16_FLOAT: case DXGI_FORMAT_R32G32_FLOAT:
    case DXGI_FORMAT_R32G8X24_TYPELESS: case DXGI_FORMAT_D32_FLOAT_S8X24_UINT: return 8;
    case DXGI_FORMAT_R32G32B32A32_FLOAT: return 16;
    default: return 0;
    }
}
// Scope guards balance Map/file ownership across early errors and the C ABI exception barrier.
struct Mapping {
    ID3D11DeviceContext* context;
    ID3D11Texture2D* texture;
    bool mapped = false;
    ~Mapping() { if (mapped) context->Unmap(texture, 0); }
};
struct File {
    FILE* value = nullptr;
    ~File() { if (value) std::fclose(value); }
    bool close() { FILE* p = value; value = nullptr; return p && std::fclose(p) == 0; }
};
}
extern "C" rsf_dump_texture_result rsf_dump_texture_bytes(
    void* device_pointer, void* context_pointer, void* texture_pointer,
    const rsf_texture_dump_options* options) try
{
    if (!device_pointer || !context_pointer || !texture_pointer || !options ||
        options->struct_size < sizeof(*options) || !options->output_prefix_utf8)
        return RSF_TEXTURE_ERROR_INVALID_ARGUMENT;
    if (options->abi_version != RSF_TEXTURE_DUMP_ABI_VERSION) return RSF_TEXTURE_ERROR_ABI_MISMATCH;
    auto* device = static_cast<ID3D11Device*>(device_pointer);
    auto* context = static_cast<ID3D11DeviceContext*>(context_pointer);
    auto* resource = static_cast<ID3D11Resource*>(texture_pointer);
    D3D11_RESOURCE_DIMENSION dimension{}; resource->GetType(&dimension);
    if (dimension != D3D11_RESOURCE_DIMENSION_TEXTURE2D ||
        context->GetType() != D3D11_DEVICE_CONTEXT_IMMEDIATE) return RSF_TEXTURE_ERROR_INVALID_ARGUMENT;
    ComPtr<ID3D11Device> texture_owner, context_owner;
    resource->GetDevice(&texture_owner); context->GetDevice(&context_owner);
    if (texture_owner.Get() != device || context_owner.Get() != device)
        return RSF_TEXTURE_ERROR_FOREIGN_DEVICE;
    auto* texture = static_cast<ID3D11Texture2D*>(texture_pointer);
    D3D11_TEXTURE2D_DESC desc{}; texture->GetDesc(&desc);
    const uint32_t stride = pixel_bytes(desc.Format);
    const uint64_t row_bytes = uint64_t(desc.Width) * stride;
    if (!stride || desc.SampleDesc.Count != 1 || !row_bytes || !desc.Height ||
        row_bytes * desc.Height > 256ull * 1024 * 1024) return RSF_TEXTURE_ERROR_UNSUPPORTED_FORMAT;
    D3D11_TEXTURE2D_DESC staging = desc;
    staging.Usage = D3D11_USAGE_STAGING; staging.BindFlags = 0;
    staging.CPUAccessFlags = D3D11_CPU_ACCESS_READ; staging.MiscFlags = 0;
    staging.MipLevels = 1; staging.ArraySize = 1;
    ComPtr<ID3D11Texture2D> copy;
    if (FAILED(device->CreateTexture2D(&staging, nullptr, &copy))) return RSF_TEXTURE_ERROR_STAGING_FAILED;
    if (options->log) options->log(options->log_user, "raw dump: copying subresource zero and mapping");
    context->CopySubresourceRegion(copy.Get(), 0, 0, 0, 0, texture, 0, nullptr);
    D3D11_MAPPED_SUBRESOURCE data{};
    Mapping mapping{context, copy.Get()};
    if (FAILED(context->Map(copy.Get(), 0, D3D11_MAP_READ, 0, &data))) return RSF_TEXTURE_ERROR_MAP_FAILED;
    mapping.mapped = true;
    if (data.RowPitch < row_bytes) return RSF_TEXTURE_ERROR_MAP_FAILED;
    const std::string prefix = options->output_prefix_utf8;
    File binary{std::fopen((prefix + ".bin").c_str(), "wb")};
    if (!binary.value) return RSF_TEXTURE_ERROR_WRITE_FAILED;
    // Pack rows without driver padding. Metadata records both the stored stride and Map RowPitch
    // so a reader can interpret the binary independently of the readback device.
    for (uint32_t row = 0; row < desc.Height; ++row) {
        const auto* source = static_cast<const unsigned char*>(data.pData) + size_t(row) * data.RowPitch;
        if (std::fwrite(source, 1, size_t(row_bytes), binary.value) != row_bytes)
            return RSF_TEXTURE_ERROR_WRITE_FAILED;
    }
    if (!binary.close()) return RSF_TEXTURE_ERROR_WRITE_FAILED;
    File metadata{std::fopen((prefix + "_raw.json").c_str(), "wb")};
    if (!metadata.value) return RSF_TEXTURE_ERROR_WRITE_FAILED;
    const int written = std::fprintf(metadata.value,
        "{\"width\":%u,\"height\":%u,\"format\":%u,\"row_pitch\":%llu,"
        "\"source_row_pitch\":%u,\"subresource\":0,\"bytes\":%llu}\n",
        desc.Width, desc.Height, uint32_t(desc.Format), static_cast<unsigned long long>(row_bytes),
        data.RowPitch, static_cast<unsigned long long>(row_bytes * desc.Height));
    return written > 0 && metadata.close() ? RSF_TEXTURE_OK : RSF_TEXTURE_ERROR_WRITE_FAILED;
}
// Allocation/string failures must not unwind through the C ABI; map them to diagnostic failure.
catch (...) { return RSF_TEXTURE_ERROR_WRITE_FAILED; }
