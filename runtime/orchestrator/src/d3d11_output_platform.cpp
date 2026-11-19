// SPDX-License-Identifier: MIT
#include "d3d11_output_platform.h"
#include <d3d11.h>
#include <rescaleframe/srv_format.h>

static_assert(RSF_D3D11_FORMAT_RGBA32_FLOAT == DXGI_FORMAT_R32G32B32A32_FLOAT);
static_assert(RSF_D3D11_FORMAT_RGBA16_FLOAT == DXGI_FORMAT_R16G16B16A16_FLOAT);
static_assert(RSF_D3D11_FORMAT_R11G11B10_FLOAT == DXGI_FORMAT_R11G11B10_FLOAT);
static_assert(RSF_D3D11_USAGE_DEFAULT == D3D11_USAGE_DEFAULT);
static_assert(RSF_D3D11_USAGE_IMMUTABLE == D3D11_USAGE_IMMUTABLE);
static_assert(RSF_D3D11_BIND_SHADER_RESOURCE == D3D11_BIND_SHADER_RESOURCE);
static_assert(RSF_D3D11_BIND_RENDER_TARGET == D3D11_BIND_RENDER_TARGET);

extern "C" void* rsf_d3d11_output_query_texture(void* resource)
{
    ID3D11Texture2D* result = nullptr;
    if (FAILED(static_cast<ID3D11Resource*>(resource)->QueryInterface(IID_PPV_ARGS(&result)))) return nullptr;
    return result;
}

extern "C" void* rsf_d3d11_output_child_device(void* child)
{
    ID3D11Device* result = nullptr;
    static_cast<ID3D11DeviceChild*>(child)->GetDevice(&result);
    return result;
}

extern "C" void rsf_d3d11_output_texture_desc(void* texture, rsf_d3d11_texture_desc* desc)
{
    D3D11_TEXTURE2D_DESC native{};
    static_cast<ID3D11Texture2D*>(texture)->GetDesc(&native);
    *desc = {native.Width, native.Height, native.MipLevels, native.ArraySize, uint32_t(native.Format),
        native.SampleDesc.Count, native.SampleDesc.Quality, uint32_t(native.Usage), native.BindFlags,
        native.CPUAccessFlags, native.MiscFlags};
}

extern "C" void* rsf_d3d11_output_create_texture(void* device, const rsf_d3d11_texture_desc* desc)
{
    const D3D11_TEXTURE2D_DESC native{desc->width, desc->height, desc->mips, desc->layers,
        static_cast<DXGI_FORMAT>(desc->format), {desc->samples, desc->sample_quality},
        static_cast<D3D11_USAGE>(desc->usage), desc->bind_flags, desc->cpu_access_flags, desc->misc_flags};
    ID3D11Texture2D* result = nullptr;
    if (FAILED(static_cast<ID3D11Device*>(device)->CreateTexture2D(&native, nullptr, &result))) return nullptr;
    return result;
}

extern "C" void* rsf_d3d11_output_create_srv(void* device, void* texture, uint32_t format)
{
    D3D11_SHADER_RESOURCE_VIEW_DESC desc{};
    desc.Format = static_cast<DXGI_FORMAT>(format);
    desc.ViewDimension = D3D11_SRV_DIMENSION_TEXTURE2D;
    desc.Texture2D.MipLevels = 1;
    ID3D11ShaderResourceView* result = nullptr;
    if (FAILED(static_cast<ID3D11Device*>(device)->CreateShaderResourceView(
        static_cast<ID3D11Texture2D*>(texture), &desc, &result))) return nullptr;
    return result;
}

extern "C" void* rsf_d3d11_output_create_rtv(void* device, void* texture)
{
    ID3D11RenderTargetView* result = nullptr;
    if (FAILED(static_cast<ID3D11Device*>(device)->CreateRenderTargetView(
        static_cast<ID3D11Texture2D*>(texture), nullptr, &result))) return nullptr;
    return result;
}

extern "C" uint32_t rsf_d3d11_output_srv_format(uint32_t format)
{
    return uint32_t(rsf::srv_format(static_cast<DXGI_FORMAT>(format)));
}
extern "C" uint32_t rsf_d3d11_output_device_ok(void* device)
{
    return SUCCEEDED(static_cast<ID3D11Device*>(device)->GetDeviceRemovedReason());
}

extern "C" uint32_t rsf_d3d11_output_is_immediate(void* context)
{
    return static_cast<ID3D11DeviceContext*>(context)->GetType() == D3D11_DEVICE_CONTEXT_IMMEDIATE;
}

extern "C" void rsf_d3d11_output_clear_state(void* context)
{
    static_cast<ID3D11DeviceContext*>(context)->ClearState();
}

extern "C" void rsf_d3d11_output_unbind_targets(void* context)
{
    static_cast<ID3D11DeviceContext*>(context)->OMSetRenderTargets(0, nullptr, nullptr);
}

extern "C" void rsf_d3d11_output_copy_resource(void* context, void* destination, void* source)
{
    static_cast<ID3D11DeviceContext*>(context)->CopyResource(
        static_cast<ID3D11Texture2D*>(destination), static_cast<ID3D11Texture2D*>(source));
}

extern "C" void rsf_d3d11_output_copy_region(void* context, void* destination, void* source,
    uint32_t left, uint32_t top, uint32_t width, uint32_t height)
{
    const D3D11_BOX box{0, 0, 0, width, height, 1};
    static_cast<ID3D11DeviceContext*>(context)->CopySubresourceRegion(
        static_cast<ID3D11Texture2D*>(destination), 0, left, top, 0,
        static_cast<ID3D11Texture2D*>(source), 0, &box);
}
