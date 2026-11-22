// SPDX-License-Identifier: MIT
//
// The shared D3D11 frame tap's native side: the typed vtable detours and the D3D11 calls the Rust
// owner (runtime/orchestrator/src/frame_tap.rs) makes through `platform`. Hook admission and
// retirement, plan and callback state, gates, substitution, classification, capture routing and
// the references the tap holds are Rust's.

#include "frame_tap_platform.h"
#include "vtable_patch.h"

#include <rescaleframe/frame_tap.h>
#include <rescaleframe/resource_roles.h>

#include <windows.h>

#include <d3d11.h>
#include <d3d11_1.h>

#include <algorithm>
#include <cstring>
#include <iterator>

namespace {

using ps_set_shader_resources_fn = void(STDMETHODCALLTYPE*)(ID3D11DeviceContext*, UINT, UINT,
                                                            ID3D11ShaderResourceView* const*);
using set_constant_buffers_fn = void(STDMETHODCALLTYPE*)(ID3D11DeviceContext*, UINT, UINT,
                                                         ID3D11Buffer* const*);
using ps_set_samplers_fn = void(STDMETHODCALLTYPE*)(ID3D11DeviceContext*, UINT, UINT,
                                                    ID3D11SamplerState* const*);
using draw_fn = void(STDMETHODCALLTYPE*)(ID3D11DeviceContext*, UINT, UINT);
using draw_indexed_fn = void(STDMETHODCALLTYPE*)(ID3D11DeviceContext*, UINT, UINT, INT);
using draw_instanced_fn = void(STDMETHODCALLTYPE*)(ID3D11DeviceContext*, UINT, UINT, UINT, UINT);
using draw_indexed_instanced_fn = void(STDMETHODCALLTYPE*)(ID3D11DeviceContext*, UINT, UINT, UINT,
                                                           INT, UINT);
using om_set_render_targets_fn = void(STDMETHODCALLTYPE*)(ID3D11DeviceContext*, UINT,
                                                          ID3D11RenderTargetView* const*,
                                                          ID3D11DepthStencilView*);
using om_set_render_targets_and_uavs_fn = void(STDMETHODCALLTYPE*)(
    ID3D11DeviceContext*, UINT, ID3D11RenderTargetView* const*, ID3D11DepthStencilView*, UINT, UINT,
    ID3D11UnorderedAccessView* const*, const UINT*);
using rs_set_viewports_fn = void(STDMETHODCALLTYPE*)(ID3D11DeviceContext*, UINT,
                                                     const D3D11_VIEWPORT*);
using rs_set_scissor_rects_fn = void(STDMETHODCALLTYPE*)(ID3D11DeviceContext*, UINT,
                                                         const D3D11_RECT*);
constexpr UINT view_slots = D3D11_COMMONSHADER_INPUT_RESOURCE_SLOT_COUNT;
constexpr UINT sampler_slots = D3D11_COMMONSHADER_SAMPLER_SLOT_COUNT;
constexpr UINT constant_slots = D3D11_COMMONSHADER_CONSTANT_BUFFER_API_SLOT_COUNT;

static_assert(view_slots == 128 && sampler_slots == 16 && constant_slots == 14);
static_assert(D3D11_VIEWPORT_AND_SCISSORRECT_OBJECT_COUNT_PER_PIPELINE == 16 &&
              D3D11_SIMULTANEOUS_RENDER_TARGET_COUNT == 8);
static_assert(sizeof(rsf_frame_tap_viewport) == sizeof(D3D11_VIEWPORT) &&
              sizeof(rsf_frame_tap_rect) == sizeof(D3D11_RECT));
static_assert(D3D11_KEEP_RENDER_TARGETS_AND_DEPTH_STENCIL == 0xffffffffu &&
              D3D11_MAP_WRITE_DISCARD == 4 && D3D11_BIND_CONSTANT_BUFFER == 4);
static_assert(RSF_FRAME_TAP_OK == 0 && RSF_FRAME_TAP_ERROR_INVALID_ARGUMENT == -1 &&
              RSF_FRAME_TAP_ERROR_ABI_MISMATCH == -2 && RSF_FRAME_TAP_ERROR_ALREADY_INSTALLED == -3 &&
              RSF_FRAME_TAP_ERROR_NOT_INSTALLED == -4 && RSF_FRAME_TAP_ERROR_PATCH_FAILED == -5);
static_assert(RSF_FRAME_TAP_LEAVE == 0 && RSF_FRAME_TAP_DIVERT_PATCH_ALPHA == 2);
static_assert(RSF_ROLE_SCENE_COLOR == 1 && RSF_ROLE_DEPTH == 2 && RSF_ROLE_MOTION == 3 &&
              RSF_ROLE_EXPOSURE == 4 && RSF_ROLE_HISTORY == 5);
static_assert(sizeof(rsf_frame_tap_platform) == 32 * sizeof(void*));

ID3D11DeviceContext* context_of(void* context)
{
    return static_cast<ID3D11DeviceContext*>(context);
}

// The texture behind a view, with a reference the caller owns. Shader resource, render target and
// depth stencil views share ID3D11View, which is where GetResource lives.
void* texture_behind(void* view) noexcept
{
    ID3D11Resource* resource = nullptr;
    static_cast<ID3D11View*>(view)->GetResource(&resource);
    if (!resource) {
        return nullptr;
    }
    ID3D11Texture2D* texture = nullptr;
    resource->QueryInterface(__uuidof(ID3D11Texture2D), reinterpret_cast<void**>(&texture));
    resource->Release();
    return texture;
}

void drop(IUnknown* object)
{
    if (object) {
        object->Release();
    }
}

// The game's blend with its alpha operations replaced by the over operator on coverage,
// a1 + a2(1 - a1). Colour factors and everything else about the draw stay the game's.
void* patch_blend(void* context, void* blend, rsf_frame_tap_blend_log* log) noexcept
{
    auto* original = static_cast<ID3D11BlendState*>(blend);
    D3D11_BLEND_DESC description{};
    if (original) {
        original->GetDesc(&description);
    } else {
        // No blend state bound is D3D11's default, spelled out so it is patched like any other.
        description.RenderTarget[0].BlendEnable = FALSE;
        description.RenderTarget[0].RenderTargetWriteMask = D3D11_COLOR_WRITE_ENABLE_ALL;
    }
    const D3D11_BLEND_DESC before = original ? description : D3D11_BLEND_DESC{};
    const UINT targets = description.IndependentBlendEnable ? 8u : 1u;
    for (UINT index = 0; index < targets; ++index) {
        D3D11_RENDER_TARGET_BLEND_DESC& target = description.RenderTarget[index];
        // Alpha has to be writable before any alpha operation means anything. A colour-only blend
        // is ordinary for an interface drawn into a target whose alpha nobody reads, and a layer
        // with colour and no coverage composites to nothing.
        target.RenderTargetWriteMask |= D3D11_COLOR_WRITE_ENABLE_ALPHA;
        if (!target.BlendEnable) {
            // An opaque draw already writes alpha 1 where it covers.
            continue;
        }
        target.SrcBlendAlpha = D3D11_BLEND_ONE;
        target.DestBlendAlpha = D3D11_BLEND_INV_SRC_ALPHA;
        target.BlendOpAlpha = D3D11_BLEND_OP_ADD;
    }

    ID3D11Device* device = nullptr;
    context_of(context)->GetDevice(&device);
    if (!device) {
        return nullptr;
    }
    ID3D11BlendState* patched = nullptr;
    const HRESULT made = device->CreateBlendState(&description, &patched);
    device->Release();
    if (FAILED(made) || !patched) {
        return nullptr;
    }
    const D3D11_RENDER_TARGET_BLEND_DESC& first = before.RenderTarget[0];
    *log = rsf_frame_tap_blend_log{uint32_t(first.BlendEnable),
                                   uint32_t(first.SrcBlend),
                                   uint32_t(first.DestBlend),
                                   uint32_t(first.BlendOp),
                                   uint32_t(first.SrcBlendAlpha),
                                   uint32_t(first.DestBlendAlpha),
                                   uint32_t(first.BlendOpAlpha),
                                   uint32_t(first.RenderTargetWriteMask),
                                   uint32_t(description.RenderTarget[0].RenderTargetWriteMask)};
    return patched;
}

void save_output(void* context, rsf_frame_tap_output* output) noexcept
{
    ID3D11DeviceContext* c = context_of(context);
    ID3D11RenderTargetView* targets[D3D11_SIMULTANEOUS_RENDER_TARGET_COUNT]{};
    ID3D11DepthStencilView* depth = nullptr;
    c->OMGetRenderTargets(D3D11_SIMULTANEOUS_RENDER_TARGET_COUNT, targets, &depth);
    std::copy(std::begin(targets), std::end(targets), output->targets);
    output->depth_view = depth;
    ID3D11BlendState* blend = nullptr;
    c->OMGetBlendState(&blend, output->blend_factor, &output->blend_mask);
    output->blend = blend;
    UINT viewports = D3D11_VIEWPORT_AND_SCISSORRECT_OBJECT_COUNT_PER_PIPELINE;
    c->RSGetViewports(&viewports, reinterpret_cast<D3D11_VIEWPORT*>(output->viewports));
    output->viewport_count = viewports;
    UINT scissors = D3D11_VIEWPORT_AND_SCISSORRECT_OBJECT_COUNT_PER_PIPELINE;
    c->RSGetScissorRects(&scissors, reinterpret_cast<D3D11_RECT*>(output->scissors));
    output->scissor_count = scissors;
}

// Getters release their references immediately: the context owns the live bindings.
void seed_geometry(void* context, rsf_frame_tap_geometry* geometry, void** pixel_constants) noexcept
{
    ID3D11DeviceContext* c = context_of(context);
    ID3D11Buffer* buffers[D3D11_IA_VERTEX_INPUT_RESOURCE_SLOT_COUNT]{};
    c->IAGetVertexBuffers(0, D3D11_IA_VERTEX_INPUT_RESOURCE_SLOT_COUNT, buffers, geometry->strides,
                          geometry->offsets);
    for (UINT index = 0; index < D3D11_IA_VERTEX_INPUT_RESOURCE_SLOT_COUNT; ++index) {
        geometry->vertex_buffers[index] = buffers[index];
        drop(buffers[index]);
    }
    ID3D11Buffer* index_buffer = nullptr;
    DXGI_FORMAT format{};
    c->IAGetIndexBuffer(&index_buffer, &format, &geometry->index_offset);
    geometry->index_buffer = index_buffer;
    geometry->index_format = uint32_t(format);
    drop(index_buffer);
    ID3D11InputLayout* layout = nullptr;
    c->IAGetInputLayout(&layout);
    geometry->input_layout = layout;
    drop(layout);
    D3D11_PRIMITIVE_TOPOLOGY topology{};
    c->IAGetPrimitiveTopology(&topology);
    geometry->topology = uint32_t(topology);
    ID3D11VertexShader* shader = nullptr;
    c->VSGetShader(&shader, nullptr, nullptr);
    geometry->vertex_shader = shader;
    drop(shader);
    c->VSGetConstantBuffers(0, constant_slots, buffers);
    for (UINT index = 0; index < constant_slots; ++index) {
        geometry->vertex_constants[index] = buffers[index];
        drop(buffers[index]);
    }
    c->PSGetConstantBuffers(0, constant_slots, buffers);
    for (UINT index = 0; index < constant_slots; ++index) {
        pixel_constants[index] = buffers[index];
        drop(buffers[index]);
    }
}

void draw(void* original, void* context, const rsf_frame_tap_draw_call* call) noexcept
{
    ID3D11DeviceContext* c = context_of(context);
    switch (call->kind) {
    case 0:
        reinterpret_cast<draw_fn>(original)(c, call->count, call->start);
        break;
    case 1:
        reinterpret_cast<draw_indexed_fn>(original)(c, call->count, call->start, call->base_vertex);
        break;
    case 2:
        reinterpret_cast<draw_instanced_fn>(original)(c, call->count, call->instances, call->start,
                                                      call->first_instance);
        break;
    case 3:
        reinterpret_cast<draw_indexed_instanced_fn>(original)(
            c, call->count, call->instances, call->start, call->base_vertex, call->first_instance);
        break;
    default:
        break;
    }
}

// Only filters that blend between mips with a mip range are biased, as Unreal 4.27 biases only
// the world texture group's samplers; comparison, min/max and point-mip samplers pass through.
void* clone_sampler(void* context, void* sampler, float bias) noexcept
{
    D3D11_SAMPLER_DESC description{};
    static_cast<ID3D11SamplerState*>(sampler)->GetDesc(&description);
    if (!(description.Filter & 0x1) || (description.Filter & 0x180) ||
        !(description.MaxLOD > description.MinLOD)) {
        return nullptr;
    }
    description.MipLODBias = std::clamp(description.MipLODBias + bias, -16.0f, 15.99f);
    ID3D11Device* device = nullptr;
    context_of(context)->GetDevice(&device);
    ID3D11SamplerState* clone = nullptr;
    if (device) {
        device->CreateSamplerState(&description, &clone);
        device->Release();
    }
    return clone;
}

// Keep foreign exceptions on the native side of the Rust owner.
template <class Callback, class... Arguments>
void invoke(Callback callback, Arguments... arguments) noexcept
{
    try {
        callback(arguments...);
    } catch (...) {
    }
}

constexpr rsf_frame_tap_platform platform{
    [](void** vtable, uint32_t slot, void* replacement) noexcept -> int32_t {
        return rsf::patch_slot(vtable, slot, replacement, nullptr) ? 1 : 0;
    },
    [](void* context) noexcept -> int32_t {
        ID3D11DeviceContext1* extended = nullptr;
        if (FAILED(context_of(context)->QueryInterface(__uuidof(ID3D11DeviceContext1),
                                                       reinterpret_cast<void**>(&extended))) ||
            !extended) {
            return 0;
        }
        extended->Release();
        return 1;
    },
    texture_behind,
    [](void* view) noexcept -> void* {
        ID3D11Resource* resource = nullptr;
        static_cast<ID3D11View*>(view)->GetResource(&resource);
        return resource;
    },
    [](void* texture, rsf_texture_facts* facts) noexcept {
        D3D11_TEXTURE2D_DESC description{};
        static_cast<ID3D11Texture2D*>(texture)->GetDesc(&description);
        *facts = rsf_texture_facts{sizeof(rsf_texture_facts),
                                   description.Width,
                                   description.Height,
                                   uint32_t(description.Format),
                                   description.BindFlags,
                                   description.MipLevels,
                                   description.ArraySize,
                                   description.SampleDesc.Count};
    },
    [](void* view) noexcept -> uint32_t {
        D3D11_RENDER_TARGET_VIEW_DESC description{};
        static_cast<ID3D11RenderTargetView*>(view)->GetDesc(&description);
        return uint32_t(description.Format);
    },
    [](void* resource, uint32_t* bind_flags) noexcept -> uint32_t {
        auto* typed = static_cast<ID3D11Resource*>(resource);
        D3D11_RESOURCE_DIMENSION dimension = D3D11_RESOURCE_DIMENSION_UNKNOWN;
        typed->GetType(&dimension);
        if (dimension != D3D11_RESOURCE_DIMENSION_BUFFER) {
            return 0;
        }
        D3D11_BUFFER_DESC description{};
        static_cast<ID3D11Buffer*>(typed)->GetDesc(&description);
        *bind_flags = description.BindFlags;
        return description.ByteWidth;
    },
    [](void* context, void** views) noexcept {
        ID3D11ShaderResourceView* bound[view_slots]{};
        context_of(context)->PSGetShaderResources(0, view_slots, bound);
        std::copy(std::begin(bound), std::end(bound), views);
    },
    [](void* original, void* context, uint32_t start, uint32_t count, void* const* views) noexcept {
        reinterpret_cast<ps_set_shader_resources_fn>(original)(
            context_of(context), start, count,
            reinterpret_cast<ID3D11ShaderResourceView* const*>(views));
    },
    [](void* original, void* context, uint32_t count, void* const* views, void* depth,
       const rsf_frame_tap_uavs* uavs) noexcept {
        auto* targets = reinterpret_cast<ID3D11RenderTargetView* const*>(views);
        auto* depth_view = static_cast<ID3D11DepthStencilView*>(depth);
        if (uavs) {
            reinterpret_cast<om_set_render_targets_and_uavs_fn>(original)(
                context_of(context), count, targets, depth_view, uavs->start, uavs->count,
                reinterpret_cast<ID3D11UnorderedAccessView* const*>(uavs->views),
                uavs->initial_counts);
        } else {
            reinterpret_cast<om_set_render_targets_fn>(original)(context_of(context), count,
                                                                 targets, depth_view);
        }
    },
    [](void* original, void* context, uint32_t count,
       const rsf_frame_tap_viewport* viewports) noexcept {
        auto* typed = reinterpret_cast<const D3D11_VIEWPORT*>(viewports);
        if (original) {
            reinterpret_cast<rs_set_viewports_fn>(original)(context_of(context), count, typed);
        } else {
            context_of(context)->RSSetViewports(count, typed);
        }
    },
    [](void* original, void* context, uint32_t count,
       const rsf_frame_tap_rect* rectangles) noexcept {
        auto* typed = reinterpret_cast<const D3D11_RECT*>(rectangles);
        if (original) {
            reinterpret_cast<rs_set_scissor_rects_fn>(original)(context_of(context), count, typed);
        } else {
            context_of(context)->RSSetScissorRects(count, typed);
        }
    },
    [](void* context, rsf_frame_tap_viewport* viewport) noexcept -> uint32_t {
        UINT count = 1;
        context_of(context)->RSGetViewports(&count, reinterpret_cast<D3D11_VIEWPORT*>(viewport));
        return count;
    },
    [](void* context, void* blend, const float* factor, uint32_t mask) noexcept {
        context_of(context)->OMSetBlendState(static_cast<ID3D11BlendState*>(blend), factor, mask);
    },
    save_output,
    patch_blend,
    [](void* context, void** geometry, void** hull, void** domain) noexcept {
        ID3D11DeviceContext* c = context_of(context);
        ID3D11Buffer* buffers[constant_slots]{};
        const auto copy = [&](void** target) {
            for (UINT index = 0; index < constant_slots; ++index) {
                target[index] = buffers[index];
                drop(buffers[index]);
            }
        };
        c->GSGetConstantBuffers(0, constant_slots, buffers);
        copy(geometry);
        c->HSGetConstantBuffers(0, constant_slots, buffers);
        copy(hull);
        c->DSGetConstantBuffers(0, constant_slots, buffers);
        copy(domain);
    },
    [](void* original, void* context, uint32_t stage, uint32_t slot, void* buffer) noexcept {
        ID3D11DeviceContext* c = context_of(context);
        auto* typed = static_cast<ID3D11Buffer*>(buffer);
        switch (stage) {
        case 0:
        case 1:
            reinterpret_cast<set_constant_buffers_fn>(original)(c, slot, 1, &typed);
            break;
        case 2:
            c->GSSetConstantBuffers(slot, 1, &typed);
            break;
        case 3:
            c->HSSetConstantBuffers(slot, 1, &typed);
            break;
        case 4:
            c->DSSetConstantBuffers(slot, 1, &typed);
            break;
        default:
            break;
        }
    },
    seed_geometry,
    draw,
    [](void* context, void** samplers) noexcept {
        ID3D11SamplerState* bound[sampler_slots]{};
        context_of(context)->PSGetSamplers(0, sampler_slots, bound);
        std::copy(std::begin(bound), std::end(bound), samplers);
    },
    [](void* original, void* context, uint32_t start, uint32_t count,
       void* const* samplers) noexcept {
        reinterpret_cast<ps_set_samplers_fn>(original)(
            context_of(context), start, count,
            reinterpret_cast<ID3D11SamplerState* const*>(samplers));
    },
    clone_sampler,
    invoke<rsf_frame_tap_log_fn, void*, const char*>,
    invoke<rsf_frame_tap_fn, void*, const rsf_frame_tap_pass*>,
    invoke<rsf_frame_tap_target_fn, void*, const rsf_frame_tap_target_draw*>,
    invoke<rsf_frame_tap_geometry_fn, void*, const rsf_frame_tap_geometry*>,
    invoke<rsf_frame_tap_compute_fn, void*, void*, uint32_t, uint32_t, uint32_t, void*, uint32_t>,
    invoke<rsf_frame_tap_constants_fn, void*, void*, void*, uint32_t>,
    [](rsf_frame_tap_gate_fn fn, void* user, void* context, void* texture) noexcept -> int32_t {
        try {
            return fn(user, context, texture);
        } catch (...) {
            return 0;
        }
    },
    [](rsf_frame_tap_verdict_fn fn, void* user,
       const rsf_frame_tap_target_draw* facts) noexcept -> rsf_frame_tap_verdict {
        try {
            return fn(user, facts);
        } catch (...) {
            return RSF_FRAME_TAP_LEAVE;
        }
    },
    [](rsf_frame_tap_constant_override_fn fn, void* user, const rsf_frame_tap_target_draw* facts,
       uint32_t* slots, void** buffers) noexcept -> int32_t {
        try {
            return fn(user, facts, slots, buffers);
        } catch (...) {
            return 0;
        }
    },
};

} // namespace

extern "C" const rsf_frame_tap_platform* rsf_frame_tap_native(void)
{
    return &platform;
}
