/* SPDX-License-Identifier: MIT */
/* C11 helpers shared by the explicit D3D11 host device launchers. Rust owns the phases and
   verdicts; these launchers only create the hidden window, the device and the swap chain.
   A structured-exception filter on the MSVC C++ exception code 0xE06D7363 stops what the
   former catch-all stopped inside callbacks; other faults propagate. */
#pragma once
#ifndef WIN32_LEAN_AND_MEAN
#define WIN32_LEAN_AND_MEAN
#endif
#ifndef NOMINMAX
#define NOMINMAX
#endif
#define COBJMACROS
#include <windows.h>
#include <d3d11.h>
#include <dxgi1_6.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
#include <rescaleframe/foreign_guard.h>
#include "d3d11_host_device_fixture.h"

#define RSF_FIXTURE_WIDTH 960u
#define RSF_FIXTURE_HEIGHT 540u

/* The argument is decoded with the active code page, as std::filesystem::path does. */
static inline int rsf_fixture_wide_root(const char* argument, wchar_t* root, size_t capacity)
{
    const int length = MultiByteToWideChar(CP_ACP, 0, argument, -1, root, (int)capacity);
    return length > 0;
}

/* Appends name with one separator, like path / name. */
static inline int rsf_fixture_join(wchar_t* out, size_t capacity, const wchar_t* base, const wchar_t* name)
{
    const size_t length = wcslen(base);
    const int separator = length && base[length - 1] != L'\\' && base[length - 1] != L'/';
    return swprintf(out, capacity, separator ? L"%ls\\%ls" : L"%ls%ls", base, name) > 0;
}

static inline int rsf_fixture_exists(const wchar_t* path)
{
    return GetFileAttributesW(path) != INVALID_FILE_ATTRIBUTES;
}

static inline int rsf_fixture_is_directory(const wchar_t* path)
{
    const DWORD attributes = GetFileAttributesW(path);
    return attributes != INVALID_FILE_ATTRIBUTES && (attributes & FILE_ATTRIBUTE_DIRECTORY) != 0;
}

/* One owned texture reference from the Rust fixture, or null. */
static inline ID3D11Texture2D* rsf_fixture_texture(ID3D11Device* device, DXGI_FORMAT format, UINT bindings)
{
    return (ID3D11Texture2D*)rsf_d3d11_host_fixture_texture(device, RSF_FIXTURE_WIDTH, RSF_FIXTURE_HEIGHT,
        (uint32_t)format, bindings);
}

/* First hardware NVIDIA adapter on the factory, or null. One owned reference. */
static inline IDXGIAdapter1* rsf_fixture_nvidia_adapter(IDXGIFactory1* factory)
{
    UINT index;
    for (index = 0;; ++index) {
        IDXGIAdapter1* adapter = NULL;
        DXGI_ADAPTER_DESC1 desc;
        if (FAILED(IDXGIFactory1_EnumAdapters1(factory, index, &adapter))) return NULL;
        memset(&desc, 0, sizeof(desc));
        IDXGIAdapter1_GetDesc1(adapter, &desc);
        if (desc.VendorId == 0x10de && !(desc.Flags & DXGI_ADAPTER_FLAG_SOFTWARE)) {
            printf("Adapter: %ls\n", desc.Description);
            return adapter;
        }
        IDXGIAdapter1_Release(adapter);
    }
}

/* Hardware device and swap chain on the adapter, with the debug layer when it is installed. */
static inline HRESULT rsf_fixture_create_device(IDXGIAdapter1* adapter, DXGI_SWAP_CHAIN_DESC* desc,
    IDXGISwapChain** chain, ID3D11Device** device, ID3D11DeviceContext** context)
{
    const D3D_FEATURE_LEVEL level = D3D_FEATURE_LEVEL_11_1;
    HRESULT created = D3D11CreateDeviceAndSwapChain((IDXGIAdapter*)adapter, D3D_DRIVER_TYPE_UNKNOWN, NULL,
        D3D11_CREATE_DEVICE_DEBUG, &level, 1, D3D11_SDK_VERSION, desc, chain, device, NULL, context);
    if (created == DXGI_ERROR_SDK_COMPONENT_MISSING)
        created = D3D11CreateDeviceAndSwapChain((IDXGIAdapter*)adapter, D3D_DRIVER_TYPE_UNKNOWN, NULL,
            0, &level, 1, D3D11_SDK_VERSION, desc, chain, device, NULL, context);
    return created;
}

/* The four fixture textures and their views. Unset members stay null, so a launcher can
   release the whole set after any partial failure. */
typedef struct rsf_fixture_targets {
    ID3D11Texture2D *color, *motion, *depth, *output;
    ID3D11RenderTargetView *color_view, *motion_view, *output_view;
    ID3D11DepthStencilView* depth_view;
} rsf_fixture_targets;

#define RSF_FIXTURE_RELEASE(Type, pointer) \
    do { if (pointer) { Type##_Release(pointer); (pointer) = NULL; } } while (0)

static inline void rsf_fixture_targets_release(rsf_fixture_targets* t)
{
    RSF_FIXTURE_RELEASE(ID3D11DepthStencilView, t->depth_view);
    RSF_FIXTURE_RELEASE(ID3D11RenderTargetView, t->output_view);
    RSF_FIXTURE_RELEASE(ID3D11RenderTargetView, t->motion_view);
    RSF_FIXTURE_RELEASE(ID3D11RenderTargetView, t->color_view);
    RSF_FIXTURE_RELEASE(ID3D11Texture2D, t->output);
    RSF_FIXTURE_RELEASE(ID3D11Texture2D, t->depth);
    RSF_FIXTURE_RELEASE(ID3D11Texture2D, t->motion);
    RSF_FIXTURE_RELEASE(ID3D11Texture2D, t->color);
}

/* Both return 0 when a texture or a view cannot be created, so the launchers can report the
   two failures separately. The caller releases through rsf_fixture_targets_release. */
static inline int rsf_fixture_allocate_textures(rsf_fixture_targets* t, ID3D11Device* device)
{
    t->color = rsf_fixture_texture(device, DXGI_FORMAT_R11G11B10_FLOAT,
        D3D11_BIND_RENDER_TARGET | D3D11_BIND_SHADER_RESOURCE);
    t->motion = rsf_fixture_texture(device, DXGI_FORMAT_R16G16_FLOAT,
        D3D11_BIND_RENDER_TARGET | D3D11_BIND_SHADER_RESOURCE);
    t->depth = rsf_fixture_texture(device, DXGI_FORMAT_R24G8_TYPELESS,
        D3D11_BIND_DEPTH_STENCIL | D3D11_BIND_SHADER_RESOURCE);
    t->output = rsf_fixture_texture(device, DXGI_FORMAT_R16G16B16A16_FLOAT,
        D3D11_BIND_RENDER_TARGET | D3D11_BIND_SHADER_RESOURCE);
    return t->color && t->motion && t->depth && t->output;
}

static inline int rsf_fixture_create_views(rsf_fixture_targets* t, ID3D11Device* device)
{
    D3D11_DEPTH_STENCIL_VIEW_DESC ds;
    memset(&ds, 0, sizeof(ds));
    ds.Format = DXGI_FORMAT_D24_UNORM_S8_UINT;
    ds.ViewDimension = D3D11_DSV_DIMENSION_TEXTURE2D;
    return !(FAILED(ID3D11Device_CreateRenderTargetView(device, (ID3D11Resource*)t->color, NULL, &t->color_view)) ||
        FAILED(ID3D11Device_CreateRenderTargetView(device, (ID3D11Resource*)t->motion, NULL, &t->motion_view)) ||
        FAILED(ID3D11Device_CreateRenderTargetView(device, (ID3D11Resource*)t->output, NULL, &t->output_view)) ||
        FAILED(ID3D11Device_CreateDepthStencilView(device, (ID3D11Resource*)t->depth, &ds, &t->depth_view)));
}
