// SPDX-License-Identifier: GPL-3.0-only

#include <rescaleframe/d3d11_state.h>

#include <windows.h>

#include <d3d11.h>

#include <cstring>
#include <new>

namespace {

// How much of each stage is kept. D3D11 allows 128 shader resource slots and 16 samplers per
// stage; Unreal's D3D11 backend and the vendor runtimes here stay inside the low ones, and saving
// all 128 across three stages would be 384 reference counts for slots nothing ever binds.
//
// Sixteen is not a guess about Unreal. It is the count the D3D11 constants themselves name for
// samplers, so a stage whose samplers all fit is a stage whose interesting textures do too. Where
// that turns out to be wrong the symptom is a texture the game rebinds anyway, because a pass that
// reaches past slot 16 sets its own bindings up.
constexpr UINT saved_resources = 16;
constexpr UINT saved_samplers = D3D11_COMMONSHADER_SAMPLER_SLOT_COUNT;
constexpr UINT saved_constants = D3D11_COMMONSHADER_CONSTANT_BUFFER_API_SLOT_COUNT;
constexpr UINT saved_vertex_buffers = 8;
constexpr UINT saved_uavs = D3D11_PS_CS_UAV_REGISTER_COUNT;
constexpr UINT saved_viewports = D3D11_VIEWPORT_AND_SCISSORRECT_OBJECT_COUNT_PER_PIPELINE;

// One programmable stage's bindings. The three this fills are the ones anything here can disturb:
// the vertex and pixel stages because a full screen pass uses them, and the compute stage because
// that is what a vendor reconstruction actually runs.
struct Stage {
    ID3D11ShaderResourceView* resources[saved_resources];
    ID3D11SamplerState* samplers[saved_samplers];
    ID3D11Buffer* constants[saved_constants];
};

struct State {
    // Input assembler.
    ID3D11InputLayout* layout;
    D3D11_PRIMITIVE_TOPOLOGY topology;
    ID3D11Buffer* index_buffer;
    DXGI_FORMAT index_format;
    UINT index_offset;
    ID3D11Buffer* vertex_buffers[saved_vertex_buffers];
    UINT vertex_strides[saved_vertex_buffers];
    UINT vertex_offsets[saved_vertex_buffers];

    // Shaders. Class instances are deliberately not saved: asking for them means providing an
    // array and a count for each stage, and nothing in this project or in Unreal's D3D11 backend
    // uses shader linkage.
    ID3D11VertexShader* vertex_shader;
    ID3D11HullShader* hull_shader;
    ID3D11DomainShader* domain_shader;
    ID3D11GeometryShader* geometry_shader;
    ID3D11PixelShader* pixel_shader;
    ID3D11ComputeShader* compute_shader;

    Stage vertex;
    Stage pixel;
    Stage compute;
    ID3D11UnorderedAccessView* compute_uavs[saved_uavs];

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

static_assert(sizeof(State) <= RSF_D3D11_STATE_BYTES,
              "rsf_d3d11_state is too small for the state it has to hold");
static_assert(alignof(State) <= alignof(uint64_t), "rsf_d3d11_state is not aligned for the state");

State& state_of(rsf_d3d11_state* state)
{
    return *reinterpret_cast<State*>(state->opaque);
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
}

} // namespace

extern "C" uint32_t rsf_d3d11_state_save(void* context_pointer, rsf_d3d11_state* state_pointer)
{
    if (!context_pointer || !state_pointer) {
        return 0;
    }
    auto* context = static_cast<ID3D11DeviceContext*>(context_pointer);
    // Zeroed first. Every Get below writes each slot it is asked about, but a partial failure or a
    // future field added without a Get would otherwise leave a stack pointer to be released.
    std::memset(state_pointer, 0, sizeof(*state_pointer));
    State& state = *new (state_pointer->opaque) State{};

    context->IAGetInputLayout(&state.layout);
    context->IAGetPrimitiveTopology(&state.topology);
    context->IAGetIndexBuffer(&state.index_buffer, &state.index_format, &state.index_offset);
    context->IAGetVertexBuffers(0, saved_vertex_buffers, state.vertex_buffers,
                                state.vertex_strides, state.vertex_offsets);

    context->VSGetShader(&state.vertex_shader, nullptr, nullptr);
    context->HSGetShader(&state.hull_shader, nullptr, nullptr);
    context->DSGetShader(&state.domain_shader, nullptr, nullptr);
    context->GSGetShader(&state.geometry_shader, nullptr, nullptr);
    context->PSGetShader(&state.pixel_shader, nullptr, nullptr);
    context->CSGetShader(&state.compute_shader, nullptr, nullptr);

    context->VSGetShaderResources(0, saved_resources, state.vertex.resources);
    context->VSGetSamplers(0, saved_samplers, state.vertex.samplers);
    context->VSGetConstantBuffers(0, saved_constants, state.vertex.constants);
    context->PSGetShaderResources(0, saved_resources, state.pixel.resources);
    context->PSGetSamplers(0, saved_samplers, state.pixel.samplers);
    context->PSGetConstantBuffers(0, saved_constants, state.pixel.constants);
    context->CSGetShaderResources(0, saved_resources, state.compute.resources);
    context->CSGetSamplers(0, saved_samplers, state.compute.samplers);
    context->CSGetConstantBuffers(0, saved_constants, state.compute.constants);
    context->CSGetUnorderedAccessViews(0, saved_uavs, state.compute_uavs);

    context->RSGetState(&state.raster);
    state.viewport_count = saved_viewports;
    context->RSGetViewports(&state.viewport_count, state.viewports);
    state.scissor_count = saved_viewports;
    context->RSGetScissorRects(&state.scissor_count, state.scissors);

    context->OMGetRenderTargets(D3D11_SIMULTANEOUS_RENDER_TARGET_COUNT, state.targets,
                                &state.depth_view);
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
    State& state = state_of(state_pointer);

    // Inputs first, outputs after. A texture that is still bound as a shader resource when it is
    // bound as a render target is silently unbound by the runtime, and the binding that loses is
    // the one that arrived first, so restoring the targets before the resources would quietly drop
    // exactly the render target this is trying to hand back.
    ID3D11ShaderResourceView* no_resources[saved_resources] = {};
    ID3D11UnorderedAccessView* no_uavs[saved_uavs] = {};
    UINT no_counts[saved_uavs] = {};
    context->VSSetShaderResources(0, saved_resources, no_resources);
    context->PSSetShaderResources(0, saved_resources, no_resources);
    context->CSSetShaderResources(0, saved_resources, no_resources);
    context->CSSetUnorderedAccessViews(0, saved_uavs, no_uavs, no_counts);

    context->OMSetRenderTargets(D3D11_SIMULTANEOUS_RENDER_TARGET_COUNT, state.targets,
                                state.depth_view);
    context->OMSetBlendState(state.blend, state.blend_factor, state.blend_mask);
    context->OMSetDepthStencilState(state.depth_state, state.stencil_reference);

    context->VSSetShaderResources(0, saved_resources, state.vertex.resources);
    context->VSSetSamplers(0, saved_samplers, state.vertex.samplers);
    context->VSSetConstantBuffers(0, saved_constants, state.vertex.constants);
    context->PSSetShaderResources(0, saved_resources, state.pixel.resources);
    context->PSSetSamplers(0, saved_samplers, state.pixel.samplers);
    context->PSSetConstantBuffers(0, saved_constants, state.pixel.constants);
    context->CSSetShaderResources(0, saved_resources, state.compute.resources);
    context->CSSetSamplers(0, saved_samplers, state.compute.samplers);
    context->CSSetConstantBuffers(0, saved_constants, state.compute.constants);
    context->CSSetUnorderedAccessViews(0, saved_uavs, state.compute_uavs, no_counts);

    context->VSSetShader(state.vertex_shader, nullptr, 0);
    context->HSSetShader(state.hull_shader, nullptr, 0);
    context->DSSetShader(state.domain_shader, nullptr, 0);
    context->GSSetShader(state.geometry_shader, nullptr, 0);
    context->PSSetShader(state.pixel_shader, nullptr, 0);
    context->CSSetShader(state.compute_shader, nullptr, 0);

    context->IASetInputLayout(state.layout);
    context->IASetPrimitiveTopology(state.topology);
    context->IASetIndexBuffer(state.index_buffer, state.index_format, state.index_offset);
    context->IASetVertexBuffers(0, saved_vertex_buffers, state.vertex_buffers,
                                state.vertex_strides, state.vertex_offsets);

    context->RSSetState(state.raster);
    if (state.viewport_count > 0) {
        context->RSSetViewports(state.viewport_count, state.viewports);
    }
    if (state.scissor_count > 0) {
        context->RSSetScissorRects(state.scissor_count, state.scissors);
    }

    release_array(reinterpret_cast<IUnknown* const*>(state.vertex_buffers), saved_vertex_buffers);
    release_stage(state.vertex);
    release_stage(state.pixel);
    release_stage(state.compute);
    release_array(reinterpret_cast<IUnknown* const*>(state.compute_uavs), saved_uavs);
    release_array(reinterpret_cast<IUnknown* const*>(state.targets),
                  D3D11_SIMULTANEOUS_RENDER_TARGET_COUNT);

    IUnknown* const singles[] = {
        state.layout,          state.index_buffer,    state.vertex_shader,  state.hull_shader,
        state.domain_shader,   state.geometry_shader, state.pixel_shader,   state.compute_shader,
        state.raster,          state.depth_view,      state.blend,          state.depth_state,
    };
    release_array(singles, sizeof(singles) / sizeof(singles[0]));

    // Zeroed so a second restore of the same state releases nothing. It is a caller mistake either
    // way, and one that leaves the game short a reference on every resource it had bound is the
    // kind that surfaces minutes later as a use after free somewhere else entirely.
    std::memset(state_pointer, 0, sizeof(*state_pointer));
}
