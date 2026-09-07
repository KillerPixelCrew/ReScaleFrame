// Drive the frame tap's render target watch the way a game's frame tail drives it: bind a target,
// bind inputs, draw, and check that what comes back describes the draw that was made.
//
// The reconstruction input set is not exercised here. It is recognised from a combination of
// textures that only a real engine frame produces, and a synthetic imitation of it would test the
// imitation. The watch is different: it reports the game's own draw with no rule about what
// qualifies, so a synthetic draw is the same draw a game makes.
//
// The draws are real ones, with shaders and an index buffer, even though nothing here looks at a
// rendered pixel. Leaving the pipeline empty was the first attempt, on the reasoning that the hook
// is on the vtable entry and runs on the call rather than on the rendering. It is, and it does, and
// DXVK still faults on its own worker thread a moment later: it does not validate that a vertex
// shader is bound the way the Windows runtime does. So the draws are made valid, which costs a
// trivial shader and removes a crash that has nothing to do with what is being tested.

#include <rescaleframe/frame_tap.h>
#include <rescaleframe/depth_replay.h>
#include <rescaleframe/ac7_scene_color.h>

#include <windows.h>

#include <d3d11.h>

#include <d3dcommon.h>

#include <cstdio>
#include <cstring>
#include <string>
#include <vector>

namespace {

bool passed = true;

void check(bool condition, const char* message)
{
    if (!condition) {
        std::fprintf(stderr, "%s\n", message);
        passed = false;
    }
}

// This patches vtables and drives a graphics runtime, so a hang is a realistic failure. Announcing
// each stage means a stall says where it stalled rather than nothing at all.
void stage(const char* what)
{
    std::fprintf(stderr, "[stage] %s\n", what);
    std::fflush(stderr);
}

struct Report {
    uint32_t watch_index = 0;
    void* render_target = nullptr;
    uint32_t target_width = 0;
    uint32_t target_height = 0;
    uint32_t viewport_width = 0;
    uint32_t viewport_height = 0;
    uint32_t draw_index = 0;
    uint32_t indexed = 0;
    uint32_t element_count = 0;
    std::vector<rsf_frame_tap_input> inputs;
    void* pixel_shader = nullptr;
    void* vertex_shader = nullptr;
    void* input_layout = nullptr;
    void* blend_state = nullptr;
    void* depth_stencil_state = nullptr;
    uint32_t vertex_stride = 0;
    uint32_t topology = 0;
};

std::vector<Report> reports;

// Set for one stage only, so the rest of the test keeps its plain behaviour. Null means the
// callback does nothing beyond recording, which is what it did before.
struct {
    ID3D11DeviceContext* context = nullptr;
    ID3D11RenderTargetView* other_target = nullptr;
    ID3D11RenderTargetView* original_target = nullptr;
} divert_rehearsal;

void collect_target_draw(void* user, const rsf_frame_tap_target_draw* draw)
{
    (void)user;
    Report report;
    report.watch_index = draw->watch_index;
    report.render_target = draw->render_target;
    report.target_width = draw->target_width;
    report.target_height = draw->target_height;
    report.viewport_width = draw->viewport_width;
    report.viewport_height = draw->viewport_height;
    report.draw_index = draw->draw_index;
    report.indexed = draw->indexed;
    report.element_count = draw->element_count;
    // Copied rather than kept. The header says every pointer in the report is borrowed for the
    // duration of the call, and `inputs` is a stack array inside the hook.
    for (uint32_t index = 0; index < draw->input_count; ++index) {
        report.inputs.push_back(draw->inputs[index]);
    }
    report.pixel_shader = draw->pixel_shader;
    report.vertex_shader = draw->vertex_shader;
    report.input_layout = draw->input_layout;
    report.blend_state = draw->blend_state;
    report.depth_stencil_state = draw->depth_stencil_state;
    report.vertex_stride = draw->vertex_stride;
    report.topology = draw->topology;
    reports.push_back(report);

    // Act like a divert: bind somewhere else and put it back, from inside the hook. This is the
    // shape of what M2 does for real, and what it must not disturb is the shadow of the game's own
    // bindings.
    //
    // Honest about its reach: this passes with re-entry suppressed by a flag as well as by a
    // depth, because the hooks it goes through check and return before constructing a guard, so a
    // nested call leaves a flag alone. The case that separates the two needs a plan active, where
    // the substitution guard is constructed ahead of that check; it becomes reachable when the
    // divert lands and is worth a case of its own then. What this covers today is that a callback
    // may bind from inside a hook at all without the next draw being misattributed.
    if (divert_rehearsal.context) {
        ID3D11RenderTargetView* elsewhere = divert_rehearsal.other_target;
        divert_rehearsal.context->OMSetRenderTargets(1, &elsewhere, nullptr);
        // Slot two specifically, the one the game bound and the assertions below read. Clearing an
        // unread slot would let this pass without meaning anything, which an earlier version of
        // this test did.
        ID3D11ShaderResourceView* nothing = nullptr;
        divert_rehearsal.context->PSSetShaderResources(2, 1, &nothing);
        divert_rehearsal.context->OMSetRenderTargets(1, &divert_rehearsal.original_target, nullptr);
    }
}

// Candidate reports, which are about the frame rather than about a watched target.
std::vector<Report> candidates;

void collect_candidate_draw(void* user, const rsf_frame_tap_target_draw* draw)
{
    (void)user;
    Report report;
    report.watch_index = draw->watch_index;
    report.render_target = draw->render_target;
    report.indexed = draw->indexed;
    report.element_count = draw->element_count;
    report.input_layout = draw->input_layout;
    report.pixel_shader = draw->pixel_shader;
    report.vertex_shader = draw->vertex_shader;
    candidates.push_back(report);
}

uint32_t gates_seen = 0;

void note_gate(void* user, void* context, void* texture)
{
    (void)user;
    (void)context;
    (void)texture;
    ++gates_seen;
}

// What the runtime actually has bound, as against what the test asked for. The two differing is
// the entire claim a substitution makes, so every check of one is a call to the other.
//
// Both of these hand back a reference and both drop it before returning. Comparing addresses is
// all the caller does, and holding the reference would keep a view alive past the point the test
// releases it, which is where a leak turns into a crash on shutdown instead.
ID3D11RenderTargetView* bound_target(ID3D11DeviceContext* context)
{
    ID3D11RenderTargetView* view = nullptr;
    context->OMGetRenderTargets(1, &view, nullptr);
    if (view) {
        view->Release();
    }
    return view;
}

ID3D11ShaderResourceView* bound_resource(ID3D11DeviceContext* context, UINT slot)
{
    ID3D11ShaderResourceView* view = nullptr;
    context->PSGetShaderResources(slot, 1, &view);
    if (view) {
        view->Release();
    }
    return view;
}

ID3D11Texture2D* make_target(ID3D11Device* device, UINT width, UINT height, DXGI_FORMAT format)
{
    D3D11_TEXTURE2D_DESC description{};
    description.Width = width;
    description.Height = height;
    description.MipLevels = 1;
    description.ArraySize = 1;
    description.Format = format;
    description.SampleDesc.Count = 1;
    description.Usage = D3D11_USAGE_DEFAULT;
    description.BindFlags = D3D11_BIND_RENDER_TARGET | D3D11_BIND_SHADER_RESOURCE;
    ID3D11Texture2D* texture = nullptr;
    if (FAILED(device->CreateTexture2D(&description, nullptr, &texture))) {
        return nullptr;
    }
    return texture;
}

// A triangle covering the target, addressed by vertex id, and a constant colour. Neither reads the
// bound shader resource: the tap shadows the binding call, not the shader, so what the pixels do
// with it is beside the point.
const char* const shader_source = R"(
float4 vertex_main(uint id : SV_VertexID) : SV_Position
{
    return float4(id == 1 ? 3.0 : -1.0, id == 2 ? -3.0 : 1.0, 0.0, 1.0);
}

float4 pixel_main() : SV_Target
{
    return float4(1.0, 0.0, 0.0, 1.0);
}
)";

using compile_fn = HRESULT(WINAPI*)(LPCVOID, SIZE_T, LPCSTR, const D3D_SHADER_MACRO*, void*, LPCSTR,
                                    LPCSTR, UINT, UINT, ID3DBlob**, ID3DBlob**);

// Loaded rather than linked, the same way present_blit does it, so nothing here needs the compiler
// import library to build.
compile_fn load_compiler()
{
    const HMODULE module =
        LoadLibraryExW(L"d3dcompiler_47.dll", nullptr, LOAD_LIBRARY_SEARCH_SYSTEM32);
    if (!module) {
        return nullptr;
    }
    return reinterpret_cast<compile_fn>(
        reinterpret_cast<void*>(GetProcAddress(module, "D3DCompile")));
}

ID3DBlob* compile_one(compile_fn compile, const char* entry, const char* target)
{
    ID3DBlob* code = nullptr;
    ID3DBlob* errors = nullptr;
    const HRESULT compiled = compile(shader_source, std::strlen(shader_source), "frame_tap_test",
                                     nullptr, nullptr, entry, target, 0, 0, &code, &errors);
    if (FAILED(compiled) || !code) {
        std::fprintf(stderr, "%s did not compile: %s\n", entry,
                     errors ? static_cast<const char*>(errors->GetBufferPointer()) : "no message");
        code = nullptr;
    }
    if (errors) {
        errors->Release();
    }
    return code;
}

void set_viewport(ID3D11DeviceContext* context, float width, float height)
{
    D3D11_VIEWPORT viewport{};
    viewport.Width = width;
    viewport.Height = height;
    viewport.MaxDepth = 1.0f;
    context->RSSetViewports(1, &viewport);
}

rsf_ac7_scene_color selection{};
uint32_t input_reports = 0;

void collect_input_draw(void*, const rsf_frame_tap_target_draw* draw)
{
    ++input_reports;
    rsf_ac7_scene_color_draw(&selection, draw);
}

void test_composed_color(ID3D11Device* device, ID3D11DeviceContext* context)
{
    stage("selecting composed scene colour from real D3D11 draws");
    ID3D11Texture2D* base = make_target(device, 256, 144, DXGI_FORMAT_R11G11B10_FLOAT);
    ID3D11Texture2D* composed = make_target(device, 256, 144, DXGI_FORMAT_R11G11B10_FLOAT);
    ID3D11Texture2D* later = make_target(device, 256, 144, DXGI_FORMAT_R11G11B10_FLOAT);
    ID3D11Texture2D* layer = make_target(device, 128, 72, DXGI_FORMAT_R16G16B16A16_FLOAT);
    ID3D11Texture2D* tonemap = make_target(device, 256, 144, DXGI_FORMAT_B8G8R8A8_UNORM);
    ID3D11Texture2D* small = make_target(device, 128, 72, DXGI_FORMAT_R11G11B10_FLOAT);
    ID3D11ShaderResourceView* base_srv = nullptr;
    ID3D11ShaderResourceView* layer_srv = nullptr;
    ID3D11ShaderResourceView* composed_srv = nullptr;
    ID3D11RenderTargetView* base_rtv = nullptr;
    ID3D11RenderTargetView* composed_rtv = nullptr;
    ID3D11RenderTargetView* later_rtv = nullptr;
    ID3D11RenderTargetView* tonemap_rtv = nullptr;
    ID3D11RenderTargetView* small_rtv = nullptr;
    check(base && composed && later && layer && tonemap && small,
          "Composition fixture textures must be created.");
    if (!base || !composed || !later || !layer || !tonemap || !small) {
        return;
    }
    check(SUCCEEDED(device->CreateShaderResourceView(base, nullptr, &base_srv)) &&
              SUCCEEDED(device->CreateShaderResourceView(layer, nullptr, &layer_srv)) &&
              SUCCEEDED(device->CreateShaderResourceView(composed, nullptr, &composed_srv)) &&
              SUCCEEDED(device->CreateRenderTargetView(base, nullptr, &base_rtv)) &&
              SUCCEEDED(device->CreateRenderTargetView(composed, nullptr, &composed_rtv)) &&
              SUCCEEDED(device->CreateRenderTargetView(later, nullptr, &later_rtv)) &&
              SUCCEEDED(device->CreateRenderTargetView(tonemap, nullptr, &tonemap_rtv)) &&
              SUCCEEDED(device->CreateRenderTargetView(small, nullptr, &small_rtv)),
          "Composition fixture views must be created.");
    D3D11_TEXTURE2D_DESC depth_desc{};
    depth_desc.Width = 256;
    depth_desc.Height = 144;
    depth_desc.MipLevels = 1;
    depth_desc.ArraySize = 1;
    depth_desc.Format = DXGI_FORMAT_D32_FLOAT;
    depth_desc.SampleDesc.Count = 1;
    depth_desc.BindFlags = D3D11_BIND_DEPTH_STENCIL;
    ID3D11Texture2D* depth = nullptr;
    ID3D11DepthStencilView* dsv = nullptr;
    check(SUCCEEDED(device->CreateTexture2D(&depth_desc, nullptr, &depth)) &&
              SUCCEEDED(device->CreateDepthStencilView(depth, nullptr, &dsv)),
          "Composition depth fixture must be created.");

    rsf_ac7_scene_color_source(&selection, base, context, 256, 144);
    check(rsf_frame_tap_watch_input(base) == RSF_FRAME_TAP_OK, "Input watch must arm.");
    check(rsf_ac7_scene_color_selected(&selection, base) == base,
          "A frame without a recombine must use the original colour.");
    context->OMSetRenderTargets(1, &composed_rtv, nullptr);
    set_viewport(context, 256, 144);
    // Separate calls and high slots exercise inherited bindings rather than an assumed slot zero.
    context->PSSetShaderResources(30, 1, &base_srv);
    context->PSSetShaderResources(31, 1, &layer_srv);
    context->DrawIndexed(12, 0, 0);
    check(rsf_ac7_scene_color_selected(&selection, base) == base,
          "Geometry is not a recombine.");
    set_viewport(context, 128, 72);
    context->DrawIndexed(3, 0, 0);
    check(rsf_ac7_scene_color_selected(&selection, base) == base,
          "A partial viewport is not the full scene.");
    set_viewport(context, 256, 144);
    context->OMSetRenderTargets(1, &small_rtv, nullptr);
    context->DrawIndexed(3, 0, 0);
    check(rsf_ac7_scene_color_selected(&selection, base) == base,
          "A downsample must not replace scene colour.");
    context->OMSetRenderTargets(1, &tonemap_rtv, nullptr);
    context->DrawIndexed(3, 0, 0);
    check(rsf_ac7_scene_color_selected(&selection, base) == base,
          "The tonemap must never be selected.");
    context->OMSetRenderTargets(1, &composed_rtv, nullptr);
    context->DrawIndexed(3, 0, 0);
    check(rsf_ac7_scene_color_selected(&selection, base) == base,
          "A float output after the tonemap boundary must not reopen discovery.");
    rsf_ac7_scene_color_end_frame(&selection);
    context->OMSetRenderTargets(1, &composed_rtv, dsv);
    context->DrawIndexed(3, 0, 0);
    check(rsf_ac7_scene_color_selected(&selection, base) == base,
          "A depth-bound draw must not qualify.");
    context->OMSetRenderTargetsAndUnorderedAccessViews(1, &composed_rtv, nullptr, 1, 0,
                                                     nullptr, nullptr);
    ID3D11ShaderResourceView* empty = nullptr;
    context->PSSetShaderResources(31, 1, &empty);
    context->DrawIndexed(3, 0, 0);
    check(rsf_ac7_scene_color_selected(&selection, base) == base,
          "A colour-only copy must not qualify as recombination.");
    context->PSSetShaderResources(31, 1, &layer_srv);
    context->OMSetRenderTargetsAndUnorderedAccessViews(
        D3D11_KEEP_RENDER_TARGETS_AND_DEPTH_STENCIL, nullptr, nullptr, 1, 0, nullptr, nullptr);
    context->DrawIndexed(3, 0, 0);
    check(rsf_ac7_scene_color_selected(&selection, base) == composed,
          "The captured recombine-shaped draw must select the composed target.");
    check(rsf_ac7_scene_color_source(&selection, base, context, 256, 144) == 0 &&
              rsf_ac7_scene_color_selected(&selection, base) == composed,
          "A later qualifying pass carrying the same base must preserve this frame's composition.");
    context->OMSetRenderTargets(1, &later_rtv, nullptr);
    context->Draw(3, 0);
    check(rsf_ac7_scene_color_selected(&selection, base) == composed,
          "Later readers must not walk the choice down the post-process chain.");
    rsf_ac7_scene_color_end_frame(&selection);
    check(rsf_ac7_scene_color_selected(&selection, base) == base,
          "Holding the allocation must not reuse the preceding frame's selection.");
    // A deferred context shares the hook vtable but must not overwrite immediate-context shadows.
    ID3D11DeviceContext* deferred = nullptr;
    check(SUCCEEDED(device->CreateDeferredContext(0, &deferred)), "Deferred context must be created.");
    if (deferred) {
        deferred->OMSetRenderTargets(1, &tonemap_rtv, nullptr);
        deferred->PSSetShaderResources(30, 1, &empty);
        deferred->Release();
    }
    context->Draw(3, 0);
    check(rsf_ac7_scene_color_selected(&selection, base) == later,
          "A fresh draw must refresh the choice without rearming the persistent watch.");
    rsf_ac7_scene_color_end_frame(&selection);
    context->PSSetShaderResources(30, 1, &composed_srv);
    const uint32_t before_unrelated = input_reports;
    context->Draw(3, 0);
    check(input_reports == before_unrelated &&
              rsf_ac7_scene_color_selected(&selection, base) == base,
          "A draw that does not read the original colour must not be followed.");
    context->PSSetShaderResources(30, 1, &base_srv);
    context->OMSetRenderTargets(1, &base_rtv, nullptr); // implicit SRV unbind
    context->OMSetRenderTargets(1, &composed_rtv, nullptr);
    context->Draw(3, 0);
    check(rsf_ac7_scene_color_selected(&selection, base) == base,
          "An implicit SRV unbind must not leave a false read in the shadow.");
    context->PSSetShaderResources(30, 1, &base_srv);
    context->Draw(3, 0);
    check(rsf_ac7_scene_color_selected(&selection, base) == composed,
          "Explicit rebinding after an implicit unbind must restore discovery.");
    rsf_ac7_scene_color_source(&selection, later, context, 256, 144);
    check(rsf_ac7_scene_color_selected(&selection, later) == later && !selection.composed,
          "Replacing the source must discard the previous resource association.");
    rsf_ac7_scene_color_source(&selection, base, context, 128, 72);
    context->Draw(3, 0);
    check(rsf_ac7_scene_color_selected(&selection, base) == base,
          "Changing render extent must reject the old full-size association.");
    rsf_frame_tap_watch_input(nullptr);
    rsf_ac7_scene_color_clear(&selection);
    context->PSSetShaderResources(30, 1, &empty);
    context->PSSetShaderResources(31, 1, &empty);
    context->OMSetRenderTargets(0, nullptr, nullptr);
    dsv->Release();
    depth->Release();
    small_rtv->Release();
    tonemap_rtv->Release();
    later_rtv->Release();
    composed_rtv->Release();
    base_rtv->Release();
    composed_srv->Release();
    layer_srv->Release();
    base_srv->Release();
    small->Release();
    tonemap->Release();
    layer->Release();
    later->Release();
    composed->Release();
    base->Release();
}

rsf_depth_replay* depth_fixture = nullptr;
rsf_frame_tap_geometry last_geometry{};
uint32_t geometry_reports = 0, depth_draws = 0;

void collect_geometry(void*, const rsf_frame_tap_geometry* draw)
{
    if (!depth_fixture || !rsf_ac7_scene_depth_candidate(draw)) {
        return;
    }
    last_geometry = *draw;
    ++geometry_reports;
    depth_draws += rsf_depth_replay_draw(depth_fixture, draw);
}

void test_depth_replay(ID3D11Device* device, ID3D11DeviceContext* context, compile_fn compile)
{
    stage("replaying translucent geometry into seeded depth and reading pixels");
    context->ClearState();
    depth_fixture = rsf_depth_replay_create(device, 64, 64);
    check(depth_fixture != nullptr, "Depth replay targets must prepare before rendering.");
    const char* source = R"(
cbuffer Position : register(b13) { float4 transform; };
float4 vs(float3 position : POSITION) : SV_Position {
    return float4(position.xy * transform.w + transform.xy, transform.z, 1);
}
float4 ps() : SV_Target { return float4(1,0,0,1); }
)";
    auto make_shader = [&](const char* entry, const char* profile) {
        ID3DBlob* code = nullptr;
        ID3DBlob* errors = nullptr;
        const HRESULT result = compile(source, std::strlen(source), "depth_replay_test", nullptr,
                                       nullptr, entry, profile, 0, 0, &code, &errors);
        check(SUCCEEDED(result) && code, "Depth fixture shaders must compile.");
        if (errors) {
            errors->Release();
        }
        return code;
    };
    ID3DBlob* vs_code = make_shader("vs", "vs_5_0");
    ID3DBlob* ps_code = make_shader("ps", "ps_5_0");
    if (!depth_fixture || !vs_code || !ps_code) {
        passed = false;
        return;
    }
    ID3D11VertexShader* vs = nullptr;
    ID3D11PixelShader* ps = nullptr;
    ID3D11InputLayout* layout = nullptr;
    D3D11_INPUT_ELEMENT_DESC element{
        "POSITION", 0, DXGI_FORMAT_R32G32B32_FLOAT, 15, 0, D3D11_INPUT_PER_VERTEX_DATA, 0};
    check(SUCCEEDED(device->CreateVertexShader(vs_code->GetBufferPointer(),
                                               vs_code->GetBufferSize(), nullptr, &vs)) &&
              SUCCEEDED(device->CreatePixelShader(ps_code->GetBufferPointer(),
                                                  ps_code->GetBufferSize(), nullptr, &ps)) &&
              SUCCEEDED(device->CreateInputLayout(&element, 1, vs_code->GetBufferPointer(),
                                                  vs_code->GetBufferSize(), &layout)),
          "Depth fixture shader/layout creation must succeed.");
    vs_code->Release();
    ps_code->Release();
    const float vertices[][4] = {
        {9, 9, 0, 0}, {9, 9, 0, 0}, {-0.8f, -0.8f, 0, 0}, {0, 0.8f, 0, 0}, {0.8f, -0.8f, 0, 0}};
    const uint16_t index_values[] = {99, 99, 0, 1, 2};
    auto buffer = [&](const void* data, UINT bytes, UINT flags) {
        D3D11_BUFFER_DESC desc{};
        desc.ByteWidth = bytes;
        desc.BindFlags = flags;
        desc.Usage = D3D11_USAGE_DEFAULT;
        D3D11_SUBRESOURCE_DATA initial{};
        initial.pSysMem = data;
        ID3D11Buffer* b = nullptr;
        check(SUCCEEDED(device->CreateBuffer(&desc, data ? &initial : nullptr, &b)),
              "Depth fixture buffer creation.");
        return b;
    };
    ID3D11Buffer* vb = buffer(vertices, sizeof(vertices), D3D11_BIND_VERTEX_BUFFER);
    ID3D11Buffer* ib = buffer(index_values, sizeof(index_values), D3D11_BIND_INDEX_BUFFER);
    ID3D11Buffer* cb = buffer(nullptr, 16, D3D11_BIND_CONSTANT_BUFFER);
    ID3D11Texture2D* layer = make_target(device, 64, 64, DXGI_FORMAT_R16G16B16A16_FLOAT);
    ID3D11RenderTargetView* rtv = nullptr;
    check(layer && SUCCEEDED(device->CreateRenderTargetView(layer, nullptr, &rtv)),
          "Depth fixture colour target.");
    D3D11_TEXTURE2D_DESC desc{};
    desc.Width = 64;
    desc.Height = 64;
    desc.MipLevels = 1;
    desc.ArraySize = 1;
    desc.Format = DXGI_FORMAT_R32G8X24_TYPELESS;
    desc.SampleDesc.Count = 1;
    desc.BindFlags = D3D11_BIND_DEPTH_STENCIL | D3D11_BIND_SHADER_RESOURCE;
    ID3D11Texture2D* depth = nullptr;
    ID3D11Texture2D* staging = nullptr;
    check(SUCCEEDED(device->CreateTexture2D(&desc, nullptr, &depth)),
          "Opaque depth fixture creation.");
    desc.BindFlags = 0;
    desc.Usage = D3D11_USAGE_STAGING;
    desc.CPUAccessFlags = D3D11_CPU_ACCESS_READ;
    check(SUCCEEDED(device->CreateTexture2D(&desc, nullptr, &staging)),
          "Depth staging fixture creation.");
    D3D11_DEPTH_STENCIL_VIEW_DESC view{};
    view.Format = DXGI_FORMAT_D32_FLOAT_S8X24_UINT;
    view.ViewDimension = D3D11_DSV_DIMENSION_TEXTURE2D;
    ID3D11DepthStencilView* writable = nullptr;
    ID3D11DepthStencilView* readonly = nullptr;
    check(SUCCEEDED(device->CreateDepthStencilView(depth, &view, &writable)),
          "Writable fixture depth view.");
    view.Flags = D3D11_DSV_READ_ONLY_DEPTH;
    check(SUCCEEDED(device->CreateDepthStencilView(depth, &view, &readonly)),
          "Read-only fixture depth view.");
    D3D11_DEPTH_STENCIL_DESC ds{};
    ds.DepthEnable = TRUE;
    ds.DepthWriteMask = D3D11_DEPTH_WRITE_MASK_ZERO;
    ds.DepthFunc = D3D11_COMPARISON_GREATER_EQUAL;
    ds.StencilEnable = TRUE;
    ds.StencilReadMask = 255;
    ds.StencilWriteMask = 255;
    ds.FrontFace = {D3D11_STENCIL_OP_KEEP, D3D11_STENCIL_OP_KEEP, D3D11_STENCIL_OP_REPLACE,
                    D3D11_COMPARISON_ALWAYS};
    ds.BackFace = ds.FrontFace;
    ID3D11DepthStencilState* depth_state = nullptr;
    check(SUCCEEDED(device->CreateDepthStencilState(&ds, &depth_state)),
          "Depth-read stencil-write state.");
    D3D11_RASTERIZER_DESC raster_desc{};
    raster_desc.FillMode = D3D11_FILL_SOLID;
    raster_desc.CullMode = D3D11_CULL_NONE;
    raster_desc.DepthClipEnable = TRUE;
    raster_desc.ScissorEnable = TRUE;
    ID3D11RasterizerState* raster = nullptr;
    check(SUCCEEDED(device->CreateRasterizerState(&raster_desc, &raster)),
          "Depth fixture raster state.");
    ID3D11ShaderResourceView* high_srv = nullptr;
    ID3D11Texture2D* high_texture = make_target(device, 8, 8, DXGI_FORMAT_R8G8B8A8_UNORM);
    check(SUCCEEDED(device->CreateShaderResourceView(high_texture, nullptr, &high_srv)),
          "High slot fixture resource.");
    auto bind = [&](ID3D11DeviceContext* c) {
        c->VSSetShader(vs, nullptr, 0);
        c->PSSetShader(ps, nullptr, 0);
        c->IASetInputLayout(layout);
        c->IASetPrimitiveTopology(D3D11_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
        UINT stride = 16, offset = 16;
        c->IASetVertexBuffers(15, 1, &vb, &stride, &offset);
        c->IASetIndexBuffer(ib, DXGI_FORMAT_R16_UINT, 2);
        c->VSSetConstantBuffers(13, 1, &cb);
        c->PSSetShaderResources(100, 1, &high_srv);
        c->OMSetRenderTargets(1, &rtv, readonly);
        c->OMSetDepthStencilState(depth_state, 37);
        c->RSSetState(raster);
        set_viewport(c, 64, 64);
        D3D11_RECT scissor{0, 0, 64, 64};
        c->RSSetScissorRects(1, &scissor);
    };
    auto transform = [&](float x, float z) {
        const float values[4] = {x, 0, z, 0.4f};
        context->UpdateSubresource(cb, 0, nullptr, values, 0, 0);
    };
    auto read = [&](void* texture, UINT x, UINT y) {
        context->CopyResource(staging, static_cast<ID3D11Texture2D*>(texture));
        D3D11_MAPPED_SUBRESOURCE mapped{};
        if (FAILED(context->Map(staging, 0, D3D11_MAP_READ, 0, &mapped))) {
            check(false, "Depth readback mapping must succeed.");
            return -1.0f;
        }
        float value = 0;
        std::memcpy(&value, static_cast<const char*>(mapped.pData) + y * mapped.RowPitch + x * 8,
                    4);
        context->Unmap(staging, 0);
        return value;
    };
    auto selected = [&]() {
        return rsf_depth_replay_selected(depth_fixture, context, depth, layer);
    };
    bind(context);
    transform(-0.45f, 0.6f);
    context->ClearDepthStencilView(writable, D3D11_CLEAR_DEPTH | D3D11_CLEAR_STENCIL, 0.25f, 0);
    check(selected() == depth, "No layer draw must keep the original depth identity.");
    context->DrawIndexed(3, 1, 1);
    check(depth_draws == 1,
          "One game indexed draw must cause exactly one replay, without recursion.");
    check(last_geometry.vertex_buffers[15] == vb && last_geometry.strides[15] == 16 &&
              last_geometry.offsets[15] == 16 && last_geometry.index_buffer == ib &&
              last_geometry.index_format == DXGI_FORMAT_R16_UINT &&
              last_geometry.index_offset == 2 && last_geometry.input_layout == layout &&
              last_geometry.vertex_shader == vs && last_geometry.vertex_constants[13] == cb &&
              last_geometry.start == 1 && last_geometry.base_vertex == 1,
          "Shadow must include high IA/VS slots, offsets, layout and indexed draw arguments.");
    void* augmented = selected();
    check(augmented != depth, "A successful matching layer must select the copied depth.");
    check(read(augmented, 18, 32) == 0.6f && read(augmented, 2, 2) == 0.25f,
          "Triangle pixels must gain geometry depth while opaque seed survives outside coverage.");
    check(read(depth, 18, 32) == 0.25f, "Replay must never change the game's opaque depth.");
    transform(0.45f, 0.8f);
    context->DrawIndexedInstanced(3, 1, 1, 1, 0);
    check(depth_draws == 2 && last_geometry.kind == 3, "Indexed instanced draws must replay once.");
    check(
        read(augmented, 18, 32) == 0.6f && read(augmented, 46, 32) == 0.8f,
        "Immediate replay must preserve each draw's constants, and accumulate without reseeding.");
    check(rsf_depth_replay_selected(depth_fixture, context, depth, nullptr) == depth &&
              rsf_depth_replay_selected(depth_fixture, context, depth, high_texture) == depth &&
              rsf_depth_replay_selected(depth_fixture, context, high_texture, layer) ==
                  high_texture,
          "Absent/wrong composition layer or different opaque depth must use the original.");
    ID3D11DepthStencilView* actual_dsv = nullptr;
    ID3D11RenderTargetView* actual_rtv = nullptr;
    context->OMGetRenderTargets(1, &actual_rtv, &actual_dsv);
    check(actual_rtv == rtv && actual_dsv == readonly,
          "Replay must restore colour and read-only depth targets.");
    actual_rtv->Release();
    actual_dsv->Release();
    ID3D11DepthStencilState* actual_ds = nullptr;
    UINT reference = 0;
    context->OMGetDepthStencilState(&actual_ds, &reference);
    check(actual_ds == depth_state && reference == 37,
          "Replay must restore stencil reference and depth state.");
    actual_ds->Release();
    ID3D11PixelShader* actual_ps = nullptr;
    context->PSGetShader(&actual_ps, nullptr, nullptr);
    check(actual_ps == ps, "Replay must restore the pixel shader.");
    actual_ps->Release();
    ID3D11Buffer* actual_vb = nullptr;
    UINT stride = 0, offset = 0;
    context->IAGetVertexBuffers(15, 1, &actual_vb, &stride, &offset);
    check(actual_vb == vb && stride == 16 && offset == 16 &&
              bound_resource(context, 100) == high_srv,
          "Replay must preserve high vertex/resource bindings outside the broad state helper's "
          "range.");
    actual_vb->Release();

    stage("matching augmented depth to the layer consumed by composition");
    ID3D11Texture2D* base_color = make_target(device, 64, 64, DXGI_FORMAT_R11G11B10_FLOAT);
    ID3D11Texture2D* combined = make_target(device, 64, 64, DXGI_FORMAT_R11G11B10_FLOAT);
    ID3D11ShaderResourceView* base_srv = nullptr;
    ID3D11ShaderResourceView* layer_srv = nullptr;
    ID3D11RenderTargetView* combined_rtv = nullptr;
    check(SUCCEEDED(device->CreateShaderResourceView(base_color, nullptr, &base_srv)) &&
              SUCCEEDED(device->CreateShaderResourceView(layer, nullptr, &layer_srv)) &&
              SUCCEEDED(device->CreateRenderTargetView(combined, nullptr, &combined_rtv)),
          "Depth composition fixture views.");
    rsf_ac7_scene_color_source(&selection, base_color, context, 64, 64);
    rsf_frame_tap_watch_input(base_color);
    context->OMSetRenderTargets(1, &combined_rtv, nullptr);
    ID3D11ShaderResourceView* composition_inputs[] = {base_srv, layer_srv};
    context->PSSetShaderResources(0, 2, composition_inputs);
    context->Draw(3, 1);
    /* The layer is now one of a set, because a recombine arrives with several half-float inputs
       bound and picking one of them would be a guess. The claim is that the replayed layer is
       among what this frame reads, which is the question that actually matters. */
    bool layer_offered = false;
    void* depth_from_layers = depth;
    for (uint32_t i = 0; i < selection.composed_layer_count; ++i) {
        layer_offered = layer_offered || selection.composed_layers[i] == layer;
        if (rsf_depth_replay_selected(depth_fixture, context, depth,
                                      selection.composed_layers[i]) == augmented) {
            depth_from_layers = augmented;
        }
    }
    check(rsf_ac7_scene_color_selected(&selection, base_color) == combined && layer_offered &&
              depth_from_layers == augmented,
          "Same-frame composition must offer the exact layer whose depth was replayed.");
    rsf_ac7_scene_color_end_frame(&selection);
    check(selection.composed_layer_count == 0,
          "A following frame without composition must offer no layer, so the original depth "
          "stands even while the copy exists.");
    rsf_ac7_scene_color_clear(&selection);
    rsf_frame_tap_watch_input(nullptr);
    ID3D11ShaderResourceView* no_inputs[2]{};
    context->PSSetShaderResources(0, 2, no_inputs);
    combined_rtv->Release();
    layer_srv->Release();
    base_srv->Release();
    combined->Release();
    base_color->Release();
    bind(context);

    stage("depth replay frame reset, occlusion, scissor and refusal");
    rsf_depth_replay_end_frame(depth_fixture);
    check(selected() == depth, "Frame end must invalidate the previous layer immediately.");
    context->ClearDepthStencilView(writable, D3D11_CLEAR_DEPTH, 0.9f, 0);
    transform(-0.45f, 0.6f);
    context->Draw(3, 1);
    check(read(selected(), 18, 32) == 0.9f,
          "Nearer opaque depth must occlude the replayed triangle.");
    rsf_depth_replay_end_frame(depth_fixture);
    context->ClearDepthStencilView(writable, D3D11_CLEAR_DEPTH, 0.25f, 0);
    D3D11_RECT scissor{0, 0, 10, 64};
    context->RSSetScissorRects(1, &scissor);
    context->DrawInstanced(3, 1, 1, 0);
    check(read(selected(), 18, 32) == 0.25f, "Replay must retain the game's scissor coverage.");
    rsf_depth_replay_end_frame(depth_fixture);
    context->OMSetRenderTargets(1, &rtv, writable);
    context->Draw(3, 1);
    check(selected() == depth, "A writable depth view must refuse this candidate for the frame.");
    context->OMSetRenderTargets(1, &rtv, readonly);
    context->Draw(3, 1);
    check(selected() == depth, "A later supported draw must not hide an incomplete layer.");
    rsf_depth_replay_end_frame(depth_fixture);

    stage("depth replay shadow reset and foreign context isolation");
    context->ClearState();
    bind(context);
    transform(-0.45f, 0.6f);
    ID3D11DeviceContext* deferred = nullptr;
    check(SUCCEEDED(device->CreateDeferredContext(0, &deferred)), "Deferred fixture context.");
    const uint32_t before = geometry_reports;
    bind(deferred);
    deferred->Draw(3, 1);
    check(geometry_reports == before,
          "Foreign-context geometry must not enter the immediate shadow.");
    context->DrawIndexed(3, 1, 1);
    check(selected() != depth && last_geometry.vertex_buffers[15] == vb,
          "ClearState must allow correctly rebound immediate geometry to replay.");
    ID3D11CommandList* list = nullptr;
    check(SUCCEEDED(deferred->FinishCommandList(FALSE, &list)), "Deferred fixture command list.");
    context->ExecuteCommandList(list, FALSE);
    list->Release();
    deferred->Release();
    rsf_depth_replay_end_frame(depth_fixture);
    bind(context);
    context->DrawIndexed(3, 1, 1);
    check(selected() != depth,
          "ExecuteCommandList without restore must invalidate and reseed the shadow.");

    context->ClearState();
    rsf_depth_replay_destroy(depth_fixture);
    depth_fixture = nullptr;
    high_srv->Release();
    high_texture->Release();
    raster->Release();
    depth_state->Release();
    readonly->Release();
    writable->Release();
    staging->Release();
    depth->Release();
    rtv->Release();
    layer->Release();
    cb->Release();
    ib->Release();
    vb->Release();
    layout->Release();
    ps->Release();
    vs->Release();
}

} // namespace

int main()
{
    stage("validating arguments");
    rsf_frame_tap_options options{};
    options.struct_size = sizeof(options);
    options.abi_version = RSF_FRAME_TAP_ABI_VERSION;
    options.on_target_draw = collect_target_draw;
    options.on_input_draw = collect_input_draw;
    options.on_candidate_draw = collect_candidate_draw;
    options.on_geometry = collect_geometry;
    options.output_width = 512;
    options.output_height = 288;

    check(rsf_frame_tap_watch_target(0, nullptr, 0) == RSF_FRAME_TAP_ERROR_NOT_INSTALLED,
          "Watching before installing must be refused.");

    stage("creating device");
    ID3D11Device* device = nullptr;
    ID3D11DeviceContext* context = nullptr;
    D3D_FEATURE_LEVEL level{};
    const HRESULT created =
        D3D11CreateDevice(nullptr, D3D_DRIVER_TYPE_HARDWARE, nullptr, 0, nullptr, 0,
                          D3D11_SDK_VERSION, &device, &level, &context);
    if (FAILED(created) || !device || !context) {
        std::fprintf(stderr, "no D3D11 device available, skipping\n");
        return 0;
    }

    check(rsf_frame_tap_install(context, nullptr) == RSF_FRAME_TAP_ERROR_INVALID_ARGUMENT,
          "A missing options structure must be rejected.");
    options.abi_version = RSF_FRAME_TAP_ABI_VERSION + 1u;
    check(rsf_frame_tap_install(context, &options) == RSF_FRAME_TAP_ERROR_ABI_MISMATCH,
          "An incompatible ABI must be rejected.");
    options.abi_version = RSF_FRAME_TAP_ABI_VERSION;

    stage("installing tap");
    check(rsf_frame_tap_install(context, &options) == RSF_FRAME_TAP_OK,
          "Installing the tap must succeed.");
    check(rsf_frame_tap_install(context, &options) == RSF_FRAME_TAP_ERROR_ALREADY_INSTALLED,
          "Installing twice must be refused.");
    check(rsf_frame_tap_watch_target(RSF_FRAME_TAP_WATCH_SLOTS, nullptr, 0) ==
              RSF_FRAME_TAP_ERROR_INVALID_ARGUMENT,
          "A watch slot past the end must be refused.");

    stage("creating resources");
    // Two targets of different shapes, standing in for the frame's composite and something else,
    // and one texture to bind as an input so a report has something to describe.
    ID3D11Texture2D* composite = make_target(device, 256, 144, DXGI_FORMAT_B8G8R8A8_UNORM);
    ID3D11Texture2D* other = make_target(device, 128, 72, DXGI_FORMAT_B8G8R8A8_UNORM);
    ID3D11Texture2D* source = make_target(device, 64, 36, DXGI_FORMAT_R16G16B16A16_FLOAT);
    check(composite && other && source, "The test textures must be created.");
    if (!composite || !other || !source) {
        return passed ? 0 : 1;
    }

    ID3D11RenderTargetView* composite_view = nullptr;
    ID3D11RenderTargetView* other_view = nullptr;
    ID3D11ShaderResourceView* source_view = nullptr;
    check(SUCCEEDED(device->CreateRenderTargetView(composite, nullptr, &composite_view)) &&
              SUCCEEDED(device->CreateRenderTargetView(other, nullptr, &other_view)) &&
              SUCCEEDED(device->CreateShaderResourceView(source, nullptr, &source_view)),
          "The test views must be created.");

    stage("compiling shaders");
    const compile_fn compile = load_compiler();
    if (!compile) {
        std::fprintf(stderr, "d3dcompiler_47.dll is not available, skipping\n");
        return 0;
    }
    ID3DBlob* vertex_code = compile_one(compile, "vertex_main", "vs_5_0");
    ID3DBlob* pixel_code = compile_one(compile, "pixel_main", "ps_5_0");
    check(vertex_code && pixel_code, "The test shaders must compile.");
    if (!vertex_code || !pixel_code) {
        return 1;
    }
    ID3D11VertexShader* vertex_shader = nullptr;
    ID3D11PixelShader* pixel_shader = nullptr;
    check(SUCCEEDED(device->CreateVertexShader(vertex_code->GetBufferPointer(),
                                               vertex_code->GetBufferSize(), nullptr,
                                               &vertex_shader)) &&
              SUCCEEDED(device->CreatePixelShader(pixel_code->GetBufferPointer(),
                                                  pixel_code->GetBufferSize(), nullptr,
                                                  &pixel_shader)),
          "The test shaders must be created.");
    // An input layout to name as a candidate. The shader reads nothing from the input assembler,
    // and a declaration may supply more than a shader consumes, so one element is enough: what is
    // being tested is that the tap recognises the object, not what it describes.
    const D3D11_INPUT_ELEMENT_DESC layout_elements[] = {
        {"ATTRIBUTE", 0, DXGI_FORMAT_R32G32B32A32_FLOAT, 0, 0, D3D11_INPUT_PER_VERTEX_DATA, 0},
    };
    ID3D11InputLayout* test_layout = nullptr;
    check(SUCCEEDED(device->CreateInputLayout(layout_elements, 1,
                                              vertex_code->GetBufferPointer(),
                                              vertex_code->GetBufferSize(), &test_layout)) &&
              test_layout,
          "The test input layout must be created.");

    vertex_code->Release();
    pixel_code->Release();

    // Indices for the one indexed draw. Their values do not matter; the buffer has to exist,
    // because an indexed draw with nothing bound is the same unvalidated state as a draw with no
    // shader.
    const uint16_t index_values[12] = {0, 1, 2, 0, 1, 2, 0, 1, 2, 0, 1, 2};
    D3D11_BUFFER_DESC index_description{};
    index_description.ByteWidth = sizeof(index_values);
    index_description.Usage = D3D11_USAGE_IMMUTABLE;
    index_description.BindFlags = D3D11_BIND_INDEX_BUFFER;
    D3D11_SUBRESOURCE_DATA index_data{};
    index_data.pSysMem = index_values;
    ID3D11Buffer* indices = nullptr;
    check(SUCCEEDED(device->CreateBuffer(&index_description, &index_data, &indices)),
          "The test index buffer must be created.");

    context->VSSetShader(vertex_shader, nullptr, 0);
    context->PSSetShader(pixel_shader, nullptr, 0);
    context->IASetPrimitiveTopology(D3D11_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
    context->IASetIndexBuffer(indices, DXGI_FORMAT_R16_UINT, 0);

    test_depth_replay(device, context, compile);
    context->VSSetShader(vertex_shader, nullptr, 0);
    context->PSSetShader(pixel_shader, nullptr, 0);
    context->IASetPrimitiveTopology(D3D11_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
    context->IASetIndexBuffer(indices, DXGI_FORMAT_R16_UINT, 0);
    test_composed_color(device, context);

    stage("drawing into an unwatched target");
    set_viewport(context, 256.0f, 144.0f);
    context->OMSetRenderTargets(1, &composite_view, nullptr);
    context->PSSetShaderResources(2, 1, &source_view);
    context->Draw(3, 0);
    check(reports.empty(), "A draw into a target nobody asked about must be reported to nobody.");

    stage("watching the target");
    check(rsf_frame_tap_watch_target(0, composite, 3) == RSF_FRAME_TAP_OK,
          "Setting a watch must succeed.");

    // Two unindexed full screen draws and one indexed draw, which is the shape of a tonemap
    // followed by interface geometry and the thing the report has to be able to tell apart.
    context->Draw(3, 0);
    context->Draw(6, 0);
    context->DrawIndexed(12, 0, 0);

    check(reports.size() == 3, "Three draws into the watched target must produce three reports.");
    if (reports.size() == 3) {
        for (size_t index = 0; index < reports.size(); ++index) {
            const Report& report = reports[index];
            check(report.watch_index == 0, "The report must name the watch slot that matched.");
            check(report.render_target == composite,
                  "The report must name the texture behind the bound render target.");
            check(report.target_width == 256 && report.target_height == 144,
                  "The report must carry the render target's own extent.");
            check(report.viewport_width == 256 && report.viewport_height == 144,
                  "The report must carry the viewport the draw was made under.");
            check(report.inputs.size() == 1 && report.inputs[0].slot == 2 &&
                      report.inputs[0].texture == source,
                  "The report must carry the bound pixel shader inputs with their slot numbers.");
            check(report.inputs.size() == 1 && report.inputs[0].width == 64 &&
                      report.inputs[0].height == 36 &&
                      report.inputs[0].format == DXGI_FORMAT_R16G16B16A16_FLOAT,
                  "An input must carry the shape a caller identifies it by.");
        }
        // The ordinal counts draws into this target since it was bound, and the unwatched draw
        // above was into the same binding, so counting starts from it rather than from the watch.
        check(reports[0].draw_index + 1 == reports[1].draw_index &&
                  reports[1].draw_index + 1 == reports[2].draw_index,
              "The draw ordinal must advance by one per draw into the target.");
        check(reports[0].indexed == 0 && reports[0].element_count == 3,
              "An unindexed draw must be reported as one, with its vertex count.");
        check(reports[1].indexed == 0 && reports[1].element_count == 6,
              "A second unindexed draw must carry its own vertex count.");
        check(reports[2].indexed == 1 && reports[2].element_count == 12,
              "An indexed draw must be reported as one, with its index count.");
    }

    stage("a callback that binds, as a divert will");
    {
        // The watch budget is spent by the draws above, so re-arm with exactly what this stage
        // uses: two draws, one to bind through the callback and one to see what the shadow held
        // afterwards. Exactly two, so the stage leaves the watch spent and the stages below start
        // where they did before this one existed.
        check(rsf_frame_tap_watch_target(0, composite, 2) == RSF_FRAME_TAP_OK,
              "Re-arming the watch must succeed.");
        reports.clear();
        divert_rehearsal.context = context;
        divert_rehearsal.other_target = other_view;
        divert_rehearsal.original_target = composite_view;

        // The pipeline state a classifier decides on. Bound here so the report has something real
        // to carry, since everything downstream identifies a draw by exactly these pointers.
        D3D11_BLEND_DESC blend_description{};
        blend_description.RenderTarget[0].BlendEnable = TRUE;
        blend_description.RenderTarget[0].SrcBlend = D3D11_BLEND_SRC_ALPHA;
        blend_description.RenderTarget[0].DestBlend = D3D11_BLEND_INV_SRC_ALPHA;
        blend_description.RenderTarget[0].BlendOp = D3D11_BLEND_OP_ADD;
        blend_description.RenderTarget[0].SrcBlendAlpha = D3D11_BLEND_ONE;
        blend_description.RenderTarget[0].DestBlendAlpha = D3D11_BLEND_INV_SRC_ALPHA;
        blend_description.RenderTarget[0].BlendOpAlpha = D3D11_BLEND_OP_ADD;
        blend_description.RenderTarget[0].RenderTargetWriteMask = D3D11_COLOR_WRITE_ENABLE_ALL;
        ID3D11BlendState* blend = nullptr;
        check(SUCCEEDED(device->CreateBlendState(&blend_description, &blend)),
              "The test blend state must be created.");
        const FLOAT factor[4] = {0.0f, 0.0f, 0.0f, 0.0f};
        context->OMSetBlendState(blend, factor, 0xffffffffu);
        context->PSSetShader(pixel_shader, nullptr, 0);
        context->VSSetShader(vertex_shader, nullptr, 0);

        context->OMSetRenderTargets(1, &composite_view, nullptr);
        context->PSSetShaderResources(2, 1, &source_view);
        context->Draw(3, 0);
        context->Draw(3, 0);
        divert_rehearsal.context = nullptr;

        if (!reports.empty()) {
            check(reports[0].pixel_shader == pixel_shader,
                  "The report must name the bound pixel shader, which is half of what identifies "
                  "a draw.");
            check(reports[0].vertex_shader == vertex_shader,
                  "And the vertex shader, which the tap already shadowed and never reported.");
            check(reports[0].blend_state == blend,
                  "And the blend state, which is what separates an interface draw that a "
                  "premultiplied layer can represent from one it cannot.");
        }
        context->OMSetBlendState(nullptr, nullptr, 0xffffffffu);
        if (blend) {
            blend->Release();
        }

        check(reports.size() == 2,
              "Both draws must be reported: a callback binding inside the hook must not stop the "
              "next draw being recognised.");
        for (const Report& report : reports) {
            check(report.render_target == composite,
                  "The draw must still be attributed to the target the game bound, not to what "
                  "the callback bound while inside the hook.");
            check(report.inputs.size() == 1 && report.inputs[0].slot == 2 &&
                      report.inputs[0].texture == source,
                  "The callback cleared slot two and the game's binding must survive it in the "
                  "shadow, because what the shadow describes is the game's frame.");
        }
        // Put the bindings back the way the following stages expect them.
        context->OMSetRenderTargets(1, &composite_view, nullptr);
        context->PSSetShaderResources(2, 1, &source_view);
    }

    stage("the candidate prefilter");
    {
        // Nothing named: the frame pays one load per draw and nobody hears about it.
        candidates.clear();
        context->OMSetRenderTargets(1, &other_view, nullptr);
        context->Draw(3, 0);
        check(candidates.empty(),
              "With no candidate sets, no draw is a candidate. This is the state the game is in "
              "before anything has been identified, and it has to cost nothing.");

        // Named by input layout, which is how an interface producer is recognised.
        void* layout_set[] = {test_layout};
        rsf_frame_tap_candidates sets{};
        sets.struct_size = sizeof(sets);
        sets.layouts = layout_set;
        sets.layout_count = 1;
        check(rsf_frame_tap_set_candidates(&sets) == RSF_FRAME_TAP_OK,
              "Setting the candidate sets must succeed.");

        context->IASetInputLayout(test_layout);
        candidates.clear();
        context->Draw(3, 0);
        check(candidates.size() == 1,
              "A draw whose input layout is named must be reported wherever it draws: this report "
              "is about the frame, not about a target somebody watched.");
        if (!candidates.empty()) {
            check(candidates[0].watch_index == RSF_FRAME_TAP_WATCH_SLOTS,
                  "A candidate must not claim a watch slot, or it reads as a watch hit.");
            check(candidates[0].input_layout == test_layout,
                  "And must carry the layout that made it one.");
        }

        context->IASetInputLayout(nullptr);
        candidates.clear();
        context->Draw(3, 0);
        check(candidates.empty(), "A draw with an unnamed layout must not be reported.");

        // Named by reading a widget target. The prefilter looks at the low pixel slots only, which
        // is where a quad reads the interface it is drawing.
        void* widget_set[] = {source};
        sets.layouts = nullptr;
        sets.layout_count = 0;
        sets.widget_targets = widget_set;
        sets.widget_target_count = 1;
        check(rsf_frame_tap_set_candidates(&sets) == RSF_FRAME_TAP_OK,
              "Replacing the sets must succeed.");

        context->PSSetShaderResources(0, 1, &source_view);
        candidates.clear();
        context->Draw(3, 0);
        check(candidates.size() == 1,
              "A draw reading a widget target in a low slot is a candidate. Whether it is drawing "
              "the interface or merely has it left bound is the classifier's question, not this "
              "one: the prefilter's job is to be cheap and to miss nothing.");

        // Slot two as well: earlier stages bound the same texture there, and it is inside the
        // examined range, so leaving it would make the next check pass for the wrong reason.
        ID3D11ShaderResourceView* unbind = nullptr;
        context->PSSetShaderResources(0, 1, &unbind);
        context->PSSetShaderResources(2, 1, &unbind);
        context->PSSetShaderResources(8, 1, &source_view);
        candidates.clear();
        context->Draw(3, 0);
        check(candidates.empty(),
              "The same texture in a slot beyond the examined ones must not be a candidate. Past "
              "a few slots the odds of a stale binding beat the odds of a real read.");
        context->PSSetShaderResources(8, 1, &unbind);

        // Refusal rather than truncation.
        sets.widget_target_count = 100000;
        check(rsf_frame_tap_set_candidates(&sets) == RSF_FRAME_TAP_ERROR_INVALID_ARGUMENT,
              "More candidates than can be held must be refused, not truncated: a set that stops "
              "partway is a rule that stops matching partway.");
        sets.widget_target_count = 1;
        rsf_frame_tap_candidates short_sets{};
        short_sets.struct_size = 4;
        check(rsf_frame_tap_set_candidates(&short_sets) == RSF_FRAME_TAP_ERROR_INVALID_ARGUMENT,
              "A short structure must be refused.");

        check(rsf_frame_tap_set_candidates(nullptr) == RSF_FRAME_TAP_OK,
              "A null argument disarms the prefilter.");
        candidates.clear();
        context->PSSetShaderResources(0, 1, &source_view);
        context->Draw(3, 0);
        check(candidates.empty(), "And nothing is a candidate afterwards.");
        context->PSSetShaderResources(0, 1, &unbind);
    }

    stage("exhausting the budget");
    reports.clear();
    context->Draw(3, 0);
    check(reports.empty(), "A watch must stop reporting once its budget is spent.");

    stage("rebinding to another target");
    check(rsf_frame_tap_watch_target(0, composite, 4) == RSF_FRAME_TAP_OK,
          "Re-arming a watch must succeed.");
    set_viewport(context, 128.0f, 72.0f);
    context->OMSetRenderTargets(1, &other_view, nullptr);
    context->Draw(3, 0);
    check(reports.empty(), "A draw into a different target must not be reported.");

    context->OMSetRenderTargets(1, &composite_view, nullptr);
    context->Draw(3, 0);
    check(reports.size() == 1, "Binding the watched target again must resume reporting.");
    if (reports.size() == 1) {
        check(reports[0].draw_index == 0,
              "The draw ordinal must restart when the target is bound again.");
        check(reports[0].viewport_width == 128,
              "The viewport must be the one in effect at the draw, not the one at the binding.");
    }

    stage("clearing the watch");
    reports.clear();
    check(rsf_frame_tap_watch_target(0, nullptr, 0) == RSF_FRAME_TAP_OK,
          "Clearing a watch must succeed.");
    context->Draw(3, 0);
    check(reports.empty(), "A cleared watch must report nothing.");

    // The substitution plan, which is the half of this module that changes what the game draws.
    // Checked by asking the context what is actually bound after each call, because the whole
    // claim is that the game asked for one thing and another went through.
    stage("planning substitutions");
    ID3D11Texture2D* promoted = make_target(device, 512, 288, DXGI_FORMAT_B8G8R8A8_UNORM);
    ID3D11Texture2D* reconstruction = make_target(device, 512, 288, DXGI_FORMAT_R16G16B16A16_FLOAT);
    check(promoted && reconstruction, "The replacement textures must be created.");
    if (!promoted || !reconstruction) {
        return 1;
    }
    ID3D11RenderTargetView* promoted_target = nullptr;
    ID3D11ShaderResourceView* promoted_resource = nullptr;
    ID3D11ShaderResourceView* reconstruction_resource = nullptr;
    check(SUCCEEDED(device->CreateRenderTargetView(promoted, nullptr, &promoted_target)) &&
              SUCCEEDED(device->CreateShaderResourceView(promoted, nullptr, &promoted_resource)) &&
              SUCCEEDED(device->CreateShaderResourceView(reconstruction, nullptr,
                                                         &reconstruction_resource)),
          "The replacement views must be created.");

    rsf_frame_tap_plan plan{};
    plan.struct_size = sizeof(plan);
    plan.viewport_scale_x = 2.0f;
    plan.viewport_scale_y = 2.0f;
    plan.on_gate = note_gate;
    plan.count = 2;
    // The composite: promoted to a larger target, and ungated.
    plan.items[0].texture = composite;
    plan.items[0].render_view = promoted_target;
    plan.items[0].shader_view = promoted_resource;
    // The scene colour: substituted only once the composite has been bound this frame, which is
    // what keeps a reconstruction out of the passes still drawing the scene.
    plan.items[1].texture = source;
    plan.items[1].shader_view = reconstruction_resource;
    plan.items[1].after_target = composite;

    plan.viewport_scale_x = 0.0f;
    check(rsf_frame_tap_set_plan(&plan) == RSF_FRAME_TAP_ERROR_INVALID_ARGUMENT,
          "A viewport scale of zero must be refused.");
    plan.viewport_scale_x = 2.0f;
    plan.items[1].shader_view = nullptr;
    check(rsf_frame_tap_set_plan(&plan) == RSF_FRAME_TAP_ERROR_INVALID_ARGUMENT,
          "A named texture with nothing to put in its place must be refused.");
    plan.items[1].shader_view = reconstruction_resource;
    check(rsf_frame_tap_set_plan(&plan) == RSF_FRAME_TAP_OK, "A complete plan must be accepted.");

    stage("substituting before the gate opens");
    // A target that is not in the plan, so the gate stays shut and scene colour stays itself.
    context->OMSetRenderTargets(1, &other_view, nullptr);
    context->PSSetShaderResources(2, 1, &source_view);
    check(gates_seen == 0, "A target the plan does not name must not open a gate.");
    check(bound_resource(context, 2) == source_view,
          "A gated substitution must not apply before its gate opens.");

    stage("substituting after the gate opens");
    context->OMSetRenderTargets(1, &composite_view, nullptr);
    check(gates_seen == 1, "Binding the gate's target must open it exactly once.");
    check(bound_target(context) == promoted_target,
          "A planned render target must be replaced by the one the plan names.");
    context->OMSetRenderTargets(1, &composite_view, nullptr);
    check(gates_seen == 1, "A gate must not reopen within the same frame.");

    set_viewport(context, 128.0f, 72.0f);
    D3D11_VIEWPORT actual{};
    UINT actual_count = 1;
    context->RSGetViewports(&actual_count, &actual);
    check(actual_count == 1 && actual.Width == 256.0f && actual.Height == 144.0f,
          "A viewport must be scaled while a substituted target is bound.");

    // Rebinding scene colour now that the gate is open. The same call as before the gate, and a
    // different view reaches the runtime.
    ID3D11ShaderResourceView* nothing = nullptr;
    context->PSSetShaderResources(2, 1, &nothing);
    context->PSSetShaderResources(2, 1, &source_view);
    check(bound_resource(context, 2) == reconstruction_resource,
          "A planned shader resource must be replaced once its gate is open.");

    stage("leaving the substituted target");
    context->OMSetRenderTargets(1, &other_view, nullptr);
    actual_count = 1;
    context->RSGetViewports(&actual_count, &actual);
    check(actual_count == 1 && actual.Width == 128.0f && actual.Height == 72.0f,
          "The game's own viewport must come back when an unsubstituted target is bound.");

    stage("ending the frame");
    check(rsf_frame_tap_end_frame() == RSF_FRAME_TAP_OK, "Ending a frame must succeed.");
    context->PSSetShaderResources(2, 1, &nothing);
    context->PSSetShaderResources(2, 1, &source_view);
    check(bound_resource(context, 2) == source_view,
          "Ending the frame must shut the gates again.");
    context->OMSetRenderTargets(1, &composite_view, nullptr);
    check(gates_seen == 2, "A gate must open again in the next frame.");

    stage("clearing the plan");
    check(rsf_frame_tap_set_plan(nullptr) == RSF_FRAME_TAP_OK, "Clearing a plan must succeed.");
    context->OMSetRenderTargets(1, &other_view, nullptr);
    context->OMSetRenderTargets(1, &composite_view, nullptr);
    check(bound_target(context) == composite_view,
          "A cleared plan must leave the game's own render target alone.");
    check(gates_seen == 2, "A cleared plan must open no further gates.");

    stage("checking status");
    rsf_frame_tap_status status{};
    status.struct_size = sizeof(status);
    check(rsf_frame_tap_get_status(&status) == RSF_FRAME_TAP_OK, "Status must be readable.");
    check(status.installed == 1, "Status must say the tap is installed.");
    check(status.target_draws_reported == 6,
          "Status must count every reported draw, across watches and re-arms.");

    stage("uninstalling");
    check(rsf_frame_tap_uninstall() == RSF_FRAME_TAP_OK, "Uninstalling must succeed.");
    check(rsf_frame_tap_uninstall() == RSF_FRAME_TAP_ERROR_NOT_INSTALLED,
          "Uninstalling twice must be refused.");

    // After the vtable is restored the game's own calls must still work, which is the one failure
    // this whole module cannot be allowed to have.
    stage("drawing after uninstall");
    reports.clear();
    context->OMSetRenderTargets(1, &composite_view, nullptr);
    context->Draw(3, 0);
    check(reports.empty(), "Nothing may be reported once the tap is uninstalled.");

    stage("releasing");
    reconstruction_resource->Release();
    promoted_resource->Release();
    promoted_target->Release();
    reconstruction->Release();
    promoted->Release();
    indices->Release();
    if (test_layout) {
        test_layout->Release();
    }
    pixel_shader->Release();
    vertex_shader->Release();
    source_view->Release();
    other_view->Release();
    composite_view->Release();
    source->Release();
    other->Release();
    composite->Release();
    context->Release();
    device->Release();

    std::fprintf(stderr, passed ? "frame_tap: pass\n" : "frame_tap: fail\n");
    return passed ? 0 : 1;
}
