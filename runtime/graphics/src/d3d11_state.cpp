// SPDX-License-Identifier: GPL-3.0-only

#include <rescaleframe/d3d11_state.h>

#include <windows.h>

#include <d3d11_1.h>

#include <atomic>
#include <cstring>
#include <new>

namespace {

constexpr UINT saved_resources = D3D11_COMMONSHADER_INPUT_RESOURCE_SLOT_COUNT;
constexpr UINT saved_samplers = D3D11_COMMONSHADER_SAMPLER_SLOT_COUNT;
constexpr UINT saved_constants = D3D11_COMMONSHADER_CONSTANT_BUFFER_API_SLOT_COUNT;
constexpr UINT saved_vertex_buffers = D3D11_IA_VERTEX_INPUT_RESOURCE_SLOT_COUNT;
constexpr UINT saved_uavs = D3D11_1_UAV_SLOT_COUNT;
constexpr UINT saved_viewports = D3D11_VIEWPORT_AND_SCISSORRECT_OBJECT_COUNT_PER_PIPELINE;

struct Stage {
    ID3D11ShaderResourceView* resources[saved_resources];
    ID3D11SamplerState* samplers[saved_samplers];
    ID3D11Buffer* constants[saved_constants];
    UINT first_constants[saved_constants];
    UINT constant_counts[saved_constants];
    ID3D11ClassInstance* classes[D3D11_SHADER_MAX_INTERFACES];
    UINT class_count;
};

struct State {
    ID3D11DeviceContext1* context1;
    bool constant_ranges;
    UINT uav_count;
    // Input assembler.
    ID3D11InputLayout* layout;
    D3D11_PRIMITIVE_TOPOLOGY topology;
    ID3D11Buffer* index_buffer;
    DXGI_FORMAT index_format;
    UINT index_offset;
    ID3D11Buffer* vertex_buffers[saved_vertex_buffers];
    UINT vertex_strides[saved_vertex_buffers];
    UINT vertex_offsets[saved_vertex_buffers];

    // Shaders and their stage-specific bindings.
    ID3D11VertexShader* vertex_shader;
    ID3D11HullShader* hull_shader;
    ID3D11DomainShader* domain_shader;
    ID3D11GeometryShader* geometry_shader;
    ID3D11PixelShader* pixel_shader;
    ID3D11ComputeShader* compute_shader;

    Stage vertex;
    Stage hull;
    Stage domain;
    Stage geometry;
    Stage pixel;
    Stage compute;
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

struct Storage {
    // Null for a save that nested under an outer scope on the same context and took nothing.
    State* snapshot;
    // This save is the one the thread's nesting check refers to.
    bool outermost;
};
static_assert(sizeof(Storage) <= RSF_D3D11_STATE_BYTES);
static_assert(alignof(Storage) <= alignof(uint64_t));

// The context a save is open on, for this thread. A pass that runs inside another's save/restore
// would only capture what the outer scope is already going to put back, at the cost of reading and
// rebinding every slot of six stages, so it takes nothing and its restore does nothing. The passes
// that nest set every binding they use and leave the outer scope to restore the game's.
thread_local const void* open_scope = nullptr;

// One snapshot is kept for reuse. A save takes it or allocates, a restore hands it back, so a frame
// of passes does not allocate and free ~22 KB each time. Exchange rather than a lock: a thread that
// finds it taken simply allocates its own.
std::atomic<State*> spare_snapshot{nullptr};

State* take_snapshot()
{
    State* snapshot = spare_snapshot.exchange(nullptr, std::memory_order_acquire);
    if (!snapshot) {
        return new (std::nothrow) State{};
    }
    std::memset(snapshot, 0, sizeof(State));
    return snapshot;
}

void give_back_snapshot(State* snapshot)
{
    delete spare_snapshot.exchange(snapshot, std::memory_order_release);
}

// What a device can do does not change for its lifetime, so the feature queries are made once per
// device and thread. The context's own interface is still asked for on each save when constant
// buffer ranges are in play, because the snapshot holds a reference to it.
struct DeviceTraits {
    const void* device;
    UINT uav_count;
    bool constant_ranges;
};
thread_local DeviceTraits cached_traits{};

Storage& storage_of(rsf_d3d11_state* state)
{
    return *reinterpret_cast<Storage*>(state->opaque);
}

void release_array(IUnknown* const* items, size_t count)
{
    for (size_t index = 0; index < count; ++index) {
        if (items[index]) {
            items[index]->Release();
        }
    }
}

void release_stage(const Stage& stage)
{
    release_array(reinterpret_cast<IUnknown* const*>(stage.resources), saved_resources);
    release_array(reinterpret_cast<IUnknown* const*>(stage.samplers), saved_samplers);
    release_array(reinterpret_cast<IUnknown* const*>(stage.constants), saved_constants);
    release_array(reinterpret_cast<IUnknown* const*>(stage.classes), stage.class_count);
}

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

} // namespace

extern "C" uint32_t rsf_d3d11_state_save(void* context_pointer, rsf_d3d11_state* state_pointer)
{
    if (!context_pointer || !state_pointer) {
        return 0;
    }
    auto* context = static_cast<ID3D11DeviceContext*>(context_pointer);
    std::memset(state_pointer, 0, sizeof(*state_pointer));
    if (context->GetType() != D3D11_DEVICE_CONTEXT_IMMEDIATE) return 0;
    if (open_scope == context) {
        new (state_pointer->opaque) Storage{nullptr, false};
        return 1;
    }
    State* snapshot = take_snapshot();
    if (!snapshot) return 0;
    const bool outermost = !open_scope;
    if (outermost) open_scope = context;
    new (state_pointer->opaque) Storage{snapshot, outermost};
    State& state = *snapshot;
    ID3D11Device* device = nullptr;
    context->GetDevice(&device);
    if (cached_traits.device != device) {
        D3D11_FEATURE_DATA_D3D11_OPTIONS options{};
        cached_traits.uav_count = device->GetFeatureLevel() >= D3D_FEATURE_LEVEL_11_1 ?
            saved_uavs : D3D11_PS_CS_UAV_REGISTER_COUNT;
        cached_traits.constant_ranges =
            SUCCEEDED(device->CheckFeatureSupport(D3D11_FEATURE_D3D11_OPTIONS, &options, sizeof(options))) &&
            options.ConstantBufferOffsetting;
        cached_traits.device = device;
    }
    device->Release();
    state.uav_count = cached_traits.uav_count;
    if (cached_traits.constant_ranges) {
        context->QueryInterface(IID_PPV_ARGS(&state.context1));
        state.constant_ranges = state.context1 != nullptr;
    }

    context->IAGetInputLayout(&state.layout);
    context->IAGetPrimitiveTopology(&state.topology);
    context->IAGetIndexBuffer(&state.index_buffer, &state.index_format, &state.index_offset);
    context->IAGetVertexBuffers(0, saved_vertex_buffers, state.vertex_buffers,
                                state.vertex_strides, state.vertex_offsets);

    state.vertex.class_count = state.hull.class_count = state.domain.class_count =
        state.geometry.class_count = state.pixel.class_count = state.compute.class_count = D3D11_SHADER_MAX_INTERFACES;
    context->VSGetShader(&state.vertex_shader, state.vertex.classes, &state.vertex.class_count);
    context->HSGetShader(&state.hull_shader, state.hull.classes, &state.hull.class_count);
    context->DSGetShader(&state.domain_shader, state.domain.classes, &state.domain.class_count);
    context->GSGetShader(&state.geometry_shader, state.geometry.classes, &state.geometry.class_count);
    context->PSGetShader(&state.pixel_shader, state.pixel.classes, &state.pixel.class_count);
    context->CSGetShader(&state.compute_shader, state.compute.classes, &state.compute.class_count);

    Stage* stages[]{&state.vertex, &state.hull, &state.domain, &state.geometry, &state.pixel, &state.compute};
    for (size_t i = 0; i < 6; ++i) {
        auto& stage = *stages[i];
        const auto& op = operations[i];
        (context->*op.get_resources)(0, saved_resources, stage.resources);
        (context->*op.get_samplers)(0, saved_samplers, stage.samplers);
        if (state.constant_ranges) {
            (state.context1->*op.get_ranges)(0, saved_constants, stage.constants,
                                           stage.first_constants, stage.constant_counts);
        } else {
            (context->*op.get_constants)(0, saved_constants, stage.constants);
        }
    }
    context->CSGetUnorderedAccessViews(0, state.uav_count, state.compute_uavs);
    context->SOGetTargets(D3D11_SO_BUFFER_SLOT_COUNT, state.stream_targets);
    context->GetPredication(&state.predicate, &state.predicate_value);

    context->RSGetState(&state.raster);
    state.viewport_count = saved_viewports;
    context->RSGetViewports(&state.viewport_count, state.viewports);
    state.scissor_count = saved_viewports;
    context->RSGetScissorRects(&state.scissor_count, state.scissors);

    context->OMGetRenderTargetsAndUnorderedAccessViews(D3D11_SIMULTANEOUS_RENDER_TARGET_COUNT,
        state.targets, &state.depth_view, 0, state.uav_count, state.pixel_uavs);
    context->OMGetBlendState(&state.blend, state.blend_factor, &state.blend_mask);
    context->OMGetDepthStencilState(&state.depth_state, &state.stencil_reference);
    return 1;
}

extern "C" void rsf_d3d11_state_restore(void* context_pointer, rsf_d3d11_state* state_pointer)
{
    if (!context_pointer || !state_pointer) {
        return;
    }
    auto* context = static_cast<ID3D11DeviceContext*>(context_pointer);
    const Storage storage = storage_of(state_pointer);
    auto* snapshot = storage.snapshot;
    if (!snapshot) {
        // A nested save took nothing, so there is nothing to put back.
        std::memset(state_pointer, 0, sizeof(*state_pointer));
        return;
    }
    State& state = *snapshot;

    // Remove foreign bindings from every stage before restoring potentially conflicting resources.
    ID3D11ShaderResourceView* no_resources[saved_resources] = {};
    ID3D11UnorderedAccessView* no_uavs[saved_uavs] = {};
    UINT keep_counts[saved_uavs];
    for (auto& count : keep_counts) count = UINT(-1);
    for (const auto& op : operations) (context->*op.set_resources)(0, saved_resources, no_resources);
    context->CSSetUnorderedAccessViews(0, state.uav_count, no_uavs, keep_counts);
    context->OMSetRenderTargetsAndUnorderedAccessViews(0, nullptr, nullptr,
        0, state.uav_count, no_uavs, keep_counts);
    context->SOSetTargets(0, nullptr, nullptr);
    ID3D11Buffer* no_vertices[saved_vertex_buffers]{};
    UINT no_strides[saved_vertex_buffers]{};
    context->IASetVertexBuffers(0, saved_vertex_buffers, no_vertices, no_strides, no_strides);
    context->IASetIndexBuffer(nullptr, DXGI_FORMAT_UNKNOWN, 0);

    // OM render targets and UAVs share slot numbers. Start UAV restoration after the last RTV.
    UINT target_count = 0;
    for (UINT i = 0; i < D3D11_SIMULTANEOUS_RENDER_TARGET_COUNT; ++i)
        if (state.targets[i]) target_count = i + 1;
    context->OMSetRenderTargetsAndUnorderedAccessViews(target_count, state.targets, state.depth_view,
        target_count, state.uav_count - target_count, state.pixel_uavs + target_count,
        keep_counts + target_count);
    context->OMSetBlendState(state.blend, state.blend_factor, state.blend_mask);
    context->OMSetDepthStencilState(state.depth_state, state.stencil_reference);

    context->CSSetUnorderedAccessViews(0, state.uav_count, state.compute_uavs, keep_counts);
    UINT stream_offsets[D3D11_SO_BUFFER_SLOT_COUNT];
    for (auto& offset : stream_offsets) offset = UINT(-1);
    context->SOSetTargets(D3D11_SO_BUFFER_SLOT_COUNT, state.stream_targets, stream_offsets);

    Stage* stages[]{&state.vertex, &state.hull, &state.domain, &state.geometry, &state.pixel, &state.compute};
    for (size_t i = 0; i < 6; ++i) {
        auto& stage = *stages[i];
        const auto& op = operations[i];
        (context->*op.set_resources)(0, saved_resources, stage.resources);
        (context->*op.set_samplers)(0, saved_samplers, stage.samplers);
        (context->*op.set_constants)(0, saved_constants, stage.constants);
        if (state.constant_ranges) {
            (state.context1->*op.set_ranges)(0, saved_constants, stage.constants,
                                           stage.first_constants, stage.constant_counts);
        }
    }

    context->VSSetShader(state.vertex_shader, state.vertex.classes, state.vertex.class_count);
    context->HSSetShader(state.hull_shader, state.hull.classes, state.hull.class_count);
    context->DSSetShader(state.domain_shader, state.domain.classes, state.domain.class_count);
    context->GSSetShader(state.geometry_shader, state.geometry.classes, state.geometry.class_count);
    context->PSSetShader(state.pixel_shader, state.pixel.classes, state.pixel.class_count);
    context->CSSetShader(state.compute_shader, state.compute.classes, state.compute.class_count);

    context->IASetInputLayout(state.layout);
    context->IASetPrimitiveTopology(state.topology);
    context->IASetIndexBuffer(state.index_buffer, state.index_format, state.index_offset);
    context->IASetVertexBuffers(0, saved_vertex_buffers, state.vertex_buffers,
                                state.vertex_strides, state.vertex_offsets);

    context->RSSetState(state.raster);
    context->RSSetViewports(state.viewport_count, state.viewports);
    context->RSSetScissorRects(state.scissor_count, state.scissors);
    context->SetPredication(state.predicate, state.predicate_value);

    release_array(reinterpret_cast<IUnknown* const*>(state.vertex_buffers), saved_vertex_buffers);
    for (const auto* stage : stages) release_stage(*stage);
    release_array(reinterpret_cast<IUnknown* const*>(state.compute_uavs), state.uav_count);
    release_array(reinterpret_cast<IUnknown* const*>(state.pixel_uavs), state.uav_count);
    release_array(reinterpret_cast<IUnknown* const*>(state.stream_targets), D3D11_SO_BUFFER_SLOT_COUNT);
    release_array(reinterpret_cast<IUnknown* const*>(state.targets),
                  D3D11_SIMULTANEOUS_RENDER_TARGET_COUNT);

    IUnknown* const singles[] = {
        state.layout,          state.index_buffer,    state.vertex_shader,  state.hull_shader,
        state.domain_shader,   state.geometry_shader, state.pixel_shader,   state.compute_shader,
        state.raster,          state.depth_view,      state.blend,          state.depth_state,
        state.context1,        state.predicate,
    };
    release_array(singles, sizeof(singles) / sizeof(singles[0]));

    if (storage.outermost) open_scope = nullptr;
    give_back_snapshot(snapshot);
    std::memset(state_pointer, 0, sizeof(*state_pointer));
}

namespace {
struct DepthState {
    ID3D11RenderTargetView* targets[D3D11_SIMULTANEOUS_RENDER_TARGET_COUNT];
    ID3D11DepthStencilView* depth;
    ID3D11DepthStencilState* state;
    UINT reference;
    ID3D11PixelShader* shader;
    ID3D11ClassInstance* classes[D3D11_SHADER_MAX_INTERFACES];
    UINT class_count;
};
static_assert(sizeof(DepthState) <= RSF_D3D11_STATE_BYTES);
} // namespace

extern "C" uint32_t rsf_d3d11_depth_state_save(void* pointer, rsf_d3d11_state* storage)
{
    if (!pointer || !storage) {
        return 0;
    }
    auto* context = static_cast<ID3D11DeviceContext*>(pointer);
    auto& s = *new (storage->opaque) DepthState{};
    context->OMGetRenderTargets(D3D11_SIMULTANEOUS_RENDER_TARGET_COUNT, s.targets, &s.depth);
    context->OMGetDepthStencilState(&s.state, &s.reference);
    s.class_count = D3D11_SHADER_MAX_INTERFACES;
    context->PSGetShader(&s.shader, s.classes, &s.class_count);
    return 1;
}

extern "C" void rsf_d3d11_depth_state_restore(void* pointer, rsf_d3d11_state* storage)
{
    if (!pointer || !storage) {
        return;
    }
    auto* context = static_cast<ID3D11DeviceContext*>(pointer);
    auto& s = *reinterpret_cast<DepthState*>(storage->opaque);
    context->OMSetRenderTargets(D3D11_SIMULTANEOUS_RENDER_TARGET_COUNT, s.targets, s.depth);
    context->OMSetDepthStencilState(s.state, s.reference);
    context->PSSetShader(s.shader, s.classes, s.class_count);
    release_array(reinterpret_cast<IUnknown* const*>(s.targets),
                  D3D11_SIMULTANEOUS_RENDER_TARGET_COUNT);
    release_array(reinterpret_cast<IUnknown* const*>(s.classes), s.class_count);
    if (s.depth) {
        s.depth->Release();
    }
    if (s.state) {
        s.state->Release();
    }
    if (s.shader) {
        s.shader->Release();
    }
    std::memset(storage, 0, sizeof(*storage));
}

namespace {
// What one fullscreen draw disturbs and nothing more: the output merger, the vertex and pixel
// shaders, the input assembler's layout and topology, pixel slot 0 of each binding kind, and the
// rasteriser with its viewports and scissors.
struct FullscreenState {
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
static_assert(sizeof(FullscreenState) <= RSF_D3D11_STATE_BYTES);
} // namespace

extern "C" uint32_t rsf_d3d11_draw_state_save(void* pointer, rsf_d3d11_state* storage)
{
    if (!pointer || !storage) {
        return 0;
    }
    auto* context = static_cast<ID3D11DeviceContext*>(pointer);
    auto& s = *new (storage->opaque) FullscreenState{};
    context->OMGetRenderTargets(D3D11_SIMULTANEOUS_RENDER_TARGET_COUNT, s.targets, &s.depth_view);
    context->VSGetShader(&s.vertex_shader, nullptr, nullptr);
    context->PSGetShader(&s.pixel_shader, nullptr, nullptr);
    context->IAGetInputLayout(&s.layout);
    context->IAGetPrimitiveTopology(&s.topology);
    context->PSGetShaderResources(0, 1, &s.resource);
    context->PSGetSamplers(0, 1, &s.sampler);
    context->PSGetConstantBuffers(0, 1, &s.constants);
    context->OMGetBlendState(&s.blend, s.blend_factor, &s.blend_mask);
    context->OMGetDepthStencilState(&s.depth_state, &s.stencil_reference);
    context->RSGetState(&s.raster);
    s.viewport_count = saved_viewports;
    context->RSGetViewports(&s.viewport_count, s.viewports);
    s.scissor_count = saved_viewports;
    context->RSGetScissorRects(&s.scissor_count, s.scissors);
    return 1;
}

extern "C" void rsf_d3d11_draw_state_restore(void* pointer, rsf_d3d11_state* storage)
{
    if (!pointer || !storage) {
        return;
    }
    auto* context = static_cast<ID3D11DeviceContext*>(pointer);
    auto& s = *reinterpret_cast<FullscreenState*>(storage->opaque);
    context->OMSetRenderTargets(D3D11_SIMULTANEOUS_RENDER_TARGET_COUNT, s.targets, s.depth_view);
    context->VSSetShader(s.vertex_shader, nullptr, 0);
    context->PSSetShader(s.pixel_shader, nullptr, 0);
    context->IASetInputLayout(s.layout);
    context->IASetPrimitiveTopology(s.topology);
    context->PSSetShaderResources(0, 1, &s.resource);
    context->PSSetSamplers(0, 1, &s.sampler);
    context->PSSetConstantBuffers(0, 1, &s.constants);
    context->OMSetBlendState(s.blend, s.blend_factor, s.blend_mask);
    context->OMSetDepthStencilState(s.depth_state, s.stencil_reference);
    context->RSSetState(s.raster);
    context->RSSetViewports(s.viewport_count, s.viewports);
    context->RSSetScissorRects(s.scissor_count, s.scissors);

    release_array(reinterpret_cast<IUnknown* const*>(s.targets),
                  D3D11_SIMULTANEOUS_RENDER_TARGET_COUNT);
    IUnknown* const singles[] = {
        s.depth_view, s.vertex_shader, s.pixel_shader, s.layout, s.resource,
        s.sampler,    s.constants,     s.blend,        s.depth_state, s.raster,
    };
    release_array(singles, sizeof(singles) / sizeof(singles[0]));
    std::memset(storage, 0, sizeof(*storage));
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
