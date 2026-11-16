// SPDX-License-Identifier: MIT
#include "native_reinsert.h"
#include <rescaleframe/fullscreen_pass.h>
#include <rescaleframe/d3d11_state.h>
#include <rescaleframe/srv_format.h>
#include <wrl/client.h>
#include <array>

namespace {
using Microsoft::WRL::ComPtr;
bool float_colour(DXGI_FORMAT format)
{
    return format == DXGI_FORMAT_R11G11B10_FLOAT || format == DXGI_FORMAT_R16G16B16A16_FLOAT ||
        format == DXGI_FORMAT_R32G32B32A32_FLOAT;
}
struct Conversion {
    ComPtr<ID3D11Texture2D> texture;
    ComPtr<ID3D11RenderTargetView> target;
    DXGI_FORMAT format{};
    uint32_t width{}, height{};
};
struct Resources {
    ComPtr<ID3D11Device> device;
    ComPtr<ID3D11Texture2D> source;
    ComPtr<ID3D11ShaderResourceView> view;
    std::array<Conversion, 2> conversions;
    rsf_fullscreen_pass* blit{};
    void release() {
        rsf_fullscreen_pass_destroy(blit); blit = nullptr;
        conversions = {}; view.Reset(); source.Reset(); device.Reset();
    }
};
struct Bindings {
    ID3D11DeviceContext* context;
    rsf_d3d11_state saved{};
    bool valid;
    explicit Bindings(ID3D11DeviceContext* value) : context(value), valid(rsf_d3d11_state_save(value, &saved) != 0) {}
    ~Bindings() { if (valid) rsf_d3d11_state_restore(context, &saved); }
};
Resources& resources() { static Resources value; return value; }
}

bool rsf_native_reinsert(ID3D11DeviceContext* context, ID3D11Texture2D* source,
    ID3D11Texture2D* destination, ID3D11Texture2D* readable, const int32_t rect[4]) try
{
    if (!context || !source || !destination || !rect || context->GetType() != D3D11_DEVICE_CONTEXT_IMMEDIATE ||
        rect[0] < 0 || rect[1] < 0 || rect[2] <= rect[0] || rect[3] <= rect[1]) return false;
    D3D11_TEXTURE2D_DESC src{}; source->GetDesc(&src);
    const auto width = uint32_t(rect[2] - rect[0]), height = uint32_t(rect[3] - rect[1]);
    if (src.Width != width || src.Height != height || src.ArraySize != 1 || src.SampleDesc.Count != 1) return false;
    ComPtr<ID3D11Device> device, source_device;
    context->GetDevice(&device); source->GetDevice(&source_device);
    if (device.Get() != source_device.Get()) return false;
    const std::array<ID3D11Texture2D*, 2> destinations{destination, readable == destination ? nullptr : readable};
    std::array<DXGI_FORMAT, 2> formats{};
    bool conversion = false;
    for (size_t i = 0; i < destinations.size(); ++i) {
        if (!destinations[i]) continue;
        ComPtr<ID3D11Device> owner; destinations[i]->GetDevice(&owner);
        D3D11_TEXTURE2D_DESC dst{}; destinations[i]->GetDesc(&dst);
        if (owner.Get() != device.Get() || dst.ArraySize != 1 || dst.SampleDesc.Count != 1 ||
            uint32_t(rect[2]) > dst.Width || uint32_t(rect[3]) > dst.Height || dst.Usage == D3D11_USAGE_IMMUTABLE) return false;
        formats[i] = dst.Format;
        if (dst.Format != src.Format) {
            if (!float_colour(src.Format) || !float_colour(rsf::srv_format(dst.Format)) ||
                !(src.BindFlags & D3D11_BIND_SHADER_RESOURCE)) return false;
            conversion = true;
        }
    }
    auto& cache = resources();
    std::array<ID3D11Texture2D*, 2> copies{source, source};
    Bindings bindings(context);
    if (!bindings.valid) return false;
    // A vendor may leave the reconstruction bound as a UAV, or the engine may have a GS/predicate.
    // Clear bindings only; the enclosing shared scope restores all of them, including UAV counters.
    context->ClearState();
    if (conversion) {
        if (cache.device.Get() != device.Get()) { cache.release(); cache.device = device; }
        if (!cache.blit) {
            const rsf_fullscreen_setup setup{sizeof(setup), RSF_FULLSCREEN_PASS_ABI_VERSION, nullptr, nullptr};
            if (rsf_fullscreen_pass_create(device.Get(), &setup, &cache.blit) != RSF_FULLSCREEN_OK) return false;
        }
        if (cache.source.Get() != source) {
            ComPtr<ID3D11ShaderResourceView> view;
            D3D11_SHADER_RESOURCE_VIEW_DESC desc{}; desc.Format = src.Format;
            desc.ViewDimension = D3D11_SRV_DIMENSION_TEXTURE2D; desc.Texture2D.MipLevels = 1;
            if (FAILED(device->CreateShaderResourceView(source, &desc, &view))) return false;
            cache.source = source; cache.view = view;
        }
        for (size_t i = 0; i < destinations.size(); ++i) {
            if (!destinations[i] || formats[i] == src.Format) continue;
            auto& output = cache.conversions[i];
            const auto format = rsf::srv_format(formats[i]);
            if (!output.texture || output.format != format || output.width != width || output.height != height) {
                Conversion next{}; next.format = format; next.width = width; next.height = height;
                D3D11_TEXTURE2D_DESC desc{}; desc.Width = width; desc.Height = height;
                desc.MipLevels = desc.ArraySize = desc.SampleDesc.Count = 1; desc.Format = format;
                desc.BindFlags = D3D11_BIND_RENDER_TARGET;
                if (FAILED(device->CreateTexture2D(&desc, nullptr, &next.texture)) ||
                    FAILED(device->CreateRenderTargetView(next.texture.Get(), nullptr, &next.target))) return false;
                output = std::move(next);
            }
            copies[i] = output.texture.Get();
        }
        // Render every conversion before touching either engine destination.
        const rsf_fullscreen_draw draw{sizeof(draw), RSF_FULLSCREEN_COPY_RGBA, 1, width, height};
        for (size_t i = 0; i < destinations.size(); ++i) {
            if (destinations[i] && copies[i] != source && rsf_fullscreen_pass_draw(cache.blit, context,
                cache.conversions[i].target.Get(), cache.view.Get(), &draw) != RSF_FULLSCREEN_OK) return false;
        }
    }
    context->OMSetRenderTargets(0, nullptr, nullptr);
    const D3D11_BOX box{0, 0, 0, width, height, 1};
    for (size_t i = 0; i < destinations.size(); ++i) {
        if (destinations[i] && destinations[i] != copies[i])
            context->CopySubresourceRegion(destinations[i], 0, UINT(rect[0]), UINT(rect[1]), 0, copies[i], 0, &box);
    }
    return SUCCEEDED(device->GetDeviceRemovedReason());
}
catch (...) { return false; }

void rsf_native_reinsert_release() { resources().release(); }
