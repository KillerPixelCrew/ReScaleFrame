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
};

std::vector<Report> reports;

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
    reports.push_back(report);
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

} // namespace

int main()
{
    stage("validating arguments");
    rsf_frame_tap_options options{};
    options.struct_size = sizeof(options);
    options.abi_version = RSF_FRAME_TAP_ABI_VERSION;
    options.on_target_draw = collect_target_draw;
    options.on_input_draw = collect_input_draw;
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
    check(status.target_draws_reported == 4,
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
