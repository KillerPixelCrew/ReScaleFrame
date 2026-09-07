/* SPDX-License-Identifier: GPL-3.0-only */
#include <rescaleframe/ui_layer.h>

#include <windows.h>

#include <d3d11.h>

#include <cstdarg>
#include <cstdio>
#include <new>

struct rsf_ui_layer {
    ID3D11Device* device = nullptr;
    struct Slot {
        ID3D11Texture2D* texture = nullptr;
        ID3D11RenderTargetView* target = nullptr;
        ID3D11ShaderResourceView* source = nullptr;
    };
    Slot slots[RSF_UI_LAYER_RING];
    uint32_t width = 0;
    uint32_t height = 0;
    uint32_t shareable = 0;
    uint32_t slot = 0;
    bool started = false;
    bool written = false;
    uint64_t frames_begun = 0;
    uint64_t frames_written = 0;
    uint64_t clears = 0;
    rsf_ui_layer_log_fn log = nullptr;
    void* log_user = nullptr;
};

namespace {

void say(const rsf_ui_layer* layer, const char* format, ...)
{
    char message[512];
    va_list arguments;
    if (!layer || !layer->log) {
        return;
    }
    va_start(arguments, format);
    std::vsnprintf(message, sizeof(message), format, arguments);
    va_end(arguments);
    layer->log(layer->log_user, message);
}

} // namespace

extern "C" rsf_ui_layer_result rsf_ui_layer_create(void* device_pointer,
                                                   const rsf_ui_layer_setup* setup,
                                                   rsf_ui_layer** out)
{
    if (!device_pointer || !setup || !out || setup->struct_size < sizeof(rsf_ui_layer_setup)) {
        return RSF_UI_LAYER_ERROR_INVALID_ARGUMENT;
    }
    if (setup->abi_version != RSF_UI_LAYER_ABI_VERSION) {
        return RSF_UI_LAYER_ERROR_ABI_MISMATCH;
    }
    if (setup->width == 0 || setup->height == 0) {
        return RSF_UI_LAYER_ERROR_INVALID_ARGUMENT;
    }
    *out = nullptr;

    auto* device = static_cast<ID3D11Device*>(device_pointer);
    auto* layer = new (std::nothrow) rsf_ui_layer();
    if (!layer) {
        return RSF_UI_LAYER_ERROR_RESOURCE_FAILED;
    }
    layer->device = device;
    layer->device->AddRef();
    layer->width = setup->width;
    layer->height = setup->height;
    layer->shareable = setup->shareable ? 1u : 0u;
    layer->log = setup->log;
    layer->log_user = setup->log_user;

    D3D11_TEXTURE2D_DESC description{};
    description.Width = setup->width;
    description.Height = setup->height;
    description.MipLevels = 1;
    description.ArraySize = 1;
    /* Not the back buffer's R10G10B10A2. Two bits of alpha cannot express partial coverage, and
       partial coverage is the entire content of this surface. Non-sRGB on both sides, so the
       composite happens in the back buffer's own encoding and no conversion creeps in. */
    description.Format = DXGI_FORMAT_R8G8B8A8_UNORM;
    description.SampleDesc.Count = 1;
    description.Usage = D3D11_USAGE_DEFAULT;
    /* Unordered access as well as the obvious two: the alpha accumulation check generates mips over
       a copy of this, and a compute path later wants it too. */
    description.BindFlags =
        D3D11_BIND_RENDER_TARGET | D3D11_BIND_SHADER_RESOURCE | D3D11_BIND_UNORDERED_ACCESS;
    if (layer->shareable) {
        /* Shared NT handles rather than the legacy shared flag: the legacy one cannot be opened by
           D3D12, which is the only reason to share this at all. */
        description.MiscFlags = D3D11_RESOURCE_MISC_SHARED | D3D11_RESOURCE_MISC_SHARED_NTHANDLE;
    }

    for (uint32_t index = 0; index < RSF_UI_LAYER_RING; ++index) {
        rsf_ui_layer::Slot& slot = layer->slots[index];
        if (FAILED(device->CreateTexture2D(&description, nullptr, &slot.texture)) ||
            !slot.texture) {
            /* A sharing flag a driver will not honour is worth one retry without it: a layer that
               composites but cannot be handed to a D3D12 vendor is far better than no layer, and
               the alternative is a run that produces no interface at all. */
            if (layer->shareable) {
                say(layer, "ui layer: shared creation refused, retrying without sharing");
                description.MiscFlags = 0;
                layer->shareable = 0;
                if (FAILED(device->CreateTexture2D(&description, nullptr, &slot.texture)) ||
                    !slot.texture) {
                    rsf_ui_layer_destroy(layer);
                    return RSF_UI_LAYER_ERROR_RESOURCE_FAILED;
                }
            } else {
                rsf_ui_layer_destroy(layer);
                return RSF_UI_LAYER_ERROR_RESOURCE_FAILED;
            }
        }
        if (FAILED(device->CreateRenderTargetView(slot.texture, nullptr, &slot.target)) ||
            FAILED(device->CreateShaderResourceView(slot.texture, nullptr, &slot.source))) {
            rsf_ui_layer_destroy(layer);
            return RSF_UI_LAYER_ERROR_RESOURCE_FAILED;
        }
    }

    /* Cleared at creation as well as per frame, so a layer read before the first begin_frame is
       transparent rather than whatever the allocation happened to contain. */
    ID3D11DeviceContext* context = nullptr;
    device->GetImmediateContext(&context);
    if (context) {
        const FLOAT nothing[4] = {0.0f, 0.0f, 0.0f, 0.0f};
        for (auto& slot : layer->slots) {
            context->ClearRenderTargetView(slot.target, nothing);
            ++layer->clears;
        }
        context->Release();
    }

    say(layer, "ui layer: %ux%u R8G8B8A8_UNORM, %u deep, %s", setup->width, setup->height,
        (unsigned)RSF_UI_LAYER_RING, layer->shareable ? "shareable" : "not shared");
    *out = layer;
    return RSF_UI_LAYER_OK;
}

extern "C" void rsf_ui_layer_destroy(rsf_ui_layer* layer)
{
    if (!layer) {
        return;
    }
    auto drop = [](auto* item) {
        if (item) {
            item->Release();
        }
    };
    for (auto& slot : layer->slots) {
        drop(slot.source);
        drop(slot.target);
        drop(slot.texture);
    }
    drop(layer->device);
    delete layer;
}

extern "C" rsf_ui_layer_result rsf_ui_layer_begin_frame(rsf_ui_layer* layer, void* context_pointer)
{
    if (!layer || !context_pointer) {
        return RSF_UI_LAYER_ERROR_INVALID_ARGUMENT;
    }
    auto* context = static_cast<ID3D11DeviceContext*>(context_pointer);
    /* Advance first, then clear. The slot just finished stays intact for a vendor that holds it
       until the next present, which is what the ring is for. */
    if (layer->started) {
        layer->slot = (layer->slot + 1u) % RSF_UI_LAYER_RING;
    }
    layer->started = true;
    layer->written = false;
    ++layer->frames_begun;

    const FLOAT nothing[4] = {0.0f, 0.0f, 0.0f, 0.0f};
    context->ClearRenderTargetView(layer->slots[layer->slot].target, nothing);
    ++layer->clears;
    return RSF_UI_LAYER_OK;
}

extern "C" void* rsf_ui_layer_target(rsf_ui_layer* layer)
{
    if (!layer || !layer->started) {
        return nullptr;
    }
    return layer->slots[layer->slot].target;
}

extern "C" void* rsf_ui_layer_source(rsf_ui_layer* layer)
{
    if (!layer || !layer->started) {
        return nullptr;
    }
    return layer->slots[layer->slot].source;
}

extern "C" void* rsf_ui_layer_texture(rsf_ui_layer* layer)
{
    if (!layer || !layer->started) {
        return nullptr;
    }
    return layer->slots[layer->slot].texture;
}

extern "C" void rsf_ui_layer_mark_written(rsf_ui_layer* layer)
{
    if (!layer || !layer->started || layer->written) {
        return;
    }
    layer->written = true;
    ++layer->frames_written;
}

extern "C" rsf_ui_layer_result rsf_ui_layer_get_status(const rsf_ui_layer* layer,
                                                       rsf_ui_layer_status* status)
{
    if (!layer || !status || status->struct_size < sizeof(rsf_ui_layer_status)) {
        return RSF_UI_LAYER_ERROR_INVALID_ARGUMENT;
    }
    const uint32_t size = status->struct_size;
    status->width = layer->width;
    status->height = layer->height;
    status->slot = layer->slot;
    status->shareable = layer->shareable;
    status->written_this_frame = layer->written ? 1u : 0u;
    status->frames_begun = layer->frames_begun;
    status->frames_written = layer->frames_written;
    status->clears = layer->clears;
    status->struct_size = size;
    return RSF_UI_LAYER_OK;
}
