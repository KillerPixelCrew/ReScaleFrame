// SPDX-License-Identifier: MIT
// Typed D3D11 leaves for the binding scopes in runtime/orchestrator/src/d3d11_state.rs. Rust owns
// the snapshots and the references they hold, nesting, snapshot reuse and the restore order; these
// only issue the context calls.

#include "d3d11_state_private.h"

#include <windows.h>

#include <d3d11_1.h>

#include <cstddef>

namespace {

constexpr UINT saved_resources = D3D11_COMMONSHADER_INPUT_RESOURCE_SLOT_COUNT;
constexpr UINT saved_samplers = D3D11_COMMONSHADER_SAMPLER_SLOT_COUNT;
constexpr UINT saved_constants = D3D11_COMMONSHADER_CONSTANT_BUFFER_API_SLOT_COUNT;
constexpr UINT saved_vertex_buffers = D3D11_IA_VERTEX_INPUT_RESOURCE_SLOT_COUNT;
constexpr UINT saved_uavs = D3D11_1_UAV_SLOT_COUNT;
constexpr UINT saved_viewports = D3D11_VIEWPORT_AND_SCISSORRECT_OBJECT_COUNT_PER_PIPELINE;

} // namespace

// Every interface pointer below holds one reference, taken by a Get call and dropped by Rust.
struct rsf_d3d11_stage_snapshot {
    ID3D11ShaderResourceView* resources[saved_resources];
    ID3D11SamplerState* samplers[saved_samplers];
    ID3D11Buffer* constants[saved_constants];
    UINT first_constants[saved_constants];
    UINT constant_counts[saved_constants];
    ID3D11ClassInstance* classes[D3D11_SHADER_MAX_INTERFACES];
    UINT class_count;
};

struct rsf_d3d11_snapshot {
    // Set by Rust before the read.
    ID3D11DeviceContext1* context1;
    UINT uav_count;
    UINT constant_ranges;
    // Input assembler.
    ID3D11InputLayout* layout;
    ID3D11Buffer* index_buffer;
    D3D11_PRIMITIVE_TOPOLOGY topology;
    DXGI_FORMAT index_format;
    UINT index_offset;
    ID3D11Buffer* vertex_buffers[saved_vertex_buffers];
    UINT vertex_strides[saved_vertex_buffers];
    UINT vertex_offsets[saved_vertex_buffers];

    // Shaders and their stage-specific bindings, in operations[] order.
    ID3D11VertexShader* vertex_shader;
    ID3D11HullShader* hull_shader;
    ID3D11DomainShader* domain_shader;
    ID3D11GeometryShader* geometry_shader;
    ID3D11PixelShader* pixel_shader;
    ID3D11ComputeShader* compute_shader;
    rsf_d3d11_stage_snapshot stages[6];
    ID3D11UnorderedAccessView* compute_uavs[saved_uavs];
    ID3D11UnorderedAccessView* pixel_uavs[saved_uavs];
    ID3D11Buffer* stream_targets[D3D11_SO_BUFFER_SLOT_COUNT];
    ID3D11Predicate* predicate;
    BOOL predicate_value;

    // Rasteriser.
    ID3D11RasterizerState* raster;
    UINT viewport_count;
    D3D11_VIEWPORT viewports[saved_viewports];
    UINT scissor_count;
    D3D11_RECT scissors[saved_viewports];

    // Output merger.
    ID3D11RenderTargetView* targets[D3D11_SIMULTANEOUS_RENDER_TARGET_COUNT];
    ID3D11DepthStencilView* depth_view;
    ID3D11BlendState* blend;
    FLOAT blend_factor[4];
    UINT blend_mask;
    ID3D11DepthStencilState* depth_state;
    UINT stencil_reference;
};

struct rsf_d3d11_depth_snapshot {
    ID3D11RenderTargetView* targets[D3D11_SIMULTANEOUS_RENDER_TARGET_COUNT];
    ID3D11DepthStencilView* depth;
    ID3D11DepthStencilState* state;
    UINT reference;
    ID3D11PixelShader* shader;
    ID3D11ClassInstance* classes[D3D11_SHADER_MAX_INTERFACES];
    UINT class_count;
};

// What one fullscreen draw disturbs and nothing more: the output merger, the vertex and pixel
// shaders, the input assembler's layout and topology, pixel slot 0 of each binding kind, and the
// rasteriser with its viewports and scissors.
struct rsf_d3d11_draw_snapshot {
    ID3D11RenderTargetView* targets[D3D11_SIMULTANEOUS_RENDER_TARGET_COUNT];
    ID3D11DepthStencilView* depth_view;
    ID3D11VertexShader* vertex_shader;
    ID3D11PixelShader* pixel_shader;
    ID3D11InputLayout* layout;
    D3D11_PRIMITIVE_TOPOLOGY topology;
    ID3D11ShaderResourceView* resource;
    ID3D11SamplerState* sampler;
    ID3D11Buffer* constants;
    ID3D11BlendState* blend;
    FLOAT blend_factor[4];
    UINT blend_mask;
    ID3D11DepthStencilState* depth_state;
    UINT stencil_reference;
    ID3D11RasterizerState* raster;
    UINT viewport_count;
    D3D11_VIEWPORT viewports[saved_viewports];
    UINT scissor_count;
    D3D11_RECT scissors[saved_viewports];
};

struct rsf_d3d11_features {
    uint32_t uav_count;
    uint32_t constant_ranges;
};

struct rsf_d3d11_state_platform {
    uint32_t (*immediate)(void* context);
    /* Retained device of the context. */
    void* (*device)(void* context);
    void (*features)(void* device, rsf_d3d11_features*);
    /* Retained ID3D11DeviceContext1, or null. */
    void* (*context1)(void* context);
    void (*read)(void* context, rsf_d3d11_snapshot*);
    /* Removes foreign inputs and outputs before the saved bindings return. */
    void (*clear)(void* context, uint32_t uav_count);
    /* Binds the snapshot; pixel UAVs start at target_count. Takes no references of its own. */
    void (*write)(void* context, const rsf_d3d11_snapshot*, uint32_t target_count);
    void (*read_depth)(void* context, rsf_d3d11_depth_snapshot*);
    void (*write_depth)(void* context, const rsf_d3d11_depth_snapshot*);
    void (*read_draw)(void* context, rsf_d3d11_draw_snapshot*);
    void (*write_draw)(void* context, const rsf_d3d11_draw_snapshot*);
};

extern "C" {
uint32_t rsf_core_d3d11_state_save(void*, rsf_d3d11_state*, uint32_t independent,
    const rsf_d3d11_state_platform*);
void rsf_core_d3d11_state_restore(void*, rsf_d3d11_state*, const rsf_d3d11_state_platform*);
uint32_t rsf_core_d3d11_depth_state_save(void*, rsf_d3d11_state*, const rsf_d3d11_state_platform*);
void rsf_core_d3d11_depth_state_restore(void*, rsf_d3d11_state*, const rsf_d3d11_state_platform*);
uint32_t rsf_core_d3d11_draw_state_save(void*, rsf_d3d11_state*, const rsf_d3d11_state_platform*);
void rsf_core_d3d11_draw_state_restore(void*, rsf_d3d11_state*, const rsf_d3d11_state_platform*);
}

namespace {

struct StageOperations {
    void (STDMETHODCALLTYPE ID3D11DeviceContext::*get_resources)(UINT, UINT, ID3D11ShaderResourceView**);
    void (STDMETHODCALLTYPE ID3D11DeviceContext::*set_resources)(UINT, UINT, ID3D11ShaderResourceView* const*);
    void (STDMETHODCALLTYPE ID3D11DeviceContext::*get_samplers)(UINT, UINT, ID3D11SamplerState**);
    void (STDMETHODCALLTYPE ID3D11DeviceContext::*set_samplers)(UINT, UINT, ID3D11SamplerState* const*);
    void (STDMETHODCALLTYPE ID3D11DeviceContext::*get_constants)(UINT, UINT, ID3D11Buffer**);
    void (STDMETHODCALLTYPE ID3D11DeviceContext::*set_constants)(UINT, UINT, ID3D11Buffer* const*);
    void (STDMETHODCALLTYPE ID3D11DeviceContext1::*get_ranges)(UINT, UINT, ID3D11Buffer**, UINT*, UINT*);
    void (STDMETHODCALLTYPE ID3D11DeviceContext1::*set_ranges)(UINT, UINT, ID3D11Buffer* const*, const UINT*, const UINT*);
};

#define STAGE_OPERATIONS(name) { &ID3D11DeviceContext::name##GetShaderResources, \
    &ID3D11DeviceContext::name##SetShaderResources, &ID3D11DeviceContext::name##GetSamplers, \
    &ID3D11DeviceContext::name##SetSamplers, &ID3D11DeviceContext::name##GetConstantBuffers, \
    &ID3D11DeviceContext::name##SetConstantBuffers, &ID3D11DeviceContext1::name##GetConstantBuffers1, \
    &ID3D11DeviceContext1::name##SetConstantBuffers1 }
constexpr StageOperations operations[]{
    STAGE_OPERATIONS(VS), STAGE_OPERATIONS(HS), STAGE_OPERATIONS(DS),
    STAGE_OPERATIONS(GS), STAGE_OPERATIONS(PS), STAGE_OPERATIONS(CS)
};
#undef STAGE_OPERATIONS

ID3D11DeviceContext* as_context(void* context)
{
    return static_cast<ID3D11DeviceContext*>(context);
}

uint32_t immediate(void* context)
{
    return as_context(context)->GetType() == D3D11_DEVICE_CONTEXT_IMMEDIATE;
}

void* device(void* context)
{
    ID3D11Device* owner = nullptr;
    as_context(context)->GetDevice(&owner);
    return owner;
}

void features(void* raw, rsf_d3d11_features* out)
{
    auto* owner = static_cast<ID3D11Device*>(raw);
    D3D11_FEATURE_DATA_D3D11_OPTIONS options{};
    out->uav_count = owner->GetFeatureLevel() >= D3D_FEATURE_LEVEL_11_1 ?
        saved_uavs : D3D11_PS_CS_UAV_REGISTER_COUNT;
    out->constant_ranges =
        SUCCEEDED(owner->CheckFeatureSupport(D3D11_FEATURE_D3D11_OPTIONS, &options, sizeof(options))) &&
        options.ConstantBufferOffsetting;
}

void* context1(void* context)
{
    ID3D11DeviceContext1* out = nullptr;
    as_context(context)->QueryInterface(IID_PPV_ARGS(&out));
    return out;
}

void read(void* raw, rsf_d3d11_snapshot* state)
{
    auto* context = as_context(raw);
    context->IAGetInputLayout(&state->layout);
    context->IAGetPrimitiveTopology(&state->topology);
    context->IAGetIndexBuffer(&state->index_buffer, &state->index_format, &state->index_offset);
    context->IAGetVertexBuffers(0, saved_vertex_buffers, state->vertex_buffers,
                                state->vertex_strides, state->vertex_offsets);

    auto* stages = state->stages;
    for (auto& stage : state->stages) stage.class_count = D3D11_SHADER_MAX_INTERFACES;
    context->VSGetShader(&state->vertex_shader, stages[0].classes, &stages[0].class_count);
    context->HSGetShader(&state->hull_shader, stages[1].classes, &stages[1].class_count);
    context->DSGetShader(&state->domain_shader, stages[2].classes, &stages[2].class_count);
    context->GSGetShader(&state->geometry_shader, stages[3].classes, &stages[3].class_count);
    context->PSGetShader(&state->pixel_shader, stages[4].classes, &stages[4].class_count);
    context->CSGetShader(&state->compute_shader, stages[5].classes, &stages[5].class_count);

    for (size_t i = 0; i < 6; ++i) {
        auto& stage = stages[i];
        const auto& op = operations[i];
        (context->*op.get_resources)(0, saved_resources, stage.resources);
        (context->*op.get_samplers)(0, saved_samplers, stage.samplers);
        if (state->constant_ranges) {
            (state->context1->*op.get_ranges)(0, saved_constants, stage.constants,
                                              stage.first_constants, stage.constant_counts);
        } else {
            (context->*op.get_constants)(0, saved_constants, stage.constants);
        }
    }
    context->CSGetUnorderedAccessViews(0, state->uav_count, state->compute_uavs);
    context->SOGetTargets(D3D11_SO_BUFFER_SLOT_COUNT, state->stream_targets);
    context->GetPredication(&state->predicate, &state->predicate_value);

    context->RSGetState(&state->raster);
    state->viewport_count = saved_viewports;
    context->RSGetViewports(&state->viewport_count, state->viewports);
    state->scissor_count = saved_viewports;
    context->RSGetScissorRects(&state->scissor_count, state->scissors);

    context->OMGetRenderTargetsAndUnorderedAccessViews(D3D11_SIMULTANEOUS_RENDER_TARGET_COUNT,
        state->targets, &state->depth_view, 0, state->uav_count, state->pixel_uavs);
    context->OMGetBlendState(&state->blend, state->blend_factor, &state->blend_mask);
    context->OMGetDepthStencilState(&state->depth_state, &state->stencil_reference);
}

// Hidden UAV counters are preserved, not reset.
struct KeepCounts {
    UINT values[saved_uavs];
    KeepCounts()
    {
        for (auto& value : values) value = UINT(-1);
    }
};

void clear(void* raw, uint32_t uav_count)
{
    auto* context = as_context(raw);
    ID3D11ShaderResourceView* no_resources[saved_resources] = {};
    ID3D11UnorderedAccessView* no_uavs[saved_uavs] = {};
    const KeepCounts keep_counts;
    for (const auto& op : operations) (context->*op.set_resources)(0, saved_resources, no_resources);
    context->CSSetUnorderedAccessViews(0, uav_count, no_uavs, keep_counts.values);
    context->OMSetRenderTargetsAndUnorderedAccessViews(0, nullptr, nullptr,
        0, uav_count, no_uavs, keep_counts.values);
    context->SOSetTargets(0, nullptr, nullptr);
    ID3D11Buffer* no_vertices[saved_vertex_buffers]{};
    UINT no_strides[saved_vertex_buffers]{};
    context->IASetVertexBuffers(0, saved_vertex_buffers, no_vertices, no_strides, no_strides);
    context->IASetIndexBuffer(nullptr, DXGI_FORMAT_UNKNOWN, 0);
}

void write(void* raw, const rsf_d3d11_snapshot* state, uint32_t target_count)
{
    auto* context = as_context(raw);
    const KeepCounts keep_counts;
    context->OMSetRenderTargetsAndUnorderedAccessViews(target_count, state->targets, state->depth_view,
        target_count, state->uav_count - target_count, state->pixel_uavs + target_count,
        keep_counts.values + target_count);
    context->OMSetBlendState(state->blend, state->blend_factor, state->blend_mask);
    context->OMSetDepthStencilState(state->depth_state, state->stencil_reference);

    context->CSSetUnorderedAccessViews(0, state->uav_count, state->compute_uavs, keep_counts.values);
    // Stream-output targets resume their existing write cursors.
    const UINT stream_offsets[D3D11_SO_BUFFER_SLOT_COUNT]{UINT(-1), UINT(-1), UINT(-1), UINT(-1)};
    context->SOSetTargets(D3D11_SO_BUFFER_SLOT_COUNT, state->stream_targets, stream_offsets);

    const auto* stages = state->stages;
    for (size_t i = 0; i < 6; ++i) {
        const auto& stage = stages[i];
        const auto& op = operations[i];
        (context->*op.set_resources)(0, saved_resources, stage.resources);
        (context->*op.set_samplers)(0, saved_samplers, stage.samplers);
        (context->*op.set_constants)(0, saved_constants, stage.constants);
        if (state->constant_ranges) {
            (state->context1->*op.set_ranges)(0, saved_constants, stage.constants,
                                              stage.first_constants, stage.constant_counts);
        }
    }

    context->VSSetShader(state->vertex_shader, stages[0].classes, stages[0].class_count);
    context->HSSetShader(state->hull_shader, stages[1].classes, stages[1].class_count);
    context->DSSetShader(state->domain_shader, stages[2].classes, stages[2].class_count);
    context->GSSetShader(state->geometry_shader, stages[3].classes, stages[3].class_count);
    context->PSSetShader(state->pixel_shader, stages[4].classes, stages[4].class_count);
    context->CSSetShader(state->compute_shader, stages[5].classes, stages[5].class_count);

    context->IASetInputLayout(state->layout);
    context->IASetPrimitiveTopology(state->topology);
    context->IASetIndexBuffer(state->index_buffer, state->index_format, state->index_offset);
    context->IASetVertexBuffers(0, saved_vertex_buffers, state->vertex_buffers,
                                state->vertex_strides, state->vertex_offsets);

    context->RSSetState(state->raster);
    context->RSSetViewports(state->viewport_count, state->viewports);
    context->RSSetScissorRects(state->scissor_count, state->scissors);
    context->SetPredication(state->predicate, state->predicate_value);
}

void read_depth(void* raw, rsf_d3d11_depth_snapshot* s)
{
    auto* context = as_context(raw);
    context->OMGetRenderTargets(D3D11_SIMULTANEOUS_RENDER_TARGET_COUNT, s->targets, &s->depth);
    context->OMGetDepthStencilState(&s->state, &s->reference);
    s->class_count = D3D11_SHADER_MAX_INTERFACES;
    context->PSGetShader(&s->shader, s->classes, &s->class_count);
}

void write_depth(void* raw, const rsf_d3d11_depth_snapshot* s)
{
    auto* context = as_context(raw);
    context->OMSetRenderTargets(D3D11_SIMULTANEOUS_RENDER_TARGET_COUNT, s->targets, s->depth);
    context->OMSetDepthStencilState(s->state, s->reference);
    context->PSSetShader(s->shader, s->classes, s->class_count);
}

void read_draw(void* raw, rsf_d3d11_draw_snapshot* s)
{
    auto* context = as_context(raw);
    context->OMGetRenderTargets(D3D11_SIMULTANEOUS_RENDER_TARGET_COUNT, s->targets, &s->depth_view);
    context->VSGetShader(&s->vertex_shader, nullptr, nullptr);
    context->PSGetShader(&s->pixel_shader, nullptr, nullptr);
    context->IAGetInputLayout(&s->layout);
    context->IAGetPrimitiveTopology(&s->topology);
    context->PSGetShaderResources(0, 1, &s->resource);
    context->PSGetSamplers(0, 1, &s->sampler);
    context->PSGetConstantBuffers(0, 1, &s->constants);
    context->OMGetBlendState(&s->blend, s->blend_factor, &s->blend_mask);
    context->OMGetDepthStencilState(&s->depth_state, &s->stencil_reference);
    context->RSGetState(&s->raster);
    s->viewport_count = saved_viewports;
    context->RSGetViewports(&s->viewport_count, s->viewports);
    s->scissor_count = saved_viewports;
    context->RSGetScissorRects(&s->scissor_count, s->scissors);
}

void write_draw(void* raw, const rsf_d3d11_draw_snapshot* s)
{
    auto* context = as_context(raw);
    context->OMSetRenderTargets(D3D11_SIMULTANEOUS_RENDER_TARGET_COUNT, s->targets, s->depth_view);
    context->VSSetShader(s->vertex_shader, nullptr, 0);
    context->PSSetShader(s->pixel_shader, nullptr, 0);
    context->IASetInputLayout(s->layout);
    context->IASetPrimitiveTopology(s->topology);
    context->PSSetShaderResources(0, 1, &s->resource);
    context->PSSetSamplers(0, 1, &s->sampler);
    context->PSSetConstantBuffers(0, 1, &s->constants);
    context->OMSetBlendState(s->blend, s->blend_factor, s->blend_mask);
    context->OMSetDepthStencilState(s->depth_state, s->stencil_reference);
    context->RSSetState(s->raster);
    context->RSSetViewports(s->viewport_count, s->viewports);
    context->RSSetScissorRects(s->scissor_count, s->scissors);
}

constexpr rsf_d3d11_state_platform platform{
    immediate, device, features, context1, read, clear, write, read_depth, write_depth, read_draw, write_draw,
};
// Layouts the Rust owner mirrors.
static_assert(sizeof(rsf_d3d11_state_platform) == 11 * sizeof(void*));
static_assert(sizeof(rsf_d3d11_features) == 8);
static_assert(D3D11_SHADER_MAX_INTERFACES == 253);
static_assert(sizeof(rsf_d3d11_stage_snapshot) == 3408);
static_assert(sizeof(rsf_d3d11_snapshot) == 22904 && offsetof(rsf_d3d11_snapshot, stages) == 608 &&
    offsetof(rsf_d3d11_snapshot, targets) == 22784);
static_assert(sizeof(rsf_d3d11_depth_snapshot) == 2128 && sizeof(rsf_d3d11_draw_snapshot) == 832);
static_assert(sizeof(D3D11_VIEWPORT) == 24 && sizeof(D3D11_RECT) == 16);

} // namespace

extern "C" uint32_t rsf_d3d11_state_save(void* context, rsf_d3d11_state* state)
{
    return rsf_core_d3d11_state_save(context, state, 0, &platform);
}

uint32_t rsf_d3d11_state_save_independent(void* context, rsf_d3d11_state* state)
{
    return rsf_core_d3d11_state_save(context, state, 1, &platform);
}

extern "C" void rsf_d3d11_state_restore(void* context, rsf_d3d11_state* state)
{
    rsf_core_d3d11_state_restore(context, state, &platform);
}

extern "C" uint32_t rsf_d3d11_depth_state_save(void* context, rsf_d3d11_state* state)
{
    return rsf_core_d3d11_depth_state_save(context, state, &platform);
}

extern "C" void rsf_d3d11_depth_state_restore(void* context, rsf_d3d11_state* state)
{
    rsf_core_d3d11_depth_state_restore(context, state, &platform);
}

extern "C" uint32_t rsf_d3d11_draw_state_save(void* context, rsf_d3d11_state* state)
{
    return rsf_core_d3d11_draw_state_save(context, state, &platform);
}

extern "C" void rsf_d3d11_draw_state_restore(void* context, rsf_d3d11_state* state)
{
    rsf_core_d3d11_draw_state_restore(context, state, &platform);
}

extern "C" void rsf_d3d11_copy_resource(void* context, void* destination, void* source)
{
    if (!context || !destination || !source) {
        return;
    }
    static_cast<ID3D11DeviceContext*>(context)->CopyResource(
        static_cast<ID3D11Resource*>(destination), static_cast<ID3D11Resource*>(source));
}

extern "C" void* rsf_d3d11_create_shader_view(void* device, void* texture)
{
    ID3D11ShaderResourceView* view = nullptr;
    if (!device || !texture ||
        FAILED(static_cast<ID3D11Device*>(device)->CreateShaderResourceView(
            static_cast<ID3D11Resource*>(texture), nullptr, &view))) {
        return nullptr;
    }
    return view;
}
