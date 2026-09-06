// SPDX-License-Identifier: GPL-3.0-only

#include <rescaleframe/overlay_renderer.h>

#include <windows.h>

#include <d3d11.h>

#include <cstdarg>
#include <cstddef>
#include <cstdio>
#include <cstring>
#include <new>
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

using compile_fn = HRESULT(WINAPI*)(LPCVOID, SIZE_T, LPCSTR, const D3D_SHADER_MACRO*, void*, LPCSTR,
                                    LPCSTR, UINT, UINT, ID3DBlob**, ID3DBlob**);

// Ceilings on one frame's geometry. egui is nowhere near these; they exist so that a count that
// arrived wrong is refused rather than turned into an allocation the size of the address space.
constexpr uint32_t max_vertices = 1u << 22;
constexpr uint32_t max_indices = 1u << 23;

// Buffers grow in whole blocks so that a panel opening does not recreate them on every frame it
// grows by a few triangles.
constexpr uint32_t growth_block = 4096;

constexpr uint32_t max_class_instances = D3D11_SHADER_MAX_INTERFACES;
constexpr uint32_t max_viewports = D3D11_VIEWPORT_AND_SCISSORRECT_OBJECT_COUNT_PER_PIPELINE;

struct Texture {
    uint64_t id;
    ID3D11Texture2D* texture;
    ID3D11ShaderResourceView* view;
    uint32_t width;
    uint32_t height;
};

// Everything this pass touches on the context. The game did not ask for any of it to change, and
// what is left changed corrupts the game's own rendering after we return, which presents as the
// game breaking rather than as the overlay breaking.
struct SavedState {
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

    // Render targets are deliberately absent. This pass draws into whatever is bound and never
    // rebinds it, because putting render targets back means calling OMSetRenderTargets, and that
    // call also unbinds every unordered access view the output merger holds. Their original start
    // slot is not something OMGetRenderTargetsAndUnorderedAccessViews reports, so a game that had
    // any bound could not be put back exactly. Not touching them is the only safe answer, and it
    // costs nothing: the caller has already bound the image the overlay belongs on.
};

void release_instances(ID3D11ClassInstance** instances, UINT count)
{
    for (UINT index = 0; index < count; ++index) {
        if (instances[index]) {
            instances[index]->Release();
        }
    }
}

void save_state(ID3D11DeviceContext* context, SavedState& state)
{
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
}

// Puts the context back and drops every reference the Get calls above handed over. Those are real
// references and this runs every frame, so a missed Release is a leak that grows with playtime.
void restore_state(ID3D11DeviceContext* context, SavedState& state)
{
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

    if (state.input_layout) {
        state.input_layout->Release();
    }
    if (state.vertex_buffer) {
        state.vertex_buffer->Release();
    }
    if (state.index_buffer) {
        state.index_buffer->Release();
    }
    if (state.vertex_shader) {
        state.vertex_shader->Release();
    }
    release_instances(state.vertex_instances, state.vertex_instance_count);
    if (state.vertex_constants) {
        state.vertex_constants->Release();
    }
    if (state.pixel_shader) {
        state.pixel_shader->Release();
    }
    release_instances(state.pixel_instances, state.pixel_instance_count);
    if (state.pixel_resource) {
        state.pixel_resource->Release();
    }
    if (state.pixel_sampler) {
        state.pixel_sampler->Release();
    }
    if (state.geometry_shader) {
        state.geometry_shader->Release();
    }
    release_instances(state.geometry_instances, state.geometry_instance_count);
    if (state.hull_shader) {
        state.hull_shader->Release();
    }
    release_instances(state.hull_instances, state.hull_instance_count);
    if (state.domain_shader) {
        state.domain_shader->Release();
    }
    release_instances(state.domain_instances, state.domain_instance_count);
    if (state.rasterizer) {
        state.rasterizer->Release();
    }
    if (state.blend) {
        state.blend->Release();
    }
    if (state.depth_stencil) {
        state.depth_stencil->Release();
    }
}

bool is_srgb_format(DXGI_FORMAT format)
{
    return format == DXGI_FORMAT_R8G8B8A8_UNORM_SRGB || format == DXGI_FORMAT_B8G8R8A8_UNORM_SRGB ||
           format == DXGI_FORMAT_B8G8R8X8_UNORM_SRGB;
}

// Loaded by name from the system directory only. This runs inside a game process, and a plain
// LoadLibrary would let anything named d3dcompiler_47.dll next to the executable answer instead.
compile_fn load_compiler()
{
    const HMODULE module = LoadLibraryExW(L"d3dcompiler_47.dll", nullptr,
                                          LOAD_LIBRARY_SEARCH_SYSTEM32);
    if (!module) {
        return nullptr;
    }
    return reinterpret_cast<compile_fn>(
        reinterpret_cast<void*>(GetProcAddress(module, "D3DCompile")));
}

} // namespace

struct rsf_overlay_renderer {
    ID3D11Device* device = nullptr;
    ID3D11VertexShader* vertex_shader = nullptr;
    ID3D11PixelShader* pixel_shader = nullptr;
    ID3D11InputLayout* input_layout = nullptr;
    ID3D11Buffer* constants = nullptr;
    ID3D11Buffer* vertices = nullptr;
    ID3D11Buffer* indices = nullptr;
    uint32_t vertex_capacity = 0;
    uint32_t index_capacity = 0;
    ID3D11BlendState* blend = nullptr;
    ID3D11DepthStencilState* depth_stencil = nullptr;
    ID3D11RasterizerState* rasterizer = nullptr;
    ID3D11SamplerState* sampler = nullptr;
    std::vector<Texture> textures;
    // Diagnostics that would otherwise repeat every frame for as long as the game runs.
    bool reported_missing_texture = false;
    bool reported_srgb_target = false;
    bool reported_no_target = false;
    rsf_overlay_renderer_log_fn log = nullptr;
    void* log_user = nullptr;
};

namespace {

void say(const rsf_overlay_renderer* renderer, const char* format, ...)
{
    if (!renderer || !renderer->log) {
        return;
    }
    char message[512];
    va_list arguments;
    va_start(arguments, format);
    std::vsnprintf(message, sizeof(message), format, arguments);
    va_end(arguments);
    renderer->log(renderer->log_user, message);
}

// The two ways this can draw nothing visible without anything failing. Neither is corrected here:
// an absent render target is the caller's to bind, and an sRGB view needs a colour conversion that
// nobody has yet been able to look at on a screen. Reported once each, because a message every
// frame for the length of a session is not a diagnostic.
void report_target_once(rsf_overlay_renderer* renderer, ID3D11DeviceContext* context)
{
    if (renderer->reported_no_target && renderer->reported_srgb_target) {
        return;
    }
    ID3D11RenderTargetView* target = nullptr;
    context->OMGetRenderTargets(1, &target, nullptr);
    if (!target) {
        if (!renderer->reported_no_target) {
            renderer->reported_no_target = true;
            say(renderer, "no render target was bound when the overlay drew, so it went nowhere");
        }
        return;
    }
    if (!renderer->reported_srgb_target) {
        D3D11_RENDER_TARGET_VIEW_DESC description{};
        target->GetDesc(&description);
        if (is_srgb_format(description.Format)) {
            renderer->reported_srgb_target = true;
            say(renderer,
                "the bound render target is an sRGB view, so the overlay's colours are encoded "
                "twice and will look washed out");
        }
    }
    target->Release();
}

// Linear, because the overlay holds a font atlas and a handful of images at most. The returned
// pointer is into the vector and only survives until the next insertion.
Texture* find_texture(rsf_overlay_renderer* renderer, uint64_t id)
{
    for (Texture& entry : renderer->textures) {
        if (entry.id == id) {
            return &entry;
        }
    }
    return nullptr;
}

bool grow_buffer(rsf_overlay_renderer* renderer, ID3D11Buffer** buffer, uint32_t* capacity,
                 uint32_t needed, uint32_t stride, UINT bind_flag)
{
    if (*buffer && *capacity >= needed) {
        return true;
    }
    const uint32_t blocks = (needed + growth_block - 1) / growth_block;
    const uint32_t wanted = blocks * growth_block;

    D3D11_BUFFER_DESC description{};
    description.ByteWidth = wanted * stride;
    description.Usage = D3D11_USAGE_DYNAMIC;
    description.BindFlags = bind_flag;
    description.CPUAccessFlags = D3D11_CPU_ACCESS_WRITE;

    ID3D11Buffer* replacement = nullptr;
    if (FAILED(renderer->device->CreateBuffer(&description, nullptr, &replacement)) ||
        !replacement) {
        return false;
    }
    if (*buffer) {
        (*buffer)->Release();
    }
    *buffer = replacement;
    *capacity = wanted;
    return true;
}

} // namespace

extern "C" rsf_overlay_renderer_result rsf_overlay_renderer_create(
    void* d3d11_device, const rsf_overlay_renderer_setup* setup, rsf_overlay_renderer** out)
{
    if (!d3d11_device || !setup || !out ||
        setup->struct_size < sizeof(rsf_overlay_renderer_setup)) {
        return RSF_OVERLAY_RENDERER_ERROR_INVALID_ARGUMENT;
    }
    if (setup->abi_version != RSF_OVERLAY_RENDERER_ABI_VERSION) {
        return RSF_OVERLAY_RENDERER_ERROR_ABI_MISMATCH;
    }
    *out = nullptr;

    auto* device = static_cast<ID3D11Device*>(d3d11_device);
    auto* renderer = new (std::nothrow) rsf_overlay_renderer();
    if (!renderer) {
        return RSF_OVERLAY_RENDERER_ERROR_RESOURCE_FAILED;
    }
    renderer->device = device;
    renderer->device->AddRef();
    renderer->log = setup->log;
    renderer->log_user = setup->log_user;

    const compile_fn compile = load_compiler();
    if (!compile) {
        say(renderer, "d3dcompiler_47.dll could not be loaded from the system directory");
        rsf_overlay_renderer_destroy(renderer);
        return RSF_OVERLAY_RENDERER_ERROR_SHADER_FAILED;
    }

    const SIZE_T source_length = std::strlen(overlay_shader);
    ID3DBlob* vertex_bytecode = nullptr;
    ID3DBlob* errors = nullptr;
    HRESULT compiled = compile(overlay_shader, source_length, "overlay_renderer", nullptr, nullptr,
                               "vertex_main", "vs_4_0", 0, 0, &vertex_bytecode, &errors);
    if (FAILED(compiled) || !vertex_bytecode) {
        say(renderer, "overlay vertex shader did not compile: %s",
            errors ? static_cast<const char*>(errors->GetBufferPointer()) : "no message");
        if (errors) {
            errors->Release();
        }
        if (vertex_bytecode) {
            vertex_bytecode->Release();
        }
        rsf_overlay_renderer_destroy(renderer);
        return RSF_OVERLAY_RENDERER_ERROR_SHADER_FAILED;
    }
    if (errors) {
        errors->Release();
        errors = nullptr;
    }

    ID3DBlob* pixel_bytecode = nullptr;
    compiled = compile(overlay_shader, source_length, "overlay_renderer", nullptr, nullptr,
                       "pixel_main", "ps_4_0", 0, 0, &pixel_bytecode, &errors);
    if (FAILED(compiled) || !pixel_bytecode) {
        say(renderer, "overlay pixel shader did not compile: %s",
            errors ? static_cast<const char*>(errors->GetBufferPointer()) : "no message");
        if (errors) {
            errors->Release();
        }
        if (pixel_bytecode) {
            pixel_bytecode->Release();
        }
        vertex_bytecode->Release();
        rsf_overlay_renderer_destroy(renderer);
        return RSF_OVERLAY_RENDERER_ERROR_SHADER_FAILED;
    }
    if (errors) {
        errors->Release();
    }

    const HRESULT made_vertex_shader =
        device->CreateVertexShader(vertex_bytecode->GetBufferPointer(),
                                   vertex_bytecode->GetBufferSize(), nullptr,
                                   &renderer->vertex_shader);
    const HRESULT made_pixel_shader =
        device->CreatePixelShader(pixel_bytecode->GetBufferPointer(),
                                  pixel_bytecode->GetBufferSize(), nullptr,
                                  &renderer->pixel_shader);
    pixel_bytecode->Release();
    if (FAILED(made_vertex_shader) || FAILED(made_pixel_shader) || !renderer->vertex_shader ||
        !renderer->pixel_shader) {
        say(renderer, "overlay shaders could not be created");
        vertex_bytecode->Release();
        rsf_overlay_renderer_destroy(renderer);
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
        elements, 3, vertex_bytecode->GetBufferPointer(), vertex_bytecode->GetBufferSize(),
        &renderer->input_layout);
    vertex_bytecode->Release();
    if (FAILED(made_layout) || !renderer->input_layout) {
        say(renderer, "overlay input layout could not be created");
        rsf_overlay_renderer_destroy(renderer);
        return RSF_OVERLAY_RENDERER_ERROR_RESOURCE_FAILED;
    }

    D3D11_BUFFER_DESC constants{};
    constants.ByteWidth = sizeof(Constants);
    constants.Usage = D3D11_USAGE_DYNAMIC;
    constants.BindFlags = D3D11_BIND_CONSTANT_BUFFER;
    constants.CPUAccessFlags = D3D11_CPU_ACCESS_WRITE;
    if (FAILED(device->CreateBuffer(&constants, nullptr, &renderer->constants)) ||
        !renderer->constants) {
        say(renderer, "overlay constant buffer could not be created");
        rsf_overlay_renderer_destroy(renderer);
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
        say(renderer, "overlay blend state could not be created");
        rsf_overlay_renderer_destroy(renderer);
        return RSF_OVERLAY_RENDERER_ERROR_RESOURCE_FAILED;
    }

    // The overlay sits on top of a finished image. It reads no depth and writes none, so whatever
    // depth buffer the game has is neither consulted nor damaged.
    D3D11_DEPTH_STENCIL_DESC depth_stencil{};
    depth_stencil.DepthEnable = FALSE;
    depth_stencil.DepthWriteMask = D3D11_DEPTH_WRITE_MASK_ZERO;
    depth_stencil.DepthFunc = D3D11_COMPARISON_ALWAYS;
    depth_stencil.StencilEnable = FALSE;
    if (FAILED(device->CreateDepthStencilState(&depth_stencil, &renderer->depth_stencil)) ||
        !renderer->depth_stencil) {
        say(renderer, "overlay depth stencil state could not be created");
        rsf_overlay_renderer_destroy(renderer);
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
        say(renderer, "overlay rasterizer state could not be created");
        rsf_overlay_renderer_destroy(renderer);
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
        say(renderer, "overlay sampler could not be created");
        rsf_overlay_renderer_destroy(renderer);
        return RSF_OVERLAY_RENDERER_ERROR_RESOURCE_FAILED;
    }

    say(renderer, "overlay renderer ready");
    *out = renderer;
    return RSF_OVERLAY_RENDERER_OK;
}

extern "C" rsf_overlay_renderer_result rsf_overlay_renderer_upload_texture(
    rsf_overlay_renderer* renderer, void* context_pointer,
    const rsf_overlay_texture_update* update)
{
    if (!renderer || !context_pointer || !update || !update->pixels || update->width == 0 ||
        update->height == 0) {
        return RSF_OVERLAY_RENDERER_ERROR_INVALID_ARGUMENT;
    }
    // D3D11's own maximum, so nothing larger could be created anyway. Checking it here also keeps
    // the row pitch below from overflowing on a size that arrived wrong.
    if (update->width > D3D11_REQ_TEXTURE2D_U_OR_V_DIMENSION ||
        update->height > D3D11_REQ_TEXTURE2D_U_OR_V_DIMENSION) {
        say(renderer, "overlay texture update of %ux%u is larger than D3D11 allows", update->width,
            update->height);
        return RSF_OVERLAY_RENDERER_ERROR_INVALID_ARGUMENT;
    }

    auto* context = static_cast<ID3D11DeviceContext*>(context_pointer);
    const UINT row_pitch = update->width * 4u;

    if (update->is_whole_texture) {
        D3D11_TEXTURE2D_DESC description{};
        description.Width = update->width;
        description.Height = update->height;
        description.MipLevels = 1;
        description.ArraySize = 1;
        description.Format = DXGI_FORMAT_R8G8B8A8_UNORM;
        description.SampleDesc.Count = 1;
        description.Usage = D3D11_USAGE_DEFAULT;
        description.BindFlags = D3D11_BIND_SHADER_RESOURCE;

        D3D11_SUBRESOURCE_DATA initial{};
        initial.pSysMem = update->pixels;
        initial.SysMemPitch = row_pitch;

        ID3D11Texture2D* texture = nullptr;
        if (FAILED(renderer->device->CreateTexture2D(&description, &initial, &texture)) ||
            !texture) {
            say(renderer, "overlay texture %llu could not be created at %ux%u",
                static_cast<unsigned long long>(update->id), update->width, update->height);
            return RSF_OVERLAY_RENDERER_ERROR_RESOURCE_FAILED;
        }
        ID3D11ShaderResourceView* view = nullptr;
        if (FAILED(renderer->device->CreateShaderResourceView(texture, nullptr, &view)) || !view) {
            texture->Release();
            say(renderer, "overlay texture %llu has no shader resource view",
                static_cast<unsigned long long>(update->id));
            return RSF_OVERLAY_RENDERER_ERROR_RESOURCE_FAILED;
        }

        // Built before the old one is dropped, so a failure above leaves what the overlay is
        // already drawing with intact.
        Texture* existing = find_texture(renderer, update->id);
        if (existing) {
            if (existing->view) {
                existing->view->Release();
            }
            if (existing->texture) {
                existing->texture->Release();
            }
            existing->texture = texture;
            existing->view = view;
            existing->width = update->width;
            existing->height = update->height;
            return RSF_OVERLAY_RENDERER_OK;
        }

        const Texture entry{update->id, texture, view, update->width, update->height};
        try {
            renderer->textures.push_back(entry);
        } catch (...) {
            view->Release();
            texture->Release();
            return RSF_OVERLAY_RENDERER_ERROR_RESOURCE_FAILED;
        }
        return RSF_OVERLAY_RENDERER_OK;
    }

    Texture* target = find_texture(renderer, update->id);
    if (!target) {
        say(renderer, "overlay patch for texture %llu, which was never uploaded whole",
            static_cast<unsigned long long>(update->id));
        return RSF_OVERLAY_RENDERER_ERROR_INVALID_ARGUMENT;
    }
    // In 64 bit, because a sum that wrapped would pass the bounds check below and then write
    // outside the texture.
    const uint64_t right = static_cast<uint64_t>(update->x) + update->width;
    const uint64_t bottom = static_cast<uint64_t>(update->y) + update->height;
    if (right > target->width || bottom > target->height) {
        say(renderer, "overlay patch %ux%u at %u,%u does not fit texture %llu of %ux%u",
            update->width, update->height, update->x, update->y,
            static_cast<unsigned long long>(update->id), target->width, target->height);
        return RSF_OVERLAY_RENDERER_ERROR_INVALID_ARGUMENT;
    }

    D3D11_BOX box{};
    box.left = update->x;
    box.top = update->y;
    box.front = 0;
    box.right = static_cast<UINT>(right);
    box.bottom = static_cast<UINT>(bottom);
    box.back = 1;
    context->UpdateSubresource(target->texture, 0, &box, update->pixels, row_pitch,
                               row_pitch * update->height);
    return RSF_OVERLAY_RENDERER_OK;
}

extern "C" void rsf_overlay_renderer_free_texture(rsf_overlay_renderer* renderer, uint64_t id)
{
    if (!renderer) {
        return;
    }
    for (size_t index = 0; index < renderer->textures.size(); ++index) {
        if (renderer->textures[index].id != id) {
            continue;
        }
        if (renderer->textures[index].view) {
            renderer->textures[index].view->Release();
        }
        if (renderer->textures[index].texture) {
            renderer->textures[index].texture->Release();
        }
        renderer->textures[index] = renderer->textures.back();
        renderer->textures.pop_back();
        return;
    }
}

extern "C" rsf_overlay_renderer_result rsf_overlay_renderer_draw(rsf_overlay_renderer* renderer,
                                                                 void* context_pointer,
                                                                 const rsf_overlay_draw_data* data,
                                                                 uint32_t target_width,
                                                                 uint32_t target_height)
{
    if (!renderer || !context_pointer || !data ||
        data->struct_size < sizeof(rsf_overlay_draw_data) || target_width == 0 ||
        target_height == 0) {
        return RSF_OVERLAY_RENDERER_ERROR_INVALID_ARGUMENT;
    }
    if (data->call_count == 0 || data->vertex_count == 0 || data->index_count == 0) {
        // Nothing to draw, which is every frame the overlay is hidden. No context state has been
        // touched yet, so there is nothing to put back either.
        return RSF_OVERLAY_RENDERER_OK;
    }
    if (!data->vertices || !data->indices || !data->calls) {
        return RSF_OVERLAY_RENDERER_ERROR_INVALID_ARGUMENT;
    }
    if (data->vertex_count > max_vertices || data->index_count > max_indices) {
        say(renderer, "overlay frame of %u vertices and %u indices is past any plausible ceiling",
            data->vertex_count, data->index_count);
        return RSF_OVERLAY_RENDERER_ERROR_INVALID_ARGUMENT;
    }

    auto* context = static_cast<ID3D11DeviceContext*>(context_pointer);

    if (!grow_buffer(renderer, &renderer->vertices, &renderer->vertex_capacity, data->vertex_count,
                     sizeof(rsf_overlay_vertex), D3D11_BIND_VERTEX_BUFFER) ||
        !grow_buffer(renderer, &renderer->indices, &renderer->index_capacity, data->index_count,
                     sizeof(uint32_t), D3D11_BIND_INDEX_BUFFER)) {
        say(renderer, "overlay geometry buffers could not be grown to %u vertices and %u indices",
            data->vertex_count, data->index_count);
        return RSF_OVERLAY_RENDERER_ERROR_RESOURCE_FAILED;
    }

    // One discard map each, for the whole frame's geometry, rather than one per draw call. The
    // draw calls index into what is written here.
    D3D11_MAPPED_SUBRESOURCE mapped{};
    if (FAILED(context->Map(renderer->vertices, 0, D3D11_MAP_WRITE_DISCARD, 0, &mapped))) {
        return RSF_OVERLAY_RENDERER_ERROR_RESOURCE_FAILED;
    }
    std::memcpy(mapped.pData, data->vertices,
                static_cast<size_t>(data->vertex_count) * sizeof(rsf_overlay_vertex));
    context->Unmap(renderer->vertices, 0);

    if (FAILED(context->Map(renderer->indices, 0, D3D11_MAP_WRITE_DISCARD, 0, &mapped))) {
        return RSF_OVERLAY_RENDERER_ERROR_RESOURCE_FAILED;
    }
    std::memcpy(mapped.pData, data->indices,
                static_cast<size_t>(data->index_count) * sizeof(uint32_t));
    context->Unmap(renderer->indices, 0);

    if (FAILED(context->Map(renderer->constants, 0, D3D11_MAP_WRITE_DISCARD, 0, &mapped))) {
        return RSF_OVERLAY_RENDERER_ERROR_RESOURCE_FAILED;
    }
    Constants values{};
    values.inverse_target_size[0] = 2.0f / static_cast<float>(target_width);
    values.inverse_target_size[1] = 2.0f / static_cast<float>(target_height);
    std::memcpy(mapped.pData, &values, sizeof(values));
    context->Unmap(renderer->constants, 0);

    report_target_once(renderer, context);

    SavedState state{};
    save_state(context, state);

    const UINT stride = sizeof(rsf_overlay_vertex);
    const UINT offset = 0;
    context->IASetInputLayout(renderer->input_layout);
    context->IASetPrimitiveTopology(D3D11_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
    context->IASetVertexBuffers(0, 1, &renderer->vertices, &stride, &offset);
    context->IASetIndexBuffer(renderer->indices, DXGI_FORMAT_R32_UINT, 0);
    context->VSSetShader(renderer->vertex_shader, nullptr, 0);
    context->VSSetConstantBuffers(0, 1, &renderer->constants);
    context->PSSetShader(renderer->pixel_shader, nullptr, 0);
    context->PSSetSamplers(0, 1, &renderer->sampler);
    context->GSSetShader(nullptr, nullptr, 0);
    context->HSSetShader(nullptr, nullptr, 0);
    context->DSSetShader(nullptr, nullptr, 0);

    const FLOAT blend_factor[4] = {0.0f, 0.0f, 0.0f, 0.0f};
    context->OMSetBlendState(renderer->blend, blend_factor, 0xffffffffu);
    context->OMSetDepthStencilState(renderer->depth_stencil, 0);
    context->RSSetState(renderer->rasterizer);

    // No OMSetRenderTargets. Whatever the caller bound is what this draws into, and the reason for
    // leaving it alone is written next to SavedState. The game's depth stencil view stays bound and
    // is neither read nor written, because the depth stencil state above disables both.
    D3D11_VIEWPORT viewport{};
    viewport.TopLeftX = 0.0f;
    viewport.TopLeftY = 0.0f;
    viewport.Width = static_cast<float>(target_width);
    viewport.Height = static_cast<float>(target_height);
    viewport.MinDepth = 0.0f;
    viewport.MaxDepth = 1.0f;
    context->RSSetViewports(1, &viewport);

    for (uint32_t index = 0; index < data->call_count; ++index) {
        const rsf_overlay_draw_call& call = data->calls[index];
        if (call.index_count == 0) {
            continue;
        }
        // The draw data is somebody else's memory and a run past the end of it is a GPU fault
        // rather than an exception, so the ranges are checked rather than trusted.
        const uint64_t index_end = static_cast<uint64_t>(call.index_offset) + call.index_count;
        if (index_end > data->index_count || call.vertex_offset >= data->vertex_count) {
            say(renderer, "overlay draw call %u indexes outside the frame's own geometry", index);
            continue;
        }

        const uint64_t left = call.clip_x;
        const uint64_t top = call.clip_y;
        uint64_t right = left + call.clip_width;
        uint64_t bottom = top + call.clip_height;
        if (right > target_width) {
            right = target_width;
        }
        if (bottom > target_height) {
            bottom = target_height;
        }
        // Entirely off the target. D3D11 rejects an inverted rectangle, and egui does produce empty
        // clips for widgets scrolled out of view.
        if (left >= right || top >= bottom) {
            continue;
        }

        const Texture* texture = find_texture(renderer, call.texture_id);
        if (!texture) {
            if (!renderer->reported_missing_texture) {
                renderer->reported_missing_texture = true;
                say(renderer, "overlay draw call names texture %llu, which was never uploaded",
                    static_cast<unsigned long long>(call.texture_id));
            }
            continue;
        }

        D3D11_RECT scissor{};
        scissor.left = static_cast<LONG>(left);
        scissor.top = static_cast<LONG>(top);
        scissor.right = static_cast<LONG>(right);
        scissor.bottom = static_cast<LONG>(bottom);
        context->RSSetScissorRects(1, &scissor);
        context->PSSetShaderResources(0, 1, &texture->view);
        context->DrawIndexed(call.index_count, call.index_offset,
                             static_cast<INT>(call.vertex_offset));
    }

    // Our own atlas out of the slot before the game's bindings go back, so a texture the game is
    // about to use as a render target is not left bound here as a shader resource.
    ID3D11ShaderResourceView* no_resource = nullptr;
    context->PSSetShaderResources(0, 1, &no_resource);

    restore_state(context, state);
    return RSF_OVERLAY_RENDERER_OK;
}

extern "C" void rsf_overlay_renderer_destroy(rsf_overlay_renderer* renderer)
{
    if (!renderer) {
        return;
    }
    for (Texture& entry : renderer->textures) {
        if (entry.view) {
            entry.view->Release();
        }
        if (entry.texture) {
            entry.texture->Release();
        }
    }
    if (renderer->sampler) {
        renderer->sampler->Release();
    }
    if (renderer->rasterizer) {
        renderer->rasterizer->Release();
    }
    if (renderer->depth_stencil) {
        renderer->depth_stencil->Release();
    }
    if (renderer->blend) {
        renderer->blend->Release();
    }
    if (renderer->indices) {
        renderer->indices->Release();
    }
    if (renderer->vertices) {
        renderer->vertices->Release();
    }
    if (renderer->constants) {
        renderer->constants->Release();
    }
    if (renderer->input_layout) {
        renderer->input_layout->Release();
    }
    if (renderer->pixel_shader) {
        renderer->pixel_shader->Release();
    }
    if (renderer->vertex_shader) {
        renderer->vertex_shader->Release();
    }
    if (renderer->device) {
        renderer->device->Release();
    }
    delete renderer;
}
