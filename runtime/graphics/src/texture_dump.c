// SPDX-License-Identifier: MIT

#include "texture_dump_platform.h"

#include <dxgiformat.h>
#include <stddef.h>

#define RSF_ASSERT(condition) _Static_assert(condition, #condition)

RSF_ASSERT(sizeof(rsf_texture_dump_options) == 40 && sizeof(rsf_dump_pixels) == 40 &&
    sizeof(rsf_texture_dump_report) == 40);
RSF_ASSERT(offsetof(rsf_dump_pixels, row_pitch) == 16 && offsetof(rsf_dump_pixels, data_size) == 24 && offsetof(rsf_dump_pixels, data) == 32);
RSF_ASSERT(sizeof(rsf_texture_dump_motion_options) == 64 && offsetof(rsf_texture_dump_motion_options, motion_scale) == 40);
RSF_ASSERT(RSF_TEXTURE_DUMP_ABI_VERSION == 2 &&
    RSF_TEXTURE_OK == 0 && RSF_TEXTURE_ERROR_INVALID_ARGUMENT == -1 &&
    RSF_TEXTURE_ERROR_ABI_MISMATCH == -2 && RSF_TEXTURE_ERROR_UNSUPPORTED_FORMAT == -3 &&
    RSF_TEXTURE_ERROR_STAGING_FAILED == -4 && RSF_TEXTURE_ERROR_MAP_FAILED == -5 &&
    RSF_TEXTURE_ERROR_WRITE_FAILED == -6 && RSF_TEXTURE_ERROR_FOREIGN_DEVICE == -7);
RSF_ASSERT(RSF_TEXTURE_OUTPUT_TARGA == 0 && RSF_TEXTURE_OUTPUT_PNG == 1 && RSF_TEXTURE_OUTPUT_RAW == 2);
RSF_ASSERT(RSF_DUMP_VIEW_VELOCITY == 1 && RSF_DUMP_VIEW_DECODED_MOTION == 2 &&
    RSF_DUMP_VIEW_HDR == 3 && RSF_DUMP_VIEW_DEPTH == 4 && RSF_DUMP_VIEW_SCALAR == 5 &&
    RSF_DUMP_VIEW_ALPHA == 6 && RSF_DUMP_VIEW_DENSE_MOTION == 7 &&
    RSF_DUMP_DECODED_SENTINEL_VALUE == -100);
RSF_ASSERT(DXGI_FORMAT_R8_TYPELESS == 60 && DXGI_FORMAT_R8_UNORM == 61 &&
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

#undef RSF_ASSERT

rsf_dump_texture_result rsf_dump_texture(void* device, void* context, void* texture,
    const rsf_texture_dump_options* options, rsf_texture_dump_report* report)
{
    return rsf_core_texture_dump_texture(device, context, texture, options, report, RSF_TEXTURE_OUTPUT_TARGA);
}

rsf_dump_texture_result rsf_dump_texture_png(void* device, void* context, void* texture,
    const rsf_texture_dump_options* options, rsf_texture_dump_report* report)
{
    return rsf_core_texture_dump_texture(device, context, texture, options, report, RSF_TEXTURE_OUTPUT_PNG);
}

rsf_dump_texture_result rsf_dump_texture_bytes(void* device, void* context, void* texture,
    const rsf_texture_dump_options* options)
{
    return rsf_core_texture_dump_texture(device, context, texture, options, NULL, RSF_TEXTURE_OUTPUT_RAW);
}

rsf_dump_texture_result rsf_dump_pixels_png(const rsf_dump_pixels* pixels,
    const rsf_texture_dump_options* options, rsf_texture_dump_report* report)
{
    return rsf_core_texture_dump_pixels(pixels, options, report);
}
