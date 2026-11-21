// SPDX-License-Identifier: MIT

#include "device_owner.h"
#include "overlay_renderer_platform.h"
#include <rescaleframe/log.h>
#include <rescaleframe/shader_compile.h>

#include <windows.h>

#include <d3d11.h>
#include <wrl/client.h>

#include <cstddef>
#include <cstring>
#include <vector>

namespace {

// Kept as source and compiled at load rather than shipped as bytecode, for the same reason as the
// motion decode pass: the cross build has no shader compiler, so a pass built from bytecode would
// exist only in the MSVC build and could not be built or read anywhere else. d3dcompiler_47 is
// present both on Windows and in a Proton prefix.
//
// Compiled as 4_0 rather than 5_0. Nothing here needs shader model 5, and 4_0 also creates on a
// feature level 10 device, which costs nothing and removes a way for this to fail on hardware
// nobody tested it on.
const char* const overlay_shader = R"(
cbuffer Params : register(b0)
{
    float2 InverseTargetSize;
    float2 Padding;
};

struct VertexIn
{
    float2 position : POSITION;
    float2 uv       : TEXCOORD0;
    float4 color    : COLOR0;
};

struct VertexOut
{
    float4 position : SV_Position;
    float2 uv       : TEXCOORD0;
    float4 color    : COLOR0;
};

VertexOut vertex_main(VertexIn input)
{
    VertexOut output;

    // Physical pixels to clip space, using the target size the caller passed rather than a
    // projection matrix. y is negated because pixel coordinates grow downwards and clip space
    // upwards.
    output.position = float4(input.position.x * InverseTargetSize.x - 1.0,
                             1.0 - input.position.y * InverseTargetSize.y,
                             0.0, 1.0);
    output.uv = input.uv;
    output.color = input.color;
    return output;
}

Texture2D    Atlas        : register(t0);
SamplerState AtlasSampler : register(s0);

float4 pixel_main(VertexOut input) : SV_Target
{
    // Both the vertex colour and the atlas are premultiplied, so a component wise product is
    // premultiplied again and the blend state is the only remaining half of it.
    return input.color * Atlas.Sample(AtlasSampler, input.uv);
}
)";

struct Constants {
    // 2 / width and 2 / height. The subtraction and the flip live in the shader.
    float inverse_target_size[2];
    float padding[2];
};
static_assert(sizeof(Constants) % 16 == 0, "constant buffers are bound in 16 byte registers");

static_assert(sizeof(rsf_overlay_vertex) == 20, "the input layout below describes this exact size");
static_assert(offsetof(rsf_overlay_vertex, u) == 8, "input layout offset");
static_assert(offsetof(rsf_overlay_vertex, color) == 16, "input layout offset");

constexpr uint32_t max_class_instances = D3D11_SHADER_MAX_INTERFACES;
constexpr uint32_t max_viewports = D3D11_VIEWPORT_AND_SCISSORRECT_OBJECT_COUNT_PER_PIPELINE;

} // namespace

// The caller keeps the render targets, depth view and UAVs bound throughout the draw.
// Each context getter transfers its COM references to the Rust overlay renderer.
struct rsf_overlay11_snapshot {
    ID3D11Predicate* predicate;
    BOOL predicate_value;
    ID3D11InputLayout* input_layout;
    D3D11_PRIMITIVE_TOPOLOGY topology;
    ID3D11Buffer* vertex_buffer;
    UINT vertex_stride;
    UINT vertex_offset;
    ID3D11Buffer* index_buffer;
    DXGI_FORMAT index_format;
    UINT index_offset;

    ID3D11VertexShader* vertex_shader;
    ID3D11ClassInstance* vertex_instances[max_class_instances];
    UINT vertex_instance_count;
    ID3D11Buffer* vertex_constants;

    ID3D11PixelShader* pixel_shader;
    ID3D11ClassInstance* pixel_instances[max_class_instances];
    UINT pixel_instance_count;
    ID3D11ShaderResourceView* pixel_resource;
    ID3D11SamplerState* pixel_sampler;

    // A geometry, hull or domain shader left bound would take part in our draw and turn it into
    // something else, so these are cleared as well as restored.
    ID3D11GeometryShader* geometry_shader;
    ID3D11ClassInstance* geometry_instances[max_class_instances];
    UINT geometry_instance_count;
    ID3D11HullShader* hull_shader;
    ID3D11ClassInstance* hull_instances[max_class_instances];
    UINT hull_instance_count;
    ID3D11DomainShader* domain_shader;
    ID3D11ClassInstance* domain_instances[max_class_instances];
    UINT domain_instance_count;

    ID3D11RasterizerState* rasterizer;
    D3D11_VIEWPORT viewports[max_viewports];
    UINT viewport_count;
    D3D11_RECT scissors[max_viewports];
    UINT scissor_count;

    ID3D11BlendState* blend;
    FLOAT blend_factor[4];
    UINT sample_mask;
    ID3D11DepthStencilState* depth_stencil;
    UINT stencil_reference;
};

static_assert(sizeof(D3D11_PRIMITIVE_TOPOLOGY) == 4 && sizeof(DXGI_FORMAT) == 4);
static_assert(sizeof(D3D11_VIEWPORT) == 24 && sizeof(D3D11_RECT) == 16);
static_assert(sizeof(rsf_overlay11_snapshot) == 10992 && alignof(rsf_overlay11_snapshot) == 8);
static_assert(offsetof(rsf_overlay11_snapshot, vertex_instances) == 72);
static_assert(offsetof(rsf_overlay11_snapshot, pixel_instances) == 2120);
static_assert(offsetof(rsf_overlay11_snapshot, geometry_instances) == 4176);
static_assert(offsetof(rsf_overlay11_snapshot, hull_instances) == 6216);
static_assert(offsetof(rsf_overlay11_snapshot, domain_instances) == 8256);
static_assert(offsetof(rsf_overlay11_snapshot, viewports) == 10296);
static_assert(offsetof(rsf_overlay11_snapshot, scissors) == 10684);
static_assert(offsetof(rsf_overlay11_snapshot, blend) == 10944);

namespace {

int32_t read_state(void* raw, rsf_overlay11_snapshot* snapshot) noexcept try
{
    auto* context = static_cast<ID3D11DeviceContext*>(raw);
    auto& state = *snapshot;
    state.vertex_instance_count = max_class_instances;
    state.pixel_instance_count = max_class_instances;
    state.geometry_instance_count = max_class_instances;
    state.hull_instance_count = max_class_instances;
    state.domain_instance_count = max_class_instances;
    state.viewport_count = max_viewports;
    state.scissor_count = max_viewports;

    context->IAGetInputLayout(&state.input_layout);
    context->IAGetPrimitiveTopology(&state.topology);
    context->IAGetVertexBuffers(0, 1, &state.vertex_buffer, &state.vertex_stride,
                                &state.vertex_offset);
    context->IAGetIndexBuffer(&state.index_buffer, &state.index_format, &state.index_offset);

    context->VSGetShader(&state.vertex_shader, state.vertex_instances,
                         &state.vertex_instance_count);
    context->VSGetConstantBuffers(0, 1, &state.vertex_constants);

    context->PSGetShader(&state.pixel_shader, state.pixel_instances, &state.pixel_instance_count);
    context->PSGetShaderResources(0, 1, &state.pixel_resource);
    context->PSGetSamplers(0, 1, &state.pixel_sampler);

    context->GSGetShader(&state.geometry_shader, state.geometry_instances,
                         &state.geometry_instance_count);
    context->HSGetShader(&state.hull_shader, state.hull_instances, &state.hull_instance_count);
    context->DSGetShader(&state.domain_shader, state.domain_instances,
                         &state.domain_instance_count);

    context->RSGetState(&state.rasterizer);
    context->RSGetViewports(&state.viewport_count, state.viewports);
    context->RSGetScissorRects(&state.scissor_count, state.scissors);

    context->OMGetBlendState(&state.blend, state.blend_factor, &state.sample_mask);
    context->OMGetDepthStencilState(&state.depth_stencil, &state.stencil_reference);
    context->GetPredication(&state.predicate, &state.predicate_value);
    return 1;
} catch (...) { return 0; }

void restore_state(void* raw, const rsf_overlay11_snapshot* snapshot) noexcept
{
    auto* context = static_cast<ID3D11DeviceContext*>(raw);
    const auto& state = *snapshot;
    context->IASetInputLayout(state.input_layout);
    context->IASetPrimitiveTopology(state.topology);
    context->IASetVertexBuffers(0, 1, &state.vertex_buffer, &state.vertex_stride,
                                &state.vertex_offset);
    context->IASetIndexBuffer(state.index_buffer, state.index_format, state.index_offset);

    context->VSSetShader(state.vertex_shader, state.vertex_instances, state.vertex_instance_count);
    context->VSSetConstantBuffers(0, 1, &state.vertex_constants);

    context->PSSetShader(state.pixel_shader, state.pixel_instances, state.pixel_instance_count);
    context->PSSetShaderResources(0, 1, &state.pixel_resource);
    context->PSSetSamplers(0, 1, &state.pixel_sampler);

    context->GSSetShader(state.geometry_shader, state.geometry_instances,
                         state.geometry_instance_count);
    context->HSSetShader(state.hull_shader, state.hull_instances, state.hull_instance_count);
    context->DSSetShader(state.domain_shader, state.domain_instances, state.domain_instance_count);

    context->RSSetState(state.rasterizer);
    context->RSSetViewports(state.viewport_count, state.viewports);
    context->RSSetScissorRects(state.scissor_count, state.scissors);

    context->OMSetBlendState(state.blend, state.blend_factor, state.sample_mask);
    context->OMSetDepthStencilState(state.depth_stencil, state.stencil_reference);
    context->SetPredication(state.predicate, state.predicate_value);
}

bool is_srgb_format(DXGI_FORMAT format)
{
    return format == DXGI_FORMAT_R8G8B8A8_UNORM_SRGB || format == DXGI_FORMAT_B8G8R8A8_UNORM_SRGB ||
           format == DXGI_FORMAT_B8G8R8X8_UNORM_SRGB;
}

int32_t make_pipeline(void* raw, const rsf_overlay_renderer_setup* setup,
    rsf_overlay11_pipeline* renderer) noexcept try
{
    auto* device = static_cast<ID3D11Device*>(raw);
    // Flags 0 and shader model 4_0, as before: see the note on overlay_shader.
    std::vector<uint8_t> vertex_bytecode;
    std::vector<uint8_t> pixel_bytecode;
    if (!rsf::compile_shader(overlay_shader, "overlay_renderer", "vertex_main", "vs_4_0",
                             vertex_bytecode, setup->log, setup->log_user, 0) ||
        !rsf::compile_shader(overlay_shader, "overlay_renderer", "pixel_main", "ps_4_0",
                             pixel_bytecode, setup->log, setup->log_user, 0)) {
        return RSF_OVERLAY_RENDERER_ERROR_SHADER_FAILED;
    }

    const HRESULT made_vertex_shader =
        device->CreateVertexShader(vertex_bytecode.data(), vertex_bytecode.size(), nullptr,
                                   &renderer->vertex_shader);
    const HRESULT made_pixel_shader =
        device->CreatePixelShader(pixel_bytecode.data(), pixel_bytecode.size(), nullptr,
                                  &renderer->pixel_shader);
    if (FAILED(made_vertex_shader) || FAILED(made_pixel_shader) || !renderer->vertex_shader ||
        !renderer->pixel_shader) {
        rsf::say(setup->log, setup->log_user, "overlay shaders could not be created");
        return RSF_OVERLAY_RENDERER_ERROR_SHADER_FAILED;
    }

    // egui's vertex, exactly as `rsf_overlay_vertex` lays it out. The colour is premultiplied and
    // already encoded the way the target expects, so it is read as UNORM bytes and not converted.
    const D3D11_INPUT_ELEMENT_DESC elements[] = {
        {"POSITION", 0, DXGI_FORMAT_R32G32_FLOAT, 0, 0, D3D11_INPUT_PER_VERTEX_DATA, 0},
        {"TEXCOORD", 0, DXGI_FORMAT_R32G32_FLOAT, 0, 8, D3D11_INPUT_PER_VERTEX_DATA, 0},
        {"COLOR", 0, DXGI_FORMAT_R8G8B8A8_UNORM, 0, 16, D3D11_INPUT_PER_VERTEX_DATA, 0},
    };
    const HRESULT made_layout = device->CreateInputLayout(
        elements, 3, vertex_bytecode.data(), vertex_bytecode.size(), &renderer->input_layout);
    if (FAILED(made_layout) || !renderer->input_layout) {
        rsf::say(setup->log, setup->log_user, "overlay input layout could not be created");
        return RSF_OVERLAY_RENDERER_ERROR_RESOURCE_FAILED;
    }

    D3D11_BUFFER_DESC constants{};
    constants.ByteWidth = sizeof(Constants);
    constants.Usage = D3D11_USAGE_DYNAMIC;
    constants.BindFlags = D3D11_BIND_CONSTANT_BUFFER;
    constants.CPUAccessFlags = D3D11_CPU_ACCESS_WRITE;
    if (FAILED(device->CreateBuffer(&constants, nullptr, &renderer->constants)) ||
        !renderer->constants) {
        rsf::say(setup->log, setup->log_user, "overlay constant buffer could not be created");
        return RSF_OVERLAY_RENDERER_ERROR_RESOURCE_FAILED;
    }

    // Premultiplied alpha. Straight alpha here would be wrong on every antialiased edge and every
    // glyph, in a way that reads as a font problem rather than as a blend problem.
    D3D11_BLEND_DESC blend{};
    blend.RenderTarget[0].BlendEnable = TRUE;
    blend.RenderTarget[0].SrcBlend = D3D11_BLEND_ONE;
    blend.RenderTarget[0].DestBlend = D3D11_BLEND_INV_SRC_ALPHA;
    blend.RenderTarget[0].BlendOp = D3D11_BLEND_OP_ADD;
    blend.RenderTarget[0].SrcBlendAlpha = D3D11_BLEND_ONE;
    blend.RenderTarget[0].DestBlendAlpha = D3D11_BLEND_INV_SRC_ALPHA;
    blend.RenderTarget[0].BlendOpAlpha = D3D11_BLEND_OP_ADD;
    blend.RenderTarget[0].RenderTargetWriteMask = D3D11_COLOR_WRITE_ENABLE_ALL;
    if (FAILED(device->CreateBlendState(&blend, &renderer->blend)) || !renderer->blend) {
        rsf::say(setup->log, setup->log_user, "overlay blend state could not be created");
        return RSF_OVERLAY_RENDERER_ERROR_RESOURCE_FAILED;
    }

    // The overlay sits on top of a finished image. It reads no depth and writes none, so whatever
    // depth buffer the game has is neither consulted nor damaged.
    //
    // The stencil fields are filled in even though stencil is off. Zero is not a member of
    // D3D11_STENCIL_OP or D3D11_COMPARISON_FUNC, and D3D11 range checks the descriptor it is
    // handed: there are message ids for an invalid FrontFace.StencilFailOp and friends, and the
    // CD3D11_DEPTH_STENCIL_DESC default in d3d11.h writes all of these with StencilEnable FALSE.
    // Whether the runtime really rejects a zeroed pair here could not be tried without a device,
    // so these are set to the documented defaults, which is correct either way.
    const D3D11_DEPTH_STENCILOP_DESC stencil_unused = {D3D11_STENCIL_OP_KEEP, D3D11_STENCIL_OP_KEEP,
                                                       D3D11_STENCIL_OP_KEEP,
                                                       D3D11_COMPARISON_ALWAYS};
    D3D11_DEPTH_STENCIL_DESC depth_stencil{};
    depth_stencil.DepthEnable = FALSE;
    depth_stencil.DepthWriteMask = D3D11_DEPTH_WRITE_MASK_ZERO;
    depth_stencil.DepthFunc = D3D11_COMPARISON_ALWAYS;
    depth_stencil.StencilEnable = FALSE;
    depth_stencil.StencilReadMask = D3D11_DEFAULT_STENCIL_READ_MASK;
    depth_stencil.StencilWriteMask = D3D11_DEFAULT_STENCIL_WRITE_MASK;
    depth_stencil.FrontFace = stencil_unused;
    depth_stencil.BackFace = stencil_unused;
    if (FAILED(device->CreateDepthStencilState(&depth_stencil, &renderer->depth_stencil)) ||
        !renderer->depth_stencil) {
        rsf::say(setup->log, setup->log_user, "overlay depth stencil state could not be created");
        return RSF_OVERLAY_RENDERER_ERROR_RESOURCE_FAILED;
    }

    // Culling off because egui does not promise a winding order, and scissoring on because every
    // draw call carries a clip rectangle that egui relies on.
    D3D11_RASTERIZER_DESC rasterizer{};
    rasterizer.FillMode = D3D11_FILL_SOLID;
    rasterizer.CullMode = D3D11_CULL_NONE;
    rasterizer.FrontCounterClockwise = FALSE;
    rasterizer.DepthClipEnable = TRUE;
    rasterizer.ScissorEnable = TRUE;
    rasterizer.MultisampleEnable = FALSE;
    rasterizer.AntialiasedLineEnable = FALSE;
    if (FAILED(device->CreateRasterizerState(&rasterizer, &renderer->rasterizer)) ||
        !renderer->rasterizer) {
        rsf::say(setup->log, setup->log_user, "overlay rasterizer state could not be created");
        return RSF_OVERLAY_RENDERER_ERROR_RESOURCE_FAILED;
    }

    D3D11_SAMPLER_DESC sampler{};
    sampler.Filter = D3D11_FILTER_MIN_MAG_MIP_LINEAR;
    sampler.AddressU = D3D11_TEXTURE_ADDRESS_CLAMP;
    sampler.AddressV = D3D11_TEXTURE_ADDRESS_CLAMP;
    sampler.AddressW = D3D11_TEXTURE_ADDRESS_CLAMP;
    sampler.ComparisonFunc = D3D11_COMPARISON_ALWAYS;
    sampler.MaxLOD = D3D11_FLOAT32_MAX;
    if (FAILED(device->CreateSamplerState(&sampler, &renderer->sampler)) || !renderer->sampler) {
        rsf::say(setup->log, setup->log_user, "overlay sampler could not be created");
        return RSF_OVERLAY_RENDERER_ERROR_RESOURCE_FAILED;
    }

    return RSF_OVERLAY_RENDERER_OK;
} catch (...) { return RSF_OVERLAY_RENDERER_ERROR_RESOURCE_FAILED; }

int32_t validate_context(void* device, void* raw)
{
    auto* context = static_cast<ID3D11DeviceContext*>(raw);
    return context && context->GetType() == D3D11_DEVICE_CONTEXT_IMMEDIATE &&
        rsf::same_device(context, static_cast<ID3D11Device*>(device));
}

void* make_texture(void* raw, const rsf_overlay_texture_update* update, uint32_t row_pitch)
{
    auto* device = static_cast<ID3D11Device*>(raw);
    D3D11_TEXTURE2D_DESC desc{};
    desc.Width = update->width;
    desc.Height = update->height;
    desc.MipLevels = 1;
    desc.ArraySize = 1;
    desc.Format = DXGI_FORMAT_R8G8B8A8_UNORM;
    desc.SampleDesc.Count = 1;
    desc.Usage = D3D11_USAGE_DEFAULT;
    desc.BindFlags = D3D11_BIND_SHADER_RESOURCE;
    const D3D11_SUBRESOURCE_DATA initial{update->pixels, row_pitch, 0};
    Microsoft::WRL::ComPtr<ID3D11Texture2D> texture;
    return SUCCEEDED(device->CreateTexture2D(&desc, &initial, &texture)) ? texture.Detach() : nullptr;
}

void* make_view(void* device, void* texture)
{
    Microsoft::WRL::ComPtr<ID3D11ShaderResourceView> view;
    return SUCCEEDED(static_cast<ID3D11Device*>(device)->CreateShaderResourceView(
        static_cast<ID3D11Texture2D*>(texture), nullptr, &view)) ? view.Detach() : nullptr;
}

void patch_texture(void* context, void* texture, const rsf_overlay_texture_update* update, uint32_t row_pitch)
{
    const D3D11_BOX box{update->x, update->y, 0, update->x + update->width, update->y + update->height, 1};
    static_cast<ID3D11DeviceContext*>(context)->UpdateSubresource(
        static_cast<ID3D11Texture2D*>(texture), 0, &box, update->pixels, row_pitch, row_pitch * update->height);
}

void* make_buffer(void* device, uint32_t bytes, uint32_t bind)
{
    D3D11_BUFFER_DESC desc{};
    desc.ByteWidth = bytes;
    desc.Usage = D3D11_USAGE_DYNAMIC;
    desc.BindFlags = bind;
    desc.CPUAccessFlags = D3D11_CPU_ACCESS_WRITE;
    Microsoft::WRL::ComPtr<ID3D11Buffer> buffer;
    return SUCCEEDED(static_cast<ID3D11Device*>(device)->CreateBuffer(&desc, nullptr, &buffer)) ? buffer.Detach() : nullptr;
}

int32_t write_buffer(void* raw, void* buffer, const void* bytes, size_t length)
{
    auto* context = static_cast<ID3D11DeviceContext*>(raw);
    auto* resource = static_cast<ID3D11Buffer*>(buffer);
    D3D11_MAPPED_SUBRESOURCE mapped{};
    if (FAILED(context->Map(resource, 0, D3D11_MAP_WRITE_DISCARD, 0, &mapped))) return 0;
    std::memcpy(mapped.pData, bytes, length);
    context->Unmap(resource, 0);
    return 1;
}

uint32_t target_kind(void* raw)
{
    ID3D11RenderTargetView* target = nullptr;
    static_cast<ID3D11DeviceContext*>(raw)->OMGetRenderTargets(1, &target, nullptr);
    if (!target) return 0;
    D3D11_RENDER_TARGET_VIEW_DESC desc{};
    target->GetDesc(&desc);
    target->Release();
    return is_srgb_format(desc.Format) ? 2u : 1u;
}

int32_t draw(void* raw, const rsf_overlay11_pipeline* pipeline, void* vertices, void* indices,
    uint32_t width, uint32_t height, const rsf_overlay11_draw* draws, size_t count) noexcept try
{
    auto* context = static_cast<ID3D11DeviceContext*>(raw);

    auto* vertex_buffer = static_cast<ID3D11Buffer*>(vertices);
    auto* index_buffer = static_cast<ID3D11Buffer*>(indices);
    const UINT stride = sizeof(rsf_overlay_vertex);
    const UINT offset = 0;
    context->SetPredication(nullptr, FALSE);
    context->IASetInputLayout(pipeline->input_layout);
    context->IASetPrimitiveTopology(D3D11_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
    context->IASetVertexBuffers(0, 1, &vertex_buffer, &stride, &offset);
    context->IASetIndexBuffer(index_buffer, DXGI_FORMAT_R32_UINT, 0);
    context->VSSetShader(pipeline->vertex_shader, nullptr, 0);
    context->VSSetConstantBuffers(0, 1, &pipeline->constants);
    context->PSSetShader(pipeline->pixel_shader, nullptr, 0);
    context->PSSetSamplers(0, 1, &pipeline->sampler);
    context->GSSetShader(nullptr, nullptr, 0);
    context->HSSetShader(nullptr, nullptr, 0);
    context->DSSetShader(nullptr, nullptr, 0);

    const FLOAT blend_factor[4]{};
    context->OMSetBlendState(pipeline->blend, blend_factor, UINT32_MAX);
    context->OMSetDepthStencilState(pipeline->depth_stencil, 0);
    context->RSSetState(pipeline->rasterizer);
    const D3D11_VIEWPORT viewport{0.0f, 0.0f, float(width), float(height), 0.0f, 1.0f};
    context->RSSetViewports(1, &viewport);
    for (size_t index = 0; index < count; ++index) {
        const auto& call = draws[index];
        const D3D11_RECT clip{call.clip[0], call.clip[1], call.clip[2], call.clip[3]};
        auto* view = static_cast<ID3D11ShaderResourceView*>(call.view);
        context->RSSetScissorRects(1, &clip);
        context->PSSetShaderResources(0, 1, &view);
        context->DrawIndexed(call.index_count, call.index_offset, call.vertex_offset);
    }

    // Do not leave an atlas bound when restoring the game's shader bindings.
    ID3D11ShaderResourceView* null_view = nullptr;
    context->PSSetShaderResources(0, 1, &null_view);
    return 1;
} catch (...) { return 0; }

constexpr rsf_overlay11_platform platform{
    make_pipeline, validate_context, make_texture, make_view, patch_texture, make_buffer, write_buffer,
    target_kind, read_state, draw, restore_state,
    [](rsf_overlay_renderer_log_fn log, void* user, const char* message) noexcept {
        try { log(user, message); } catch (...) {}
    },
};

static_assert(sizeof(rsf_overlay11_pipeline) == 8 * sizeof(void*) && sizeof(rsf_overlay11_draw) == 40);
static_assert(sizeof(rsf_overlay11_platform) == 12 * sizeof(void*) && sizeof(rsf_overlay_renderer_setup) == 24);
static_assert(D3D11_BIND_VERTEX_BUFFER == 1 && D3D11_BIND_INDEX_BUFFER == 2 && D3D11_REQ_TEXTURE2D_U_OR_V_DIMENSION == 16384);
static_assert(RSF_OVERLAY_RENDERER_ABI_VERSION == 1 && RSF_OVERLAY_RENDERER_OK == 0 &&
    RSF_OVERLAY_RENDERER_ERROR_INVALID_ARGUMENT == -1 && RSF_OVERLAY_RENDERER_ERROR_ABI_MISMATCH == -2 &&
    RSF_OVERLAY_RENDERER_ERROR_RESOURCE_FAILED == -4);

} // namespace

extern "C" rsf_overlay_renderer_result rsf_overlay_renderer_create(
    void* device, const rsf_overlay_renderer_setup* setup, rsf_overlay_renderer** out)
{
    return rsf_core_overlay11_create(device, setup, out, &platform);
}

extern "C" rsf_overlay_renderer_result rsf_overlay_renderer_upload_texture(
    rsf_overlay_renderer* renderer, void* context, const rsf_overlay_texture_update* update)
{
    return rsf_core_overlay11_upload(renderer, context, update);
}

extern "C" void rsf_overlay_renderer_free_texture(rsf_overlay_renderer* renderer, uint64_t id)
{
    rsf_core_overlay11_free(renderer, id);
}

extern "C" rsf_overlay_renderer_result rsf_overlay_renderer_draw(
    rsf_overlay_renderer* renderer, void* context, const rsf_overlay_draw_data* data, uint32_t width, uint32_t height)
{
    return rsf_core_overlay11_draw(renderer, context, data, width, height);
}

extern "C" void rsf_overlay_renderer_destroy(rsf_overlay_renderer* renderer)
{
    rsf_core_overlay11_destroy(renderer);
}
