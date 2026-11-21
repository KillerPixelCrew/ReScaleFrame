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
#include <tuple>

namespace {

// ID3D11DeviceContext vtable slots, counting the three IUnknown and four ID3D11DeviceChild entries
// first. Same source as the observer's slots, tools/ghidra/build-directx-types.py.
constexpr uint32_t slot_ps_set_shader_resources = 8;
constexpr uint32_t slot_ps_set_samplers = 10;
constexpr uint32_t slot_draw_indexed = 12;
constexpr uint32_t slot_draw = 13;
constexpr uint32_t slot_ps_set_constant_buffers = 16;
constexpr uint32_t slot_om_set_render_targets = 33;
constexpr uint32_t slot_om_set_render_targets_and_uavs = 34;
constexpr uint32_t slot_rs_set_viewports = 44;
constexpr uint32_t slot_rs_set_scissor_rects = 45;
constexpr uint32_t slot_clear_render_target_view = 50;

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
using clear_render_target_view_fn = void(STDMETHODCALLTYPE*)(ID3D11DeviceContext*,
                                                             ID3D11RenderTargetView*,
                                                             const FLOAT[4]);

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
static_assert(sizeof(rsf_frame_tap_platform) == 33 * sizeof(void*));

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
    [](const rsf_role_rule* rules, uint32_t count, const rsf_texture_facts* facts,
       uint32_t output_width, uint32_t output_height) noexcept -> rsf_resource_role {
        const rsf_role_table table{sizeof(rsf_role_table), rules, count};
        rsf_frame_shape shape{};
        shape.struct_size = sizeof(shape);
        shape.output_width = output_width;
        shape.output_height = output_height;
        rsf_role_verdict verdict{};
        verdict.struct_size = sizeof(verdict);
        return rsf_classify_texture(&table, facts, &shape, &verdict) ? verdict.role
                                                                    : RSF_ROLE_UNKNOWN;
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

// The hook's current original, after the vtable has been checked for a runtime rewrite.
template <class Fn>
Fn original(uint32_t hook)
{
    return reinterpret_cast<Fn>(rsf_core_frame_tap_enter(&platform, hook));
}

void leave()
{
    rsf_core_frame_tap_leave(&platform);
}

template <class Target, class Source>
Target as(Source source)
{
    return reinterpret_cast<Target>(source);
}

void STDMETHODCALLTYPE hooked_ps_set_shader_resources(ID3D11DeviceContext* context, UINT start,
                                                      UINT count,
                                                      ID3D11ShaderResourceView* const* views)
{
    rsf_core_frame_tap_views(&platform, context, start, count, as<void* const*>(views));
}

void STDMETHODCALLTYPE hooked_om_set_render_targets(ID3D11DeviceContext* context, UINT count,
                                                    ID3D11RenderTargetView* const* views,
                                                    ID3D11DepthStencilView* depth)
{
    rsf_core_frame_tap_targets(&platform, context, count, as<void* const*>(views), depth, nullptr);
}

// The same binding by another entry point. Unreal's D3D11 backend uses it whenever a pass declares
// an unordered access view, so a shadow that watched only the first would go stale.
void STDMETHODCALLTYPE hooked_om_set_render_targets_and_uavs(
    ID3D11DeviceContext* context, UINT count, ID3D11RenderTargetView* const* views,
    ID3D11DepthStencilView* depth, UINT uav_start, UINT uav_count,
    ID3D11UnorderedAccessView* const* uavs, const UINT* initial_counts)
{
    const rsf_frame_tap_uavs unordered{uav_start, uav_count, as<void* const*>(uavs),
                                       initial_counts};
    rsf_core_frame_tap_targets(&platform, context, count, as<void* const*>(views), depth,
                               &unordered);
}

void STDMETHODCALLTYPE hooked_rs_set_viewports(ID3D11DeviceContext* context, UINT count,
                                               const D3D11_VIEWPORT* viewports)
{
    rsf_core_frame_tap_viewports(&platform, context, count,
                                 as<const rsf_frame_tap_viewport*>(viewports));
}

void STDMETHODCALLTYPE hooked_rs_set_scissor_rects(ID3D11DeviceContext* context, UINT count,
                                                   const D3D11_RECT* rectangles)
{
    rsf_core_frame_tap_scissors(&platform, context, count,
                                as<const rsf_frame_tap_rect*>(rectangles));
}

void STDMETHODCALLTYPE hooked_clear_render_target_view(ID3D11DeviceContext* context,
                                                       ID3D11RenderTargetView* view,
                                                       const FLOAT colour[4])
{
    const auto forward =
        original<clear_render_target_view_fn>(RSF_FRAME_TAP_HOOK_CLEAR_TARGET);
    if (!forward) {
        return;
    }
    forward(context,
            static_cast<ID3D11RenderTargetView*>(
                rsf_core_frame_tap_clear_target(&platform, context, view)),
            colour);
    leave();
}

void STDMETHODCALLTYPE hooked_draw_indexed(ID3D11DeviceContext* context, UINT count, UINT start,
                                           INT base_vertex)
{
    const rsf_frame_tap_draw_call call{1, count, start, 1, 0, base_vertex};
    rsf_core_frame_tap_draw(&platform, context, RSF_FRAME_TAP_HOOK_DRAW_INDEXED, &call);
}

void STDMETHODCALLTYPE hooked_draw(ID3D11DeviceContext* context, UINT count, UINT start)
{
    const rsf_frame_tap_draw_call call{0, count, start, 1, 0, 0};
    rsf_core_frame_tap_draw(&platform, context, RSF_FRAME_TAP_HOOK_DRAW, &call);
}

void STDMETHODCALLTYPE hooked_indexed_instanced(ID3D11DeviceContext* context, UINT count,
                                                UINT instances, UINT start, INT base_vertex,
                                                UINT first_instance)
{
    const rsf_frame_tap_draw_call call{3, count, start, instances, first_instance, base_vertex};
    rsf_core_frame_tap_draw(&platform, context, RSF_FRAME_TAP_HOOK_DRAW_INDEXED_INSTANCED, &call);
}

void STDMETHODCALLTYPE hooked_instanced(ID3D11DeviceContext* context, UINT count, UINT instances,
                                        UINT start, UINT first_instance)
{
    const rsf_frame_tap_draw_call call{2, count, start, instances, first_instance, 0};
    rsf_core_frame_tap_draw(&platform, context, RSF_FRAME_TAP_HOOK_DRAW_INSTANCED, &call);
}

// The draws whose arguments live in a buffer or come from the stream-out count: nothing to look at
// but the fact that one happened. Kind uses rsf_frame_tap_geometry numbering.
template <uint32_t Hook, uint32_t Kind, class... Arguments>
void STDMETHODCALLTYPE indirect_draw(ID3D11DeviceContext* context, Arguments... arguments)
{
    using Fn = void(STDMETHODCALLTYPE*)(ID3D11DeviceContext*, Arguments...);
    original<Fn>(Hook)(context, arguments...);
    if constexpr (sizeof...(Arguments) == 0) {
        rsf_core_frame_tap_indirect(&platform, context, Kind, nullptr, 0);
    } else {
        const auto supplied = std::tuple<Arguments...>(arguments...);
        rsf_core_frame_tap_indirect(&platform, context, Kind, std::get<0>(supplied),
                                    std::get<1>(supplied));
    }
}

void STDMETHODCALLTYPE hooked_vertex_buffers(ID3D11DeviceContext* context, UINT start, UINT count,
                                             ID3D11Buffer* const* buffers, const UINT* strides,
                                             const UINT* offsets)
{
    using Fn = void(STDMETHODCALLTYPE*)(ID3D11DeviceContext*, UINT, UINT, ID3D11Buffer* const*,
                                        const UINT*, const UINT*);
    original<Fn>(RSF_FRAME_TAP_HOOK_VERTEX_BUFFERS)(context, start, count, buffers, strides,
                                                    offsets);
    rsf_core_frame_tap_vertex_buffers(context, start, count, as<void* const*>(buffers), strides,
                                      offsets);
}

void STDMETHODCALLTYPE hooked_index_buffer(ID3D11DeviceContext* context, ID3D11Buffer* buffer,
                                           DXGI_FORMAT format, UINT offset)
{
    using Fn = void(STDMETHODCALLTYPE*)(ID3D11DeviceContext*, ID3D11Buffer*, DXGI_FORMAT, UINT);
    original<Fn>(RSF_FRAME_TAP_HOOK_INDEX_BUFFER)(context, buffer, format, offset);
    rsf_core_frame_tap_index_buffer(context, buffer, uint32_t(format), offset);
}

void STDMETHODCALLTYPE hooked_layout(ID3D11DeviceContext* context, ID3D11InputLayout* layout)
{
    using Fn = void(STDMETHODCALLTYPE*)(ID3D11DeviceContext*, ID3D11InputLayout*);
    original<Fn>(RSF_FRAME_TAP_HOOK_LAYOUT)(context, layout);
    rsf_core_frame_tap_object(context, RSF_FRAME_TAP_OBJECT_LAYOUT, layout);
}

void STDMETHODCALLTYPE hooked_topology(ID3D11DeviceContext* context,
                                       D3D11_PRIMITIVE_TOPOLOGY topology)
{
    using Fn = void(STDMETHODCALLTYPE*)(ID3D11DeviceContext*, D3D11_PRIMITIVE_TOPOLOGY);
    original<Fn>(RSF_FRAME_TAP_HOOK_TOPOLOGY)(context, topology);
    rsf_core_frame_tap_topology(context, uint32_t(topology));
}

void STDMETHODCALLTYPE hooked_vertex_shader(ID3D11DeviceContext* context,
                                            ID3D11VertexShader* shader,
                                            ID3D11ClassInstance* const* classes, UINT count)
{
    using Fn = void(STDMETHODCALLTYPE*)(ID3D11DeviceContext*, ID3D11VertexShader*,
                                        ID3D11ClassInstance* const*, UINT);
    original<Fn>(RSF_FRAME_TAP_HOOK_VERTEX_SHADER)(context, shader, classes, count);
    rsf_core_frame_tap_object(context, RSF_FRAME_TAP_OBJECT_VERTEX_SHADER, shader);
}

// The pixel shader, the blend and the depth stencil state: recorded, not fetched at the draw, where
// a getter would cost a reference count per draw for a value that changes a few dozen times.
void STDMETHODCALLTYPE hooked_pixel_shader(ID3D11DeviceContext* context, ID3D11PixelShader* shader,
                                           ID3D11ClassInstance* const* classes, UINT count)
{
    using Fn = void(STDMETHODCALLTYPE*)(ID3D11DeviceContext*, ID3D11PixelShader*,
                                        ID3D11ClassInstance* const*, UINT);
    original<Fn>(RSF_FRAME_TAP_HOOK_PIXEL_SHADER)(context, shader, classes, count);
    rsf_core_frame_tap_object(context, RSF_FRAME_TAP_OBJECT_PIXEL_SHADER, shader);
}

void STDMETHODCALLTYPE hooked_blend_state(ID3D11DeviceContext* context, ID3D11BlendState* blend,
                                          const FLOAT factor[4], UINT mask)
{
    using Fn = void(STDMETHODCALLTYPE*)(ID3D11DeviceContext*, ID3D11BlendState*, const FLOAT[4],
                                        UINT);
    original<Fn>(RSF_FRAME_TAP_HOOK_BLEND)(context, blend, factor, mask);
    rsf_core_frame_tap_object(context, RSF_FRAME_TAP_OBJECT_BLEND, blend);
}

void STDMETHODCALLTYPE hooked_depth_stencil_state(ID3D11DeviceContext* context,
                                                  ID3D11DepthStencilState* state, UINT reference)
{
    using Fn = void(STDMETHODCALLTYPE*)(ID3D11DeviceContext*, ID3D11DepthStencilState*, UINT);
    original<Fn>(RSF_FRAME_TAP_HOOK_DEPTH_STENCIL)(context, state, reference);
    rsf_core_frame_tap_object(context, RSF_FRAME_TAP_OBJECT_DEPTH_STENCIL, state);
}

void STDMETHODCALLTYPE hooked_vertex_constants(ID3D11DeviceContext* context, UINT start, UINT count,
                                               ID3D11Buffer* const* buffers)
{
    original<set_constant_buffers_fn>(RSF_FRAME_TAP_HOOK_VERTEX_CONSTANTS)(context, start, count,
                                                                         buffers);
    rsf_core_frame_tap_vertex_constants(context, start, count, as<void* const*>(buffers));
}

void STDMETHODCALLTYPE hooked_ps_set_constant_buffers(ID3D11DeviceContext* context, UINT start,
                                                      UINT count, ID3D11Buffer* const* buffers)
{
    const auto forward = original<set_constant_buffers_fn>(RSF_FRAME_TAP_HOOK_PIXEL_CONSTANTS);
    if (!forward) {
        return;
    }
    forward(context, start, count, buffers);
    rsf_core_frame_tap_pixel_constants(&platform, context, start, count, as<void* const*>(buffers));
}

void STDMETHODCALLTYPE hooked_ps_set_samplers(ID3D11DeviceContext* context, UINT start, UINT count,
                                              ID3D11SamplerState* const* samplers)
{
    const auto forward = original<ps_set_samplers_fn>(RSF_FRAME_TAP_HOOK_SAMPLERS);
    if (!forward) {
        return;
    }
    void* bound[sampler_slots];
    forward(context, start, count,
            as<ID3D11SamplerState* const*>(rsf_core_frame_tap_samplers(
                &platform, context, start, count, as<void* const*>(samplers), bound)));
}

void STDMETHODCALLTYPE hooked_clear_state(ID3D11DeviceContext* context)
{
    using Fn = void(STDMETHODCALLTYPE*)(ID3D11DeviceContext*);
    original<Fn>(RSF_FRAME_TAP_HOOK_CLEAR_STATE)(context);
    rsf_core_frame_tap_invalidate(&platform, context, 0);
}

void STDMETHODCALLTYPE hooked_execute(ID3D11DeviceContext* context, ID3D11CommandList* list,
                                      BOOL restore)
{
    using Fn = void(STDMETHODCALLTYPE*)(ID3D11DeviceContext*, ID3D11CommandList*, BOOL);
    original<Fn>(RSF_FRAME_TAP_HOOK_EXECUTE)(context, list, restore);
    rsf_core_frame_tap_invalidate(&platform, context, restore ? 1u : 0u);
}

// Pass-through hooks for the flush-class calls and the rest of the work-submission family. Each
// exists so the vtable rewrite its call causes is noticed on the way out rather than by whatever
// hooked call the game makes next.
template <uint32_t Hook, class... Arguments>
void STDMETHODCALLTYPE pass_through(ID3D11DeviceContext* context, Arguments... arguments)
{
    using Fn = void(STDMETHODCALLTYPE*)(ID3D11DeviceContext*, Arguments...);
    original<Fn>(Hook)(context, arguments...);
    leave();
}

HRESULT STDMETHODCALLTYPE hooked_map(ID3D11DeviceContext* context, ID3D11Resource* resource,
                                     UINT subresource, D3D11_MAP kind, UINT flags,
                                     D3D11_MAPPED_SUBRESOURCE* mapped)
{
    using Fn = HRESULT(STDMETHODCALLTYPE*)(ID3D11DeviceContext*, ID3D11Resource*, UINT, D3D11_MAP,
                                           UINT, D3D11_MAPPED_SUBRESOURCE*);
    const HRESULT result =
        original<Fn>(RSF_FRAME_TAP_HOOK_MAP)(context, resource, subresource, kind, flags, mapped);
    leave();
    rsf_core_frame_tap_mapped(&platform, context, resource, subresource, uint32_t(kind),
                              SUCCEEDED(result) && mapped ? mapped->pData : nullptr);
    return result;
}

void STDMETHODCALLTYPE hooked_unmap(ID3D11DeviceContext* context, ID3D11Resource* resource,
                                    UINT subresource)
{
    using Fn = void(STDMETHODCALLTYPE*)(ID3D11DeviceContext*, ID3D11Resource*, UINT);
    rsf_core_frame_tap_unmapping(&platform, context, resource);
    original<Fn>(RSF_FRAME_TAP_HOOK_UNMAP)(context, resource, subresource);
    leave();
}

void STDMETHODCALLTYPE hooked_dispatch(ID3D11DeviceContext* context, UINT x, UINT y, UINT z)
{
    using Fn = void(STDMETHODCALLTYPE*)(ID3D11DeviceContext*, UINT, UINT, UINT);
    original<Fn>(RSF_FRAME_TAP_HOOK_DISPATCH)(context, x, y, z);
    leave();
    rsf_core_frame_tap_dispatch(&platform, context, x, y, z, nullptr, 0);
}

void STDMETHODCALLTYPE hooked_dispatch_indirect(ID3D11DeviceContext* context,
                                                ID3D11Buffer* arguments, UINT offset)
{
    using Fn = void(STDMETHODCALLTYPE*)(ID3D11DeviceContext*, ID3D11Buffer*, UINT);
    original<Fn>(RSF_FRAME_TAP_HOOK_DISPATCH_INDIRECT)(context, arguments, offset);
    leave();
    rsf_core_frame_tap_dispatch(&platform, context, 0, 0, 0, arguments, offset);
}

void STDMETHODCALLTYPE hooked_copy_resource(ID3D11DeviceContext* context,
                                            ID3D11Resource* destination, ID3D11Resource* source)
{
    using Fn = void(STDMETHODCALLTYPE*)(ID3D11DeviceContext*, ID3D11Resource*, ID3D11Resource*);
    void* forwarded_destination = destination;
    void* forwarded_source = source;
    rsf_core_frame_tap_copy(&platform, context, &forwarded_destination, &forwarded_source);
    original<Fn>(RSF_FRAME_TAP_HOOK_COPY)(context,
                                          static_cast<ID3D11Resource*>(forwarded_destination),
                                          static_cast<ID3D11Resource*>(forwarded_source));
    leave();
}

// A shader's own constants on their way to the GPU. Unreal 4.18's D3D11 RHI sends them with
// UpdateSubresource from its CPU shadow (WindowsD3D11ConstantBuffer.cpp:90), whole sub-buffer at a
// time, right before the draw; only pooled uniform buffers go through Map.
void STDMETHODCALLTYPE hooked_update_subresource(ID3D11DeviceContext* context,
                                                 ID3D11Resource* destination, UINT subresource,
                                                 const D3D11_BOX* box, const void* data,
                                                 UINT row_pitch, UINT depth_pitch)
{
    using Fn = void(STDMETHODCALLTYPE*)(ID3D11DeviceContext*, ID3D11Resource*, UINT,
                                        const D3D11_BOX*, const void*, UINT, UINT);
    alignas(16) unsigned char scratch[4096];
    data = rsf_core_frame_tap_update(&platform, context, destination, subresource,
                                     box ? 1u : 0u, data, row_pitch, scratch);
    original<Fn>(RSF_FRAME_TAP_HOOK_UPDATE)(context, destination, subresource, box, data, row_pitch,
                                            depth_pitch);
    leave();
}

void STDMETHODCALLTYPE hooked_update_subresource1(ID3D11DeviceContext* context,
                                                  ID3D11Resource* destination, UINT subresource,
                                                  const D3D11_BOX* box, const void* data,
                                                  UINT row_pitch, UINT depth_pitch, UINT flags)
{
    using Fn = void(STDMETHODCALLTYPE*)(ID3D11DeviceContext*, ID3D11Resource*, UINT,
                                        const D3D11_BOX*, const void*, UINT, UINT, UINT);
    alignas(16) unsigned char scratch[4096];
    data = rsf_core_frame_tap_update(&platform, context, destination, subresource,
                                     box ? 1u : 0u, data, row_pitch, scratch);
    original<Fn>(RSF_FRAME_TAP_HOOK_UPDATE1)(context, destination, subresource, box, data,
                                             row_pitch, depth_pitch, flags);
    leave();
}

template <class Hook>
void* replacement(Hook hook)
{
    return reinterpret_cast<void*>(hook);
}

// Indexed by the hook ids. Their order is the patch order, and the context1 slots exist only on
// ID3D11DeviceContext1.
const rsf_frame_tap_hook hooks[RSF_FRAME_TAP_HOOK_COUNT] = {
    {18, 0, replacement(&hooked_vertex_buffers)},
    {19, 0, replacement(&hooked_index_buffer)},
    {17, 0, replacement(&hooked_layout)},
    {24, 0, replacement(&hooked_topology)},
    {11, 0, replacement(&hooked_vertex_shader)},
    {7, 0, replacement(&hooked_vertex_constants)},
    {20, 0, replacement(&hooked_indexed_instanced)},
    {21, 0, replacement(&hooked_instanced)},
    {110, 0, replacement(&hooked_clear_state)},
    {58, 0, replacement(&hooked_execute)},
    {39, 0,
     replacement(&indirect_draw<RSF_FRAME_TAP_HOOK_DRAW_INDEXED_INSTANCED_INDIRECT, 5,
                                ID3D11Buffer*, UINT>)},
    {40, 0,
     replacement(&indirect_draw<RSF_FRAME_TAP_HOOK_DRAW_INSTANCED_INDIRECT, 6, ID3D11Buffer*,
                                UINT>)},
    {38, 0, replacement(&indirect_draw<RSF_FRAME_TAP_HOOK_DRAW_AUTO, 4>)},
    {9, 0, replacement(&hooked_pixel_shader)},
    {35, 0, replacement(&hooked_blend_state)},
    {36, 0, replacement(&hooked_depth_stencil_state)},
    {slot_ps_set_shader_resources, 0, replacement(&hooked_ps_set_shader_resources)},
    {slot_ps_set_constant_buffers, 0, replacement(&hooked_ps_set_constant_buffers)},
    {slot_ps_set_samplers, 0, replacement(&hooked_ps_set_samplers)},
    {slot_draw_indexed, 0, replacement(&hooked_draw_indexed)},
    {slot_draw, 0, replacement(&hooked_draw)},
    {slot_om_set_render_targets, 0, replacement(&hooked_om_set_render_targets)},
    {slot_om_set_render_targets_and_uavs, 0, replacement(&hooked_om_set_render_targets_and_uavs)},
    {slot_rs_set_viewports, 0, replacement(&hooked_rs_set_viewports)},
    {slot_rs_set_scissor_rects, 0, replacement(&hooked_rs_set_scissor_rects)},
    {slot_clear_render_target_view, 0, replacement(&hooked_clear_render_target_view)},
    {14, 0, replacement(&hooked_map)},
    {15, 0, replacement(&hooked_unmap)},
    {111, 0, replacement(&pass_through<RSF_FRAME_TAP_HOOK_FLUSH>)},
    {41, 0, replacement(&hooked_dispatch)},
    {42, 0, replacement(&hooked_dispatch_indirect)},
    {46, 0,
     replacement(&pass_through<RSF_FRAME_TAP_HOOK_COPY_REGION, ID3D11Resource*, UINT, UINT, UINT,
                               UINT, ID3D11Resource*, UINT, const D3D11_BOX*>)},
    {47, 0, replacement(&hooked_copy_resource)},
    {48, 0, replacement(&hooked_update_subresource)},
    {49, 0,
     replacement(&pass_through<RSF_FRAME_TAP_HOOK_COPY_STRUCTURE_COUNT, ID3D11Buffer*, UINT,
                               ID3D11UnorderedAccessView*>)},
    {51, 0,
     replacement(&pass_through<RSF_FRAME_TAP_HOOK_CLEAR_UAV_UINT, ID3D11UnorderedAccessView*,
                               const UINT*>)},
    {52, 0,
     replacement(&pass_through<RSF_FRAME_TAP_HOOK_CLEAR_UAV_FLOAT, ID3D11UnorderedAccessView*,
                               const FLOAT*>)},
    {53, 0,
     replacement(&pass_through<RSF_FRAME_TAP_HOOK_CLEAR_DEPTH, ID3D11DepthStencilView*, UINT,
                               FLOAT, UINT8>)},
    {54, 0,
     replacement(&pass_through<RSF_FRAME_TAP_HOOK_GENERATE_MIPS, ID3D11ShaderResourceView*>)},
    {57, 0,
     replacement(&pass_through<RSF_FRAME_TAP_HOOK_RESOLVE, ID3D11Resource*, UINT, ID3D11Resource*,
                               UINT, DXGI_FORMAT>)},
    {115, 1,
     replacement(&pass_through<RSF_FRAME_TAP_HOOK_COPY_REGION1, ID3D11Resource*, UINT, UINT, UINT,
                               UINT, ID3D11Resource*, UINT, const D3D11_BOX*, UINT>)},
    {116, 1, replacement(&hooked_update_subresource1)},
};

} // namespace

extern "C" rsf_frame_tap_result rsf_frame_tap_install(void* device_context,
                                                      const rsf_frame_tap_options* options)
{
    return rsf_core_frame_tap_install(&platform, device_context, options, hooks);
}

extern "C" rsf_frame_tap_result rsf_frame_tap_set_sampler_bias(float bias)
{
    return rsf_core_frame_tap_set_sampler_bias(&platform, bias);
}

extern "C" rsf_frame_tap_result rsf_frame_tap_refresh(void)
{
    return rsf_core_frame_tap_refresh(&platform);
}

extern "C" rsf_frame_tap_result rsf_frame_tap_uninstall(void)
{
    return rsf_core_frame_tap_uninstall(&platform);
}

extern "C" rsf_frame_tap_result rsf_frame_tap_set_plan(const rsf_frame_tap_plan* plan)
{
    return rsf_core_frame_tap_set_plan(plan);
}

extern "C" rsf_frame_tap_result rsf_frame_tap_end_frame(void)
{
    return rsf_core_frame_tap_end_frame();
}

extern "C" rsf_frame_tap_result rsf_frame_tap_watch_input(void* texture)
{
    return rsf_core_frame_tap_watch_input(texture);
}

extern "C" rsf_frame_tap_result rsf_frame_tap_set_constant_watch(uint32_t bytes,
                                                                  rsf_frame_tap_constants_fn fn,
                                                                  void* user)
{
    return rsf_core_frame_tap_set_constant_watch(bytes, fn, user);
}

extern "C" void rsf_frame_tap_set_constant_watch_writable(uint32_t writable)
{
    rsf_core_frame_tap_set_constant_watch_writable(writable);
}

extern "C" rsf_frame_tap_result rsf_frame_tap_set_research_callbacks(
    rsf_frame_tap_target_fn draw, rsf_frame_tap_compute_fn compute, void* user)
{
    return rsf_core_frame_tap_set_research(nullptr, draw, compute, user);
}

extern "C" rsf_frame_tap_result rsf_frame_tap_set_research_phase_callbacks(
    rsf_frame_tap_target_fn before, rsf_frame_tap_target_fn after,
    rsf_frame_tap_compute_fn compute, void* user)
{
    return rsf_core_frame_tap_set_research(before, after, compute, user);
}

extern "C" void* rsf_frame_tap_bound_target(void)
{
    return rsf_core_frame_tap_bound_target();
}

extern "C" rsf_frame_tap_result rsf_frame_tap_set_constant_override(
    rsf_frame_tap_constant_override_fn fn, void* user)
{
    return rsf_core_frame_tap_set_constant_override(fn, user);
}

extern "C" void rsf_frame_tap_set_constant_override_format(uint32_t format)
{
    rsf_core_frame_tap_set_constant_override_format(format);
}

extern "C" rsf_frame_tap_result rsf_frame_tap_set_divert(const rsf_frame_tap_divert_setup* setup)
{
    return rsf_core_frame_tap_set_divert(&platform, setup);
}

extern "C" rsf_frame_tap_result rsf_frame_tap_set_candidates(
    const rsf_frame_tap_candidates* candidates)
{
    return rsf_core_frame_tap_set_candidates(candidates);
}

extern "C" rsf_frame_tap_result rsf_frame_tap_watch_target(uint32_t index, void* texture,
                                                           uint32_t limit)
{
    return rsf_core_frame_tap_watch_target(index, texture, limit);
}

extern "C" rsf_frame_tap_result rsf_frame_tap_get_status(rsf_frame_tap_status* status)
{
    return rsf_core_frame_tap_get_status(status);
}
