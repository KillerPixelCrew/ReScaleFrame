// SPDX-License-Identifier: MIT
// Drive the frame tap's render target watch the way a game's frame tail drives it: bind a target,
// bind inputs, draw, and check that what comes back describes the draw that was made.
//
// The reconstruction input set is recognised from a combination of textures that only a real engine
// frame produces, so what is checked at the end is the mechanism: that the caller's role table and
// nothing else decides which bound textures are the set, not that a particular game's frame is
// recognised. The watch is different: it reports the game's own draw with no rule about what
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

#include <stdbool.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#define COM(object, method, ...) ((object)->lpVtbl->method((object), __VA_ARGS__))
#define COM0(object, method) ((object)->lpVtbl->method((object)))

static bool passed = true;

static void check(bool condition, const char* message)
{
    if (!condition) {
        fprintf(stderr, "%s\n", message);
        passed = false;
    }
}

// This patches vtables and drives a graphics runtime, so a hang is a realistic failure. Announcing
// each stage means a stall says where it stalled rather than nothing at all.
static void stage(const char* what)
{
    fprintf(stderr, "[stage] %s\n", what);
    fflush(stderr);
}

typedef struct Report {
    uint32_t watch_index;
    void* render_target;
    uint32_t target_width;
    uint32_t target_height;
    uint32_t viewport_width;
    uint32_t viewport_height;
    uint32_t draw_index;
    uint32_t indexed;
    uint32_t element_count;
    rsf_frame_tap_input inputs[RSF_FRAME_TAP_MAX_INPUTS];
    uint32_t input_count;
    void* pixel_shader;
    void* vertex_shader;
    void* input_layout;
    void* blend_state;
    void* depth_stencil_state;
    uint32_t vertex_stride;
    uint32_t topology;
} Report;

typedef struct ReportList {
    Report* reports;
    size_t count;
    size_t capacity;
} ReportList;

static ReportList reports;
static ReportList candidates;

static void append_report(ReportList* list, const Report* report)
{
    if (list->count == list->capacity) {
        const size_t capacity = list->capacity ? list->capacity * 2 : 16;
        Report* entries = realloc(list->reports, capacity * sizeof(*entries));
        if (!entries) {
            check(false, "The draw report fixture could not grow.");
            return;
        }
        list->reports = entries;
        list->capacity = capacity;
    }
    list->reports[list->count++] = *report;
}

// Set for one stage only, so the rest of the test keeps its plain behaviour. Null means the
// callback does nothing beyond recording, which is what it did before.
static struct {
    ID3D11DeviceContext* context;
    ID3D11RenderTargetView* other_target;
    ID3D11RenderTargetView* original_target;
} divert_rehearsal;

static void collect_target_draw(void* user, const rsf_frame_tap_target_draw* draw)
{
    (void)user;
    Report report = {0};
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
    check(draw->input_count <= RSF_FRAME_TAP_MAX_INPUTS, "Bounded draw input count.");
    report.input_count = draw->input_count > RSF_FRAME_TAP_MAX_INPUTS
                             ? RSF_FRAME_TAP_MAX_INPUTS : draw->input_count;
    if (report.input_count) {
        memcpy(report.inputs, draw->inputs, report.input_count * sizeof(report.inputs[0]));
    }
    report.pixel_shader = draw->pixel_shader;
    report.vertex_shader = draw->vertex_shader;
    report.input_layout = draw->input_layout;
    report.blend_state = draw->blend_state;
    report.depth_stencil_state = draw->depth_stencil_state;
    report.vertex_stride = draw->vertex_stride;
    report.topology = draw->topology;
    append_report(&reports, &report);

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
        COM(divert_rehearsal.context, OMSetRenderTargets, 1, &elsewhere, NULL);
        // Slot two specifically, the one the game bound and the assertions below read. Clearing an
        // unread slot would let this pass without meaning anything, which an earlier version of
        // this test did.
        ID3D11ShaderResourceView* nothing = NULL;
        COM(divert_rehearsal.context, PSSetShaderResources, 2, 1, &nothing);
        COM(divert_rehearsal.context, OMSetRenderTargets, 1, &divert_rehearsal.original_target, NULL);
    }
}

// Candidate reports, which are about the frame rather than about a watched target.
static void collect_candidate_draw(void* user, const rsf_frame_tap_target_draw* draw)
{
    (void)user;
    Report report = {0};
    report.watch_index = draw->watch_index;
    report.render_target = draw->render_target;
    report.indexed = draw->indexed;
    report.element_count = draw->element_count;
    report.input_layout = draw->input_layout;
    report.pixel_shader = draw->pixel_shader;
    report.vertex_shader = draw->vertex_shader;
    append_report(&candidates, &report);
}

// The verdict the stand-in classifier gives, and how often it was asked.
static rsf_frame_tap_verdict verdict_to_give = RSF_FRAME_TAP_LEAVE;
static uint32_t verdicts_asked = 0;

static rsf_frame_tap_verdict decide_divert(void* user, const rsf_frame_tap_target_draw* draw)
{
    (void)user;
    (void)draw;
    ++verdicts_asked;
    return verdict_to_give;
}

// Whether one pixel of a texture was written. The test's shader emits a constant non-zero colour,
// so a cleared pixel and a drawn one are told apart without caring what the colour is.
static bool pixel_written(ID3D11Device* device, ID3D11DeviceContext* context, ID3D11Texture2D* texture,
                   UINT x, UINT y)
{
    D3D11_TEXTURE2D_DESC description = {0};
    COM(texture, GetDesc, &description);
    D3D11_TEXTURE2D_DESC staging = description;
    staging.Usage = D3D11_USAGE_STAGING;
    staging.BindFlags = 0;
    staging.CPUAccessFlags = D3D11_CPU_ACCESS_READ;
    staging.MiscFlags = 0;
    ID3D11Texture2D* readable = NULL;
    if (FAILED(COM(device, CreateTexture2D, &staging, NULL, &readable)) || !readable) {
        return false;
    }
    COM(context, CopyResource, (ID3D11Resource*)readable, (ID3D11Resource*)texture);
    D3D11_MAPPED_SUBRESOURCE mapped = {0};
    if (FAILED(COM(context, Map, (ID3D11Resource*)readable, 0, D3D11_MAP_READ, 0, &mapped)) || !mapped.pData) {
        COM0(readable, Release);
        return false;
    }
    const uint8_t* row = (const uint8_t*)mapped.pData + (size_t)(y) * mapped.RowPitch;
    const uint8_t* pixel = row + (size_t)(x) * 4u;
    const bool written = pixel[0] != 0 || pixel[1] != 0 || pixel[2] != 0 || pixel[3] != 0;
    COM(context, Unmap, (ID3D11Resource*)readable, 0);
    COM0(readable, Release);
    return written;
}

static uint32_t gates_seen = 0;

static int note_gate(void* user, void* context, void* texture)
{
    (void)user;
    (void)context;
    (void)texture;
    ++gates_seen;
    return 1;
}

// What the runtime actually has bound, as against what the test asked for. The two differing is
// the entire claim a substitution makes, so every check of one is a call to the other.
//
// Both of these hand back a reference and both drop it before returning. Comparing addresses is
// all the caller does, and holding the reference would keep a view alive past the point the test
// releases it, which is where a leak turns into a crash on shutdown instead.
static ID3D11RenderTargetView* bound_target(ID3D11DeviceContext* context)
{
    ID3D11RenderTargetView* view = NULL;
    COM(context, OMGetRenderTargets, 1, &view, NULL);
    if (view) {
        COM0(view, Release);
    }
    return view;
}

static ID3D11ShaderResourceView* bound_resource(ID3D11DeviceContext* context, UINT slot)
{
    ID3D11ShaderResourceView* view = NULL;
    COM(context, PSGetShaderResources, slot, 1, &view);
    if (view) {
        COM0(view, Release);
    }
    return view;
}

static ID3D11Texture2D* make_target(ID3D11Device* device, UINT width, UINT height, DXGI_FORMAT format)
{
    D3D11_TEXTURE2D_DESC description = {0};
    description.Width = width;
    description.Height = height;
    description.MipLevels = 1;
    description.ArraySize = 1;
    description.Format = format;
    description.SampleDesc.Count = 1;
    description.Usage = D3D11_USAGE_DEFAULT;
    description.BindFlags = D3D11_BIND_RENDER_TARGET | D3D11_BIND_SHADER_RESOURCE;
    ID3D11Texture2D* texture = NULL;
    if (FAILED(COM(device, CreateTexture2D, &description, NULL, &texture))) {
        return NULL;
    }
    return texture;
}

// A triangle covering the target, addressed by vertex id, and a constant colour. Neither reads the
// bound shader resource: the tap shadows the binding call, not the shader, so what the pixels do
// with it is beside the point.
const char* const shader_source = "\n"
"float4 vertex_main(uint id : SV_VertexID) : SV_Position\n"
"{\n"
"    return float4(id == 1 ? 3.0 : -1.0, id == 2 ? -3.0 : 1.0, 0.0, 1.0);\n"
"}\n"
"\n"
"float4 pixel_main() : SV_Target\n"
"{\n"
"    return float4(1.0, 0.0, 0.0, 1.0);\n"
"}\n";

typedef HRESULT(WINAPI* compile_fn)(LPCVOID, SIZE_T, LPCSTR, const D3D_SHADER_MACRO*, void*, LPCSTR,
                                    LPCSTR, UINT, UINT, ID3DBlob**, ID3DBlob**);

// Loaded rather than linked, the same way present_blit does it, so nothing here needs the compiler
// import library to build.
static compile_fn load_compiler(void)
{
    const HMODULE module =
        LoadLibraryExW(L"d3dcompiler_47.dll", NULL, LOAD_LIBRARY_SEARCH_SYSTEM32);
    if (!module) {
        return NULL;
    }
    const FARPROC proc = GetProcAddress(module, "D3DCompile");
    compile_fn compile = NULL;
    _Static_assert(sizeof(compile) == sizeof(proc), "Windows function pointer size");
    memcpy(&compile, &proc, sizeof(compile));
    return compile;
}

static ID3DBlob* compile_one(compile_fn compile, const char* entry, const char* target)
{
    ID3DBlob* code = NULL;
    ID3DBlob* errors = NULL;
    const HRESULT compiled = compile(shader_source, strlen(shader_source), "frame_tap_test",
                                     NULL, NULL, entry, target, 0, 0, &code, &errors);
    if (FAILED(compiled) || !code) {
        fprintf(stderr, "%s did not compile: %s\n", entry,
                     errors ? (const char*)COM0(errors, GetBufferPointer) : "no message");
        code = NULL;
    }
    if (errors) {
        COM0(errors, Release);
    }
    return code;
}

static void set_viewport(ID3D11DeviceContext* context, float width, float height)
{
    D3D11_VIEWPORT viewport = {0};
    viewport.Width = width;
    viewport.Height = height;
    viewport.MaxDepth = 1.0f;
    COM(context, RSSetViewports, 1, &viewport);
}

static rsf_ac7_scene_color selection = {0};
static uint32_t input_reports = 0;

static void collect_input_draw(void*, const rsf_frame_tap_target_draw* draw)
{
    ++input_reports;
    rsf_ac7_scene_color_draw(&selection, draw);
}

static void test_composed_color(ID3D11Device* device, ID3D11DeviceContext* context)
{
    stage("selecting composed scene colour from real D3D11 draws");
    ID3D11Texture2D* base = make_target(device, 256, 144, DXGI_FORMAT_R11G11B10_FLOAT);
    ID3D11Texture2D* composed = make_target(device, 256, 144, DXGI_FORMAT_R11G11B10_FLOAT);
    ID3D11Texture2D* later = make_target(device, 256, 144, DXGI_FORMAT_R11G11B10_FLOAT);
    ID3D11Texture2D* layer = make_target(device, 128, 72, DXGI_FORMAT_R16G16B16A16_FLOAT);
    ID3D11Texture2D* tonemap = make_target(device, 256, 144, DXGI_FORMAT_B8G8R8A8_UNORM);
    ID3D11Texture2D* quarter = make_target(device, 128, 72, DXGI_FORMAT_R11G11B10_FLOAT);
    ID3D11ShaderResourceView* base_srv = NULL;
    ID3D11ShaderResourceView* layer_srv = NULL;
    ID3D11ShaderResourceView* composed_srv = NULL;
    ID3D11RenderTargetView* base_rtv = NULL;
    ID3D11RenderTargetView* composed_rtv = NULL;
    ID3D11RenderTargetView* later_rtv = NULL;
    ID3D11RenderTargetView* tonemap_rtv = NULL;
    ID3D11RenderTargetView* quarter_rtv = NULL;
    check(base && composed && later && layer && tonemap && quarter,
          "Composition fixture textures must be created.");
    if (!base || !composed || !later || !layer || !tonemap || !quarter) {
        return;
    }
    check(SUCCEEDED(COM(device, CreateShaderResourceView, (ID3D11Resource*)base, NULL, &base_srv)) &&
              SUCCEEDED(COM(device, CreateShaderResourceView, (ID3D11Resource*)layer, NULL, &layer_srv)) &&
              SUCCEEDED(COM(device, CreateShaderResourceView, (ID3D11Resource*)composed, NULL, &composed_srv)) &&
              SUCCEEDED(COM(device, CreateRenderTargetView, (ID3D11Resource*)base, NULL, &base_rtv)) &&
              SUCCEEDED(COM(device, CreateRenderTargetView, (ID3D11Resource*)composed, NULL, &composed_rtv)) &&
              SUCCEEDED(COM(device, CreateRenderTargetView, (ID3D11Resource*)later, NULL, &later_rtv)) &&
              SUCCEEDED(COM(device, CreateRenderTargetView, (ID3D11Resource*)tonemap, NULL, &tonemap_rtv)) &&
              SUCCEEDED(COM(device, CreateRenderTargetView, (ID3D11Resource*)quarter, NULL, &quarter_rtv)),
          "Composition fixture views must be created.");
    D3D11_TEXTURE2D_DESC depth_desc = {0};
    depth_desc.Width = 256;
    depth_desc.Height = 144;
    depth_desc.MipLevels = 1;
    depth_desc.ArraySize = 1;
    depth_desc.Format = DXGI_FORMAT_D32_FLOAT;
    depth_desc.SampleDesc.Count = 1;
    depth_desc.BindFlags = D3D11_BIND_DEPTH_STENCIL;
    ID3D11Texture2D* depth = NULL;
    ID3D11DepthStencilView* dsv = NULL;
    check(SUCCEEDED(COM(device, CreateTexture2D, &depth_desc, NULL, &depth)) &&
              SUCCEEDED(COM(device, CreateDepthStencilView, (ID3D11Resource*)depth, NULL, &dsv)),
          "Composition depth fixture must be created.");

    rsf_ac7_scene_color_source(&selection, base, context, 256, 144);
    check(rsf_frame_tap_watch_input(base) == RSF_FRAME_TAP_OK, "Input watch must arm.");
    check(rsf_ac7_scene_color_selected(&selection, base) == base,
          "A frame without a recombine must use the original colour.");
    COM(context, OMSetRenderTargets, 1, &composed_rtv, NULL);
    set_viewport(context, 256, 144);
    // Separate calls and high slots exercise inherited bindings rather than an assumed slot zero.
    COM(context, PSSetShaderResources, 30, 1, &base_srv);
    COM(context, PSSetShaderResources, 31, 1, &layer_srv);
    COM(context, DrawIndexed, 12, 0, 0);
    check(rsf_ac7_scene_color_selected(&selection, base) == base,
          "Geometry is not a recombine.");
    set_viewport(context, 128, 72);
    COM(context, DrawIndexed, 3, 0, 0);
    check(rsf_ac7_scene_color_selected(&selection, base) == base,
          "A partial viewport is not the full scene.");
    set_viewport(context, 256, 144);
    COM(context, OMSetRenderTargets, 1, &quarter_rtv, NULL);
    COM(context, DrawIndexed, 3, 0, 0);
    check(rsf_ac7_scene_color_selected(&selection, base) == base,
          "A downsample must not replace scene colour.");
    COM(context, OMSetRenderTargets, 1, &tonemap_rtv, NULL);
    COM(context, DrawIndexed, 3, 0, 0);
    check(rsf_ac7_scene_color_selected(&selection, base) == base,
          "The tonemap must never be selected.");
    COM(context, OMSetRenderTargets, 1, &composed_rtv, NULL);
    COM(context, DrawIndexed, 3, 0, 0);
    check(rsf_ac7_scene_color_selected(&selection, base) == base,
          "A float output after the tonemap boundary must not reopen discovery.");
    rsf_ac7_scene_color_end_frame(&selection);
    COM(context, OMSetRenderTargets, 1, &composed_rtv, dsv);
    COM(context, DrawIndexed, 3, 0, 0);
    check(rsf_ac7_scene_color_selected(&selection, base) == base,
          "A depth-bound draw must not qualify.");
    COM(context, OMSetRenderTargetsAndUnorderedAccessViews, 1, &composed_rtv, NULL, 1, 0,
                                                     NULL, NULL);
    ID3D11ShaderResourceView* empty = NULL;
    COM(context, PSSetShaderResources, 31, 1, &empty);
    COM(context, DrawIndexed, 3, 0, 0);
    check(rsf_ac7_scene_color_selected(&selection, base) == base,
          "A colour-only copy must not qualify as recombination.");
    COM(context, PSSetShaderResources, 31, 1, &layer_srv);
    COM(context, OMSetRenderTargetsAndUnorderedAccessViews,
        D3D11_KEEP_RENDER_TARGETS_AND_DEPTH_STENCIL, NULL, NULL, 1, 0, NULL, NULL);
    COM(context, DrawIndexed, 3, 0, 0);
    check(rsf_ac7_scene_color_selected(&selection, base) == composed,
          "The captured recombine-shaped draw must select the composed target.");
    // A full-output layer is approximately three times the scene at Ultra Performance.
    // The old two-times ceiling lost recombine discovery at that preset.
    ID3D11Texture2D* native_layer = make_target(device, 768, 432, DXGI_FORMAT_R16G16B16A16_FLOAT);
    ID3D11ShaderResourceView* native_layer_srv = NULL;
    check(native_layer && SUCCEEDED(COM(device, CreateShaderResourceView, (ID3D11Resource*)native_layer, NULL, &native_layer_srv)),
          "The full-output layer fixture must be created.");
    if (native_layer_srv) {
        rsf_ac7_scene_color_end_frame(&selection);
        COM(context, PSSetShaderResources, 31, 1, &native_layer_srv);
        COM(context, DrawIndexed, 3, 0, 0);
        check(rsf_ac7_scene_color_selected(&selection, base) == composed,
              "A native layer at three times scene size must retain recombine discovery.");
        COM(context, PSSetShaderResources, 31, 1, &layer_srv);
        COM0(native_layer_srv, Release);
    }
    if (native_layer) {
        COM0(native_layer, Release);
    }
    check(rsf_ac7_scene_color_source(&selection, base, context, 256, 144) == 0 &&
              rsf_ac7_scene_color_selected(&selection, base) == composed,
          "A later qualifying pass carrying the same base must preserve this frame's composition.");
    COM(context, OMSetRenderTargets, 1, &later_rtv, NULL);
    COM(context, Draw, 3, 0);
    check(rsf_ac7_scene_color_selected(&selection, base) == composed,
          "Later readers must not walk the choice down the post-process chain.");
    rsf_ac7_scene_color_end_frame(&selection);
    check(rsf_ac7_scene_color_selected(&selection, base) == base,
          "Holding the allocation must not reuse the preceding frame's selection.");
    // A deferred context shares the hook vtable but must not overwrite immediate-context shadows.
    ID3D11DeviceContext* deferred = NULL;
    check(SUCCEEDED(COM(device, CreateDeferredContext, 0, &deferred)), "Deferred context must be created.");
    if (deferred) {
        COM(deferred, OMSetRenderTargets, 1, &tonemap_rtv, NULL);
        COM(deferred, PSSetShaderResources, 30, 1, &empty);
        COM0(deferred, Release);
    }
    COM(context, Draw, 3, 0);
    check(rsf_ac7_scene_color_selected(&selection, base) == later,
          "A fresh draw must refresh the choice without rearming the persistent watch.");
    rsf_ac7_scene_color_end_frame(&selection);
    COM(context, PSSetShaderResources, 30, 1, &composed_srv);
    const uint32_t before_unrelated = input_reports;
    COM(context, Draw, 3, 0);
    check(input_reports == before_unrelated &&
              rsf_ac7_scene_color_selected(&selection, base) == base,
          "A draw that does not read the original colour must not be followed.");
    COM(context, PSSetShaderResources, 30, 1, &base_srv);
    COM(context, OMSetRenderTargets, 1, &base_rtv, NULL); // implicit SRV unbind
    COM(context, OMSetRenderTargets, 1, &composed_rtv, NULL);
    COM(context, Draw, 3, 0);
    check(rsf_ac7_scene_color_selected(&selection, base) == base,
          "An implicit SRV unbind must not leave a false read in the shadow.");
    COM(context, PSSetShaderResources, 30, 1, &base_srv);
    COM(context, Draw, 3, 0);
    check(rsf_ac7_scene_color_selected(&selection, base) == composed,
          "Explicit rebinding after an implicit unbind must restore discovery.");
    rsf_ac7_scene_color_source(&selection, later, context, 256, 144);
    check(rsf_ac7_scene_color_selected(&selection, later) == later && !selection.composed,
          "Replacing the source must discard the previous resource association.");
    rsf_ac7_scene_color_source(&selection, base, context, 128, 72);
    COM(context, Draw, 3, 0);
    check(rsf_ac7_scene_color_selected(&selection, base) == base,
          "Changing render extent must reject the old full-size association.");
    rsf_frame_tap_watch_input(NULL);
    rsf_ac7_scene_color_clear(&selection);
    COM(context, PSSetShaderResources, 30, 1, &empty);
    COM(context, PSSetShaderResources, 31, 1, &empty);
    COM(context, OMSetRenderTargets, 0, NULL, NULL);
    COM0(dsv, Release);
    COM0(depth, Release);
    COM0(quarter_rtv, Release);
    COM0(tonemap_rtv, Release);
    COM0(later_rtv, Release);
    COM0(composed_rtv, Release);
    COM0(base_rtv, Release);
    COM0(composed_srv, Release);
    COM0(layer_srv, Release);
    COM0(base_srv, Release);
    COM0(quarter, Release);
    COM0(tonemap, Release);
    COM0(layer, Release);
    COM0(later, Release);
    COM0(composed, Release);
    COM0(base, Release);
}

rsf_depth_replay* depth_fixture = NULL;
static rsf_frame_tap_geometry last_geometry = {0};
static uint32_t geometry_reports = 0, depth_draws = 0, geometry_calls = 0, geometry_rejected = 0;

static void collect_geometry(void*, const rsf_frame_tap_geometry* draw)
{
    ++geometry_calls;
    if (!depth_fixture || !rsf_ac7_scene_depth_candidate(draw)) {
        ++geometry_rejected;
        return;
    }
    last_geometry = *draw;
    ++geometry_reports;
    depth_draws += rsf_depth_replay_draw(depth_fixture, draw);
}

static ID3DBlob* make_depth_shader(compile_fn compile, const char* source,
                                  const char* entry, const char* profile)
{
    ID3DBlob* code = NULL;
    ID3DBlob* errors = NULL;
    const HRESULT result = compile(source, strlen(source), "depth_replay_test", NULL,
                                   NULL, entry, profile, 0, 0, &code, &errors);
    check(SUCCEEDED(result) && code, "Depth fixture shaders must compile.");
    if (errors) {
        COM0(errors, Release);
    }
    return code;
}

static ID3D11Buffer* make_depth_buffer(ID3D11Device* device, const void* data,
                                      UINT bytes, UINT flags)
{
    D3D11_BUFFER_DESC desc = {0};
    desc.ByteWidth = bytes;
    desc.BindFlags = flags;
    desc.Usage = D3D11_USAGE_DEFAULT;
    D3D11_SUBRESOURCE_DATA initial = {0};
    initial.pSysMem = data;
    ID3D11Buffer* b = NULL;
    check(SUCCEEDED(COM(device, CreateBuffer, &desc, data ? &initial : NULL, &b)),
          "Depth fixture buffer creation.");
    return b;
}

static void bind_depth_fixture(ID3D11DeviceContext* c, ID3D11VertexShader* vs,
                               ID3D11PixelShader* ps, ID3D11InputLayout* layout,
                               ID3D11Buffer* vb, ID3D11Buffer* ib, ID3D11Buffer* cb,
                               ID3D11ShaderResourceView* high_srv, ID3D11RenderTargetView* rtv,
                               ID3D11DepthStencilView* readonly, ID3D11DepthStencilState* depth_state,
                               ID3D11RasterizerState* raster)
{
    COM(c, VSSetShader, vs, NULL, 0);
    COM(c, PSSetShader, ps, NULL, 0);
    COM(c, IASetInputLayout, layout);
    COM(c, IASetPrimitiveTopology, D3D11_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
    UINT stride = 16, offset = 16;
    COM(c, IASetVertexBuffers, 15, 1, &vb, &stride, &offset);
    COM(c, IASetIndexBuffer, ib, DXGI_FORMAT_R16_UINT, 2);
    COM(c, VSSetConstantBuffers, 13, 1, &cb);
    COM(c, PSSetShaderResources, 100, 1, &high_srv);
    COM(c, OMSetRenderTargets, 1, &rtv, readonly);
    COM(c, OMSetDepthStencilState, depth_state, 37);
    COM(c, RSSetState, raster);
    set_viewport(c, 64, 64);
    D3D11_RECT scissor = {0, 0, 64, 64};
    COM(c, RSSetScissorRects, 1, &scissor);
}

static void set_depth_transform(ID3D11DeviceContext* context, ID3D11Buffer* cb, float x, float z)
{
    const float values[4] = {x, 0, z, 0.4f};
    COM(context, UpdateSubresource, (ID3D11Resource*)cb, 0, NULL, values, 0, 0);
}

static float depth_pixel(ID3D11DeviceContext* context, ID3D11Texture2D* staging,
                         void* texture, UINT x, UINT y)
{
    COM(context, CopyResource, (ID3D11Resource*)staging, (ID3D11Resource*)texture);
    D3D11_MAPPED_SUBRESOURCE mapped = {0};
    if (FAILED(COM(context, Map, (ID3D11Resource*)staging, 0, D3D11_MAP_READ, 0, &mapped))) {
        check(false, "Depth readback mapping must succeed.");
        return -1.0f;
    }
    float value = 0;
    memcpy(&value, (const char*)mapped.pData + y * mapped.RowPitch + x * 8,
                4);
    COM(context, Unmap, (ID3D11Resource*)staging, 0);
    return value;
}

static void test_depth_replay(ID3D11Device* device, ID3D11DeviceContext* context, compile_fn compile)
{
    stage("replaying translucent geometry into seeded depth and reading pixels");
    COM0(context, ClearState);
    depth_fixture = rsf_depth_replay_create(device, 64, 64);
    check(depth_fixture != NULL, "Depth replay targets must prepare before rendering.");
    const char* source = "\n"
"cbuffer Position : register(b13) { float4 transform; };\n"
"float4 vs(float3 position : POSITION) : SV_Position {\n"
"    return float4(position.xy * transform.w + transform.xy, transform.z, 1);\n"
"}\n"
"float4 ps() : SV_Target { return float4(1,0,0,1); }\n";
    ID3DBlob* vs_code = make_depth_shader(compile, source, "vs", "vs_5_0");
    ID3DBlob* ps_code = make_depth_shader(compile, source, "ps", "ps_5_0");
    if (!depth_fixture || !vs_code || !ps_code) {
        passed = false;
        return;
    }
    ID3D11VertexShader* vs = NULL;
    ID3D11PixelShader* ps = NULL;
    ID3D11InputLayout* layout = NULL;
    D3D11_INPUT_ELEMENT_DESC element = {
        "POSITION", 0, DXGI_FORMAT_R32G32B32_FLOAT, 15, 0, D3D11_INPUT_PER_VERTEX_DATA, 0};
    check(SUCCEEDED(COM(device, CreateVertexShader, COM0(vs_code, GetBufferPointer),
                                               COM0(vs_code, GetBufferSize), NULL, &vs)) &&
              SUCCEEDED(COM(device, CreatePixelShader, COM0(ps_code, GetBufferPointer),
                                                  COM0(ps_code, GetBufferSize), NULL, &ps)) &&
              SUCCEEDED(COM(device, CreateInputLayout, &element, 1, COM0(vs_code, GetBufferPointer),
                                                  COM0(vs_code, GetBufferSize), &layout)),
          "Depth fixture shader/layout creation must succeed.");
    COM0(vs_code, Release);
    COM0(ps_code, Release);
    const float vertices[][4] = {
        {9, 9, 0, 0}, {9, 9, 0, 0}, {-0.8f, -0.8f, 0, 0}, {0, 0.8f, 0, 0}, {0.8f, -0.8f, 0, 0}};
    const uint16_t index_values[] = {99, 99, 0, 1, 2};
    ID3D11Buffer* vb = make_depth_buffer(device, vertices, sizeof(vertices), D3D11_BIND_VERTEX_BUFFER);
    ID3D11Buffer* ib = make_depth_buffer(device, index_values, sizeof(index_values), D3D11_BIND_INDEX_BUFFER);
    ID3D11Buffer* cb = make_depth_buffer(device, NULL, 16, D3D11_BIND_CONSTANT_BUFFER);
    ID3D11Texture2D* layer = make_target(device, 64, 64, DXGI_FORMAT_R16G16B16A16_FLOAT);
    ID3D11RenderTargetView* rtv = NULL;
    check(layer && SUCCEEDED(COM(device, CreateRenderTargetView, (ID3D11Resource*)layer, NULL, &rtv)),
          "Depth fixture colour target.");
    D3D11_TEXTURE2D_DESC desc = {0};
    desc.Width = 64;
    desc.Height = 64;
    desc.MipLevels = 1;
    desc.ArraySize = 1;
    desc.Format = DXGI_FORMAT_R32G8X24_TYPELESS;
    desc.SampleDesc.Count = 1;
    desc.BindFlags = D3D11_BIND_DEPTH_STENCIL | D3D11_BIND_SHADER_RESOURCE;
    ID3D11Texture2D* depth = NULL;
    ID3D11Texture2D* staging = NULL;
    check(SUCCEEDED(COM(device, CreateTexture2D, &desc, NULL, &depth)),
          "Opaque depth fixture creation.");
    desc.BindFlags = 0;
    desc.Usage = D3D11_USAGE_STAGING;
    desc.CPUAccessFlags = D3D11_CPU_ACCESS_READ;
    check(SUCCEEDED(COM(device, CreateTexture2D, &desc, NULL, &staging)),
          "Depth staging fixture creation.");
    D3D11_DEPTH_STENCIL_VIEW_DESC view = {0};
    view.Format = DXGI_FORMAT_D32_FLOAT_S8X24_UINT;
    view.ViewDimension = D3D11_DSV_DIMENSION_TEXTURE2D;
    ID3D11DepthStencilView* writable = NULL;
    ID3D11DepthStencilView* readonly = NULL;
    check(SUCCEEDED(COM(device, CreateDepthStencilView, (ID3D11Resource*)depth, &view, &writable)),
          "Writable fixture depth view.");
    view.Flags = D3D11_DSV_READ_ONLY_DEPTH;
    check(SUCCEEDED(COM(device, CreateDepthStencilView, (ID3D11Resource*)depth, &view, &readonly)),
          "Read-only fixture depth view.");
    D3D11_DEPTH_STENCIL_DESC ds = {0};
    ds.DepthEnable = TRUE;
    ds.DepthWriteMask = D3D11_DEPTH_WRITE_MASK_ZERO;
    ds.DepthFunc = D3D11_COMPARISON_GREATER_EQUAL;
    ds.StencilEnable = TRUE;
    ds.StencilReadMask = 255;
    ds.StencilWriteMask = 255;
    ds.FrontFace = (D3D11_DEPTH_STENCILOP_DESC){D3D11_STENCIL_OP_KEEP, D3D11_STENCIL_OP_KEEP, D3D11_STENCIL_OP_REPLACE,
                    D3D11_COMPARISON_ALWAYS};
    ds.BackFace = ds.FrontFace;
    ID3D11DepthStencilState* depth_state = NULL;
    check(SUCCEEDED(COM(device, CreateDepthStencilState, &ds, &depth_state)),
          "Depth-read stencil-write state.");
    D3D11_RASTERIZER_DESC raster_desc = {0};
    raster_desc.FillMode = D3D11_FILL_SOLID;
    raster_desc.CullMode = D3D11_CULL_NONE;
    raster_desc.DepthClipEnable = TRUE;
    raster_desc.ScissorEnable = TRUE;
    ID3D11RasterizerState* raster = NULL;
    check(SUCCEEDED(COM(device, CreateRasterizerState, &raster_desc, &raster)),
          "Depth fixture raster state.");
    ID3D11ShaderResourceView* high_srv = NULL;
    ID3D11Texture2D* high_texture = make_target(device, 8, 8, DXGI_FORMAT_R8G8B8A8_UNORM);
    check(SUCCEEDED(COM(device, CreateShaderResourceView, (ID3D11Resource*)high_texture, NULL, &high_srv)),
          "High slot fixture resource.");
    bind_depth_fixture(context, vs, ps, layout, vb, ib, cb, high_srv, rtv,
                       readonly, depth_state, raster);
    set_depth_transform(context, cb, -0.45f, 0.6f);
    COM(context, ClearDepthStencilView, writable, D3D11_CLEAR_DEPTH | D3D11_CLEAR_STENCIL, 0.25f, 0);
    check(rsf_depth_replay_selected(depth_fixture, context, depth, layer) == depth, "No layer draw must keep the original depth identity.");
    COM(context, DrawIndexed, 3, 1, 1);
    check(depth_draws == 1,
          "One game indexed draw must cause exactly one replay, without recursion.");
    check(last_geometry.vertex_buffers[15] == vb && last_geometry.strides[15] == 16 &&
              last_geometry.offsets[15] == 16 && last_geometry.index_buffer == ib &&
              last_geometry.index_format == DXGI_FORMAT_R16_UINT &&
              last_geometry.index_offset == 2 && last_geometry.input_layout == layout &&
              last_geometry.vertex_shader == vs && last_geometry.vertex_constants[13] == cb &&
              last_geometry.start == 1 && last_geometry.base_vertex == 1,
          "Shadow must include high IA/VS slots, offsets, layout and indexed draw arguments.");
    void* augmented = rsf_depth_replay_selected(depth_fixture, context, depth, layer);
    check(augmented != depth, "A successful matching layer must select the copied depth.");
    check(depth_pixel(context, staging, augmented, 18, 32) == 0.6f && depth_pixel(context, staging, augmented, 2, 2) == 0.25f,
          "Triangle pixels must gain geometry depth while opaque seed survives outside coverage.");
    check(depth_pixel(context, staging, depth, 18, 32) == 0.25f, "Replay must never change the game's opaque depth.");
    set_depth_transform(context, cb, 0.45f, 0.8f);
    COM(context, DrawIndexedInstanced, 3, 1, 1, 1, 0);
    check(depth_draws == 2 && last_geometry.kind == 3, "Indexed instanced draws must replay once.");
    {
        rsf_depth_replay_detail detail = {0};
        rsf_depth_replay_get_detail(depth_fixture, &detail);
        fprintf(stderr,
                     "  [depth_draws=%u kind=%u geometry_calls=%u rejected_by_candidate=%u "
                     "replay_draws=%u replay_refused=%u]\n",
                     (unsigned)depth_draws, (unsigned)last_geometry.kind, geometry_calls,
                     geometry_rejected, (unsigned)detail.draws, (unsigned)detail.refused);
    }
    check(
        depth_pixel(context, staging, augmented, 18, 32) == 0.6f && depth_pixel(context, staging, augmented, 46, 32) == 0.8f,
        "Immediate replay must preserve each draw's constants, and accumulate without reseeding.");
    check(rsf_depth_replay_selected(depth_fixture, context, depth, NULL) == depth &&
              rsf_depth_replay_selected(depth_fixture, context, depth, high_texture) == depth &&
              rsf_depth_replay_selected(depth_fixture, context, high_texture, layer) ==
                  high_texture,
          "Absent/wrong composition layer or different opaque depth must use the original.");
    ID3D11DepthStencilView* actual_dsv = NULL;
    ID3D11RenderTargetView* actual_rtv = NULL;
    COM(context, OMGetRenderTargets, 1, &actual_rtv, &actual_dsv);
    check(actual_rtv == rtv && actual_dsv == readonly,
          "Replay must restore colour and read-only depth targets.");
    COM0(actual_rtv, Release);
    COM0(actual_dsv, Release);
    ID3D11DepthStencilState* actual_ds = NULL;
    UINT reference = 0;
    COM(context, OMGetDepthStencilState, &actual_ds, &reference);
    check(actual_ds == depth_state && reference == 37,
          "Replay must restore stencil reference and depth state.");
    COM0(actual_ds, Release);
    ID3D11PixelShader* actual_ps = NULL;
    COM(context, PSGetShader, &actual_ps, NULL, NULL);
    check(actual_ps == ps, "Replay must restore the pixel shader.");
    COM0(actual_ps, Release);
    ID3D11Buffer* actual_vb = NULL;
    UINT stride = 0, offset = 0;
    COM(context, IAGetVertexBuffers, 15, 1, &actual_vb, &stride, &offset);
    check(actual_vb == vb && stride == 16 && offset == 16 &&
              bound_resource(context, 100) == high_srv,
          "Replay must preserve high vertex/resource bindings outside the broad state helper's "
          "range.");
    COM0(actual_vb, Release);

    stage("matching augmented depth to the layer consumed by composition");
    ID3D11Texture2D* base_color = make_target(device, 64, 64, DXGI_FORMAT_R11G11B10_FLOAT);
    ID3D11Texture2D* combined = make_target(device, 64, 64, DXGI_FORMAT_R11G11B10_FLOAT);
    ID3D11ShaderResourceView* base_srv = NULL;
    ID3D11ShaderResourceView* layer_srv = NULL;
    ID3D11RenderTargetView* combined_rtv = NULL;
    check(SUCCEEDED(COM(device, CreateShaderResourceView, (ID3D11Resource*)base_color, NULL, &base_srv)) &&
              SUCCEEDED(COM(device, CreateShaderResourceView, (ID3D11Resource*)layer, NULL, &layer_srv)) &&
              SUCCEEDED(COM(device, CreateRenderTargetView, (ID3D11Resource*)combined, NULL, &combined_rtv)),
          "Depth composition fixture views.");
    rsf_ac7_scene_color_source(&selection, base_color, context, 64, 64);
    rsf_frame_tap_watch_input(base_color);
    COM(context, OMSetRenderTargets, 1, &combined_rtv, NULL);
    ID3D11ShaderResourceView* composition_inputs[] = {base_srv, layer_srv};
    COM(context, PSSetShaderResources, 0, 2, composition_inputs);
    COM(context, Draw, 3, 1);
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
    rsf_frame_tap_watch_input(NULL);
    ID3D11ShaderResourceView* no_inputs[2] = {0};
    COM(context, PSSetShaderResources, 0, 2, no_inputs);
    COM0(combined_rtv, Release);
    COM0(layer_srv, Release);
    COM0(base_srv, Release);
    COM0(combined, Release);
    COM0(base_color, Release);
    bind_depth_fixture(context, vs, ps, layout, vb, ib, cb, high_srv, rtv,
                       readonly, depth_state, raster);

    stage("depth replay frame reset, occlusion, scissor and refusal");
    rsf_depth_replay_end_frame(depth_fixture);
    check(rsf_depth_replay_selected(depth_fixture, context, depth, layer) == depth, "Frame end must invalidate the previous layer immediately.");
    COM(context, ClearDepthStencilView, writable, D3D11_CLEAR_DEPTH, 0.9f, 0);
    set_depth_transform(context, cb, -0.45f, 0.6f);
    COM(context, Draw, 3, 1);
    check(depth_pixel(context, staging, rsf_depth_replay_selected(depth_fixture, context, depth, layer), 18, 32) == 0.9f,
          "Nearer opaque depth must occlude the replayed triangle.");
    rsf_depth_replay_end_frame(depth_fixture);
    COM(context, ClearDepthStencilView, writable, D3D11_CLEAR_DEPTH, 0.25f, 0);
    D3D11_RECT scissor = {0, 0, 10, 64};
    COM(context, RSSetScissorRects, 1, &scissor);
    COM(context, DrawInstanced, 3, 1, 1, 0);
    check(depth_pixel(context, staging, rsf_depth_replay_selected(depth_fixture, context, depth, layer), 18, 32) == 0.25f, "Replay must retain the game's scissor coverage.");
    rsf_depth_replay_end_frame(depth_fixture);
    COM(context, OMSetRenderTargets, 1, &rtv, writable);
    COM(context, Draw, 3, 1);
    check(rsf_depth_replay_selected(depth_fixture, context, depth, layer) == depth, "A writable depth view must refuse this candidate for the frame.");
    COM(context, OMSetRenderTargets, 1, &rtv, readonly);
    COM(context, Draw, 3, 1);
    check(rsf_depth_replay_selected(depth_fixture, context, depth, layer) == depth, "A later supported draw must not hide an incomplete layer.");
    rsf_depth_replay_end_frame(depth_fixture);

    stage("depth replay shadow reset and foreign context isolation");
    COM0(context, ClearState);
    bind_depth_fixture(context, vs, ps, layout, vb, ib, cb, high_srv, rtv,
                       readonly, depth_state, raster);
    set_depth_transform(context, cb, -0.45f, 0.6f);
    ID3D11DeviceContext* deferred = NULL;
    check(SUCCEEDED(COM(device, CreateDeferredContext, 0, &deferred)), "Deferred fixture context.");
    const uint32_t before = geometry_reports;
    bind_depth_fixture(deferred, vs, ps, layout, vb, ib, cb, high_srv, rtv,
                           readonly, depth_state, raster);
    COM(deferred, Draw, 3, 1);
    check(geometry_reports == before,
          "Foreign-context geometry must not enter the immediate shadow.");
    COM(context, DrawIndexed, 3, 1, 1);
    check(rsf_depth_replay_selected(depth_fixture, context, depth, layer) != depth && last_geometry.vertex_buffers[15] == vb,
          "ClearState must allow correctly rebound immediate geometry to replay.");
    ID3D11CommandList* list = NULL;
    check(SUCCEEDED(COM(deferred, FinishCommandList, FALSE, &list)), "Deferred fixture command list.");
    COM(context, ExecuteCommandList, list, FALSE);
    COM0(list, Release);
    COM0(deferred, Release);
    rsf_depth_replay_end_frame(depth_fixture);
    bind_depth_fixture(context, vs, ps, layout, vb, ib, cb, high_srv, rtv,
                       readonly, depth_state, raster);
    COM(context, DrawIndexed, 3, 1, 1);
    check(rsf_depth_replay_selected(depth_fixture, context, depth, layer) != depth,
          "ExecuteCommandList without restore must invalidate and reseed the shadow.");

    // An indirect observer must borrow the actual argument buffer/offset, not guess direct
    // counts or collapse indexed and nonindexed GPU submissions into one unknown shape.
    const uint32_t indirect_values[10] = {0,0,0,0,3,1,1,1,0,0};
    D3D11_BUFFER_DESC indirect_desc = {0};indirect_desc.ByteWidth=sizeof(indirect_values);
    indirect_desc.MiscFlags=D3D11_RESOURCE_MISC_DRAWINDIRECT_ARGS;
    const D3D11_SUBRESOURCE_DATA indirect_initial = {indirect_values,0,0};
    ID3D11Buffer* indirect=NULL;
    check(SUCCEEDED(COM(device, CreateBuffer, &indirect_desc,&indirect_initial,&indirect)),"Create real indirect draw arguments.");
    if (indirect) {
        COM(context, DrawIndexedInstancedIndirect, indirect,16);
        check(last_geometry.kind==5 && last_geometry.indirect_buffer==indirect && last_geometry.indirect_offset==16,
              "Indexed indirect geometry retains its argument identity and byte offset.");
        COM(context, DrawInstancedIndirect, indirect,16);
        check(last_geometry.kind==6 && last_geometry.indirect_buffer==indirect && last_geometry.indirect_offset==16,
              "Nonindexed indirect geometry keeps its distinct replay contract.");
        COM0(indirect, Release);
    }

    COM0(context, ClearState);
    rsf_depth_replay_destroy(depth_fixture);
    depth_fixture = NULL;
    COM0(high_srv, Release);
    COM0(high_texture, Release);
    COM0(raster, Release);
    COM0(depth_state, Release);
    COM0(readonly, Release);
    COM0(writable, Release);
    COM0(staging, Release);
    COM0(depth, Release);
    COM0(rtv, Release);
    COM0(layer, Release);
    COM0(cb, Release);
    COM0(ib, Release);
    COM0(vb, Release);
    COM0(layout, Release);
    COM0(ps, Release);
    COM0(vs, Release);
}

// The input set as the tap's pass callback reports it.
typedef struct PassRecord {
    unsigned count;
    void* scene_color;
    void* history;
    void* motion;
    void* depth;
    void* exposure;
    uint32_t render_width;
    uint32_t render_height;
    uint32_t scene_color_format;
} PassRecord;

static void collect_pass(void* user, const rsf_frame_tap_pass* pass)
{
    PassRecord* record = user;
    ++record->count;
    record->scene_color = pass->scene_color;
    record->history = pass->history;
    record->motion = pass->motion;
    record->depth = pass->depth;
    record->exposure = pass->exposure;
    record->render_width = pass->render_width;
    record->render_height = pass->render_height;
    record->scene_color_format = pass->scene_color_format;
}

// A role table of the shape a game hands the tap. Which format is which role is the table's
// business; the tap only matches.
const rsf_role_rule test_rules[] = {
    {RSF_ROLE_EXPOSURE, RSF_ROLE_CONFIDENT, RSF_ROLE_RULE_FIXED_SIZE, DXGI_FORMAT_R32G32_FLOAT, 0, 1,
     1},
    {RSF_ROLE_MOTION, RSF_ROLE_CONFIDENT, RSF_ROLE_RULE_RENDER_SIZED, DXGI_FORMAT_R16G16_UNORM,
     D3D11_BIND_RENDER_TARGET, 0, 0},
    {RSF_ROLE_DEPTH, RSF_ROLE_CONFIDENT, RSF_ROLE_RULE_RENDER_SIZED | RSF_ROLE_RULE_ANY_DEPTH_FORMAT,
     0, D3D11_BIND_DEPTH_STENCIL, 0, 0},
    {RSF_ROLE_HISTORY, RSF_ROLE_CANDIDATE, RSF_ROLE_RULE_RENDER_SIZED,
     DXGI_FORMAT_R16G16B16A16_FLOAT, D3D11_BIND_RENDER_TARGET | D3D11_BIND_SHADER_RESOURCE, 0, 0},
    {RSF_ROLE_SCENE_COLOR, RSF_ROLE_CANDIDATE, RSF_ROLE_RULE_RENDER_SIZED,
     DXGI_FORMAT_R11G11B10_FLOAT, D3D11_BIND_RENDER_TARGET, 0, 0},
};
const rsf_role_table test_roles = {sizeof(rsf_role_table), test_rules,
                                   (uint32_t)(sizeof(test_rules) / sizeof(test_rules[0]))};

// Install a tap that wants the input set, with or without a table.
static rsf_frame_tap_result install_for_passes(ID3D11DeviceContext* context, PassRecord* record,
                                        const rsf_role_table* roles)
{
    rsf_frame_tap_options options = {0};
    options.struct_size = sizeof(options);
    options.abi_version = RSF_FRAME_TAP_ABI_VERSION;
    options.on_pass = collect_pass;
    options.on_pass_user = record;
    options.output_width = 512;
    options.output_height = 288;
    options.roles = roles;
    return rsf_frame_tap_install(context, &options);
}

static void bind_pass_inputs(ID3D11DeviceContext* context, ID3D11ShaderResourceView* history,
                             ID3D11ShaderResourceView* colour, ID3D11ShaderResourceView* motion,
                             ID3D11ShaderResourceView* depth, ID3D11ShaderResourceView* exposure)
{
    COM(context, PSSetShaderResources, 1, 1, &history);
    COM(context, PSSetShaderResources, 3, 1, &colour);
    COM(context, PSSetShaderResources, 5, 1, &motion);
    COM(context, PSSetShaderResources, 6, 1, &depth);
    COM(context, PSSetShaderResources, 9, 1, &exposure);
}

int main(void)
{
    stage("validating arguments");
    rsf_frame_tap_options options = {0};
    options.struct_size = sizeof(options);
    options.abi_version = RSF_FRAME_TAP_ABI_VERSION;
    options.on_target_draw = collect_target_draw;
    options.on_input_draw = collect_input_draw;
    options.on_candidate_draw = collect_candidate_draw;
    options.on_geometry = collect_geometry;
    options.output_width = 512;
    options.output_height = 288;

    check(rsf_frame_tap_watch_target(0, NULL, 0) == RSF_FRAME_TAP_ERROR_NOT_INSTALLED,
          "Watching before installing must be refused.");

    stage("creating device");
    ID3D11Device* device = NULL;
    ID3D11DeviceContext* context = NULL;
    D3D_FEATURE_LEVEL level = {0};
    const HRESULT created =
        D3D11CreateDevice(NULL, D3D_DRIVER_TYPE_HARDWARE, NULL, 0, NULL, 0,
                          D3D11_SDK_VERSION, &device, &level, &context);
    if (FAILED(created) || !device || !context) {
        fprintf(stderr, "no D3D11 device available, skipping\n");
        return 0;
    }

    check(rsf_frame_tap_install(context, NULL) == RSF_FRAME_TAP_ERROR_INVALID_ARGUMENT,
          "A missing options structure must be rejected.");
    {
        rsf_role_rule oversized[RSF_FRAME_TAP_MAX_ROLE_RULES + 1] = {0};
        rsf_role_table too_long = {sizeof(rsf_role_table), oversized,
                                   RSF_FRAME_TAP_MAX_ROLE_RULES + 1};
        rsf_role_table short_table = test_roles;
        short_table.struct_size = sizeof(rsf_role_table) - 4;
        rsf_role_table no_rules = test_roles;
        no_rules.rules = NULL;
        rsf_frame_tap_options bad = options;
        bad.roles = &too_long;
        check(rsf_frame_tap_install(context, &bad) == RSF_FRAME_TAP_ERROR_INVALID_ARGUMENT,
              "A role table longer than the tap holds must be refused, not truncated.");
        bad.roles = &short_table;
        check(rsf_frame_tap_install(context, &bad) == RSF_FRAME_TAP_ERROR_INVALID_ARGUMENT,
              "A short role table must be refused.");
        bad.roles = &no_rules;
        check(rsf_frame_tap_install(context, &bad) == RSF_FRAME_TAP_ERROR_INVALID_ARGUMENT,
              "A role table that counts rules and has none must be refused.");
    }
    options.abi_version = RSF_FRAME_TAP_ABI_VERSION + 1u;
    check(rsf_frame_tap_install(context, &options) == RSF_FRAME_TAP_ERROR_ABI_MISMATCH,
          "An incompatible ABI must be rejected.");
    options.abi_version = RSF_FRAME_TAP_ABI_VERSION;

    stage("installing tap");
    check(rsf_frame_tap_install(context, &options) == RSF_FRAME_TAP_OK,
          "Installing the tap must succeed.");
    check(rsf_frame_tap_install(context, &options) == RSF_FRAME_TAP_ERROR_ALREADY_INSTALLED,
          "Installing twice must be refused.");
    check(rsf_frame_tap_watch_target(RSF_FRAME_TAP_WATCH_SLOTS, NULL, 0) ==
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

    ID3D11RenderTargetView* composite_view = NULL;
    ID3D11RenderTargetView* other_view = NULL;
    ID3D11ShaderResourceView* source_view = NULL;
    check(SUCCEEDED(COM(device, CreateRenderTargetView, (ID3D11Resource*)composite, NULL, &composite_view)) &&
              SUCCEEDED(COM(device, CreateRenderTargetView, (ID3D11Resource*)other, NULL, &other_view)) &&
              SUCCEEDED(COM(device, CreateShaderResourceView, (ID3D11Resource*)source, NULL, &source_view)),
          "The test views must be created.");

    stage("compiling shaders");
    const compile_fn compile = load_compiler();
    if (!compile) {
        fprintf(stderr, "d3dcompiler_47.dll is not available, skipping\n");
        return 0;
    }
    ID3DBlob* vertex_code = compile_one(compile, "vertex_main", "vs_5_0");
    ID3DBlob* pixel_code = compile_one(compile, "pixel_main", "ps_5_0");
    check(vertex_code && pixel_code, "The test shaders must compile.");
    if (!vertex_code || !pixel_code) {
        return 1;
    }
    ID3D11VertexShader* vertex_shader = NULL;
    ID3D11PixelShader* pixel_shader = NULL;
    check(SUCCEEDED(COM(device, CreateVertexShader, COM0(vertex_code, GetBufferPointer),
                                               COM0(vertex_code, GetBufferSize), NULL,
                                               &vertex_shader)) &&
              SUCCEEDED(COM(device, CreatePixelShader, COM0(pixel_code, GetBufferPointer),
                                                  COM0(pixel_code, GetBufferSize), NULL,
                                                  &pixel_shader)),
          "The test shaders must be created.");
    // An input layout to name as a candidate. The shader reads nothing from the input assembler,
    // and a declaration may supply more than a shader consumes, so one element is enough: what is
    // being tested is that the tap recognises the object, not what it describes.
    const D3D11_INPUT_ELEMENT_DESC layout_elements[] = {
        {"ATTRIBUTE", 0, DXGI_FORMAT_R32G32B32A32_FLOAT, 0, 0, D3D11_INPUT_PER_VERTEX_DATA, 0},
    };
    ID3D11InputLayout* test_layout = NULL;
    check(SUCCEEDED(COM(device, CreateInputLayout, layout_elements, 1,
                                              COM0(vertex_code, GetBufferPointer),
                                              COM0(vertex_code, GetBufferSize), &test_layout)) &&
              test_layout,
          "The test input layout must be created.");

    COM0(vertex_code, Release);
    COM0(pixel_code, Release);

    // Indices for the one indexed draw. Their values do not matter; the buffer has to exist,
    // because an indexed draw with nothing bound is the same unvalidated state as a draw with no
    // shader.
    const uint16_t index_values[12] = {0, 1, 2, 0, 1, 2, 0, 1, 2, 0, 1, 2};
    D3D11_BUFFER_DESC index_description = {0};
    index_description.ByteWidth = sizeof(index_values);
    index_description.Usage = D3D11_USAGE_IMMUTABLE;
    index_description.BindFlags = D3D11_BIND_INDEX_BUFFER;
    D3D11_SUBRESOURCE_DATA index_data = {0};
    index_data.pSysMem = index_values;
    ID3D11Buffer* indices = NULL;
    check(SUCCEEDED(COM(device, CreateBuffer, &index_description, &index_data, &indices)),
          "The test index buffer must be created.");

    COM(context, VSSetShader, vertex_shader, NULL, 0);
    COM(context, PSSetShader, pixel_shader, NULL, 0);
    COM(context, IASetPrimitiveTopology, D3D11_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
    COM(context, IASetIndexBuffer, indices, DXGI_FORMAT_R16_UINT, 0);

    test_depth_replay(device, context, compile);
    COM(context, VSSetShader, vertex_shader, NULL, 0);
    COM(context, PSSetShader, pixel_shader, NULL, 0);
    COM(context, IASetPrimitiveTopology, D3D11_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
    COM(context, IASetIndexBuffer, indices, DXGI_FORMAT_R16_UINT, 0);
    test_composed_color(device, context);

    stage("drawing into an unwatched target");
    set_viewport(context, 256.0f, 144.0f);
    COM(context, OMSetRenderTargets, 1, &composite_view, NULL);
    COM(context, PSSetShaderResources, 2, 1, &source_view);
    COM(context, Draw, 3, 0);
    check((reports.count == 0), "A draw into a target nobody asked about must be reported to nobody.");

    stage("watching the target");
    check(rsf_frame_tap_watch_target(0, composite, 3) == RSF_FRAME_TAP_OK,
          "Setting a watch must succeed.");

    // Two unindexed full screen draws and one indexed draw, which is the shape of a tonemap
    // followed by interface geometry and the thing the report has to be able to tell apart.
    COM(context, Draw, 3, 0);
    COM(context, Draw, 6, 0);
    COM(context, DrawIndexed, 12, 0, 0);

    check(reports.count == 3, "Three draws into the watched target must produce three reports.");
    fprintf(stderr, "  [reports.count=%u]\n", (unsigned)reports.count);
    if (reports.count == 3) {
        for (size_t index = 0; index < reports.count; ++index) {
            const Report report = reports.reports[index];
            check(report.watch_index == 0, "The report must name the watch slot that matched.");
            check(report.render_target == composite,
                  "The report must name the texture behind the bound render target.");
            check(report.target_width == 256 && report.target_height == 144,
                  "The report must carry the render target's own extent.");
            check(report.viewport_width == 256 && report.viewport_height == 144,
                  "The report must carry the viewport the draw was made under.");
            check(report.input_count == 1 && report.inputs[0].slot == 2 &&
                      report.inputs[0].texture == source,
                  "The report must carry the bound pixel shader inputs with their slot numbers.");
            check(report.input_count == 1 && report.inputs[0].width == 64 &&
                      report.inputs[0].height == 36 &&
                      report.inputs[0].format == DXGI_FORMAT_R16G16B16A16_FLOAT,
                  "An input must carry the shape a caller identifies it by.");
        }
        // The ordinal counts draws into this target since it was bound, and the unwatched draw
        // above was into the same binding, so counting starts from it rather than from the watch.
        check(reports.reports[0].draw_index + 1 == reports.reports[1].draw_index &&
                  reports.reports[1].draw_index + 1 == reports.reports[2].draw_index,
              "The draw ordinal must advance by one per draw into the target.");
        check(reports.reports[0].indexed == 0 && reports.reports[0].element_count == 3,
              "An unindexed draw must be reported as one, with its vertex count.");
        check(reports.reports[1].indexed == 0 && reports.reports[1].element_count == 6,
              "A second unindexed draw must carry its own vertex count.");
        check(reports.reports[2].indexed == 1 && reports.reports[2].element_count == 12,
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
        reports.count = 0;
        divert_rehearsal.context = context;
        divert_rehearsal.other_target = other_view;
        divert_rehearsal.original_target = composite_view;

        // The pipeline state a classifier decides on. Bound here so the report has something real
        // to carry, since everything downstream identifies a draw by exactly these pointers.
        D3D11_BLEND_DESC blend_description = {0};
        blend_description.RenderTarget[0].BlendEnable = TRUE;
        blend_description.RenderTarget[0].SrcBlend = D3D11_BLEND_SRC_ALPHA;
        blend_description.RenderTarget[0].DestBlend = D3D11_BLEND_INV_SRC_ALPHA;
        blend_description.RenderTarget[0].BlendOp = D3D11_BLEND_OP_ADD;
        blend_description.RenderTarget[0].SrcBlendAlpha = D3D11_BLEND_ONE;
        blend_description.RenderTarget[0].DestBlendAlpha = D3D11_BLEND_INV_SRC_ALPHA;
        blend_description.RenderTarget[0].BlendOpAlpha = D3D11_BLEND_OP_ADD;
        blend_description.RenderTarget[0].RenderTargetWriteMask = D3D11_COLOR_WRITE_ENABLE_ALL;
        ID3D11BlendState* blend = NULL;
        check(SUCCEEDED(COM(device, CreateBlendState, &blend_description, &blend)),
              "The test blend state must be created.");
        const FLOAT factor[4] = {0.0f, 0.0f, 0.0f, 0.0f};
        COM(context, OMSetBlendState, blend, factor, 0xffffffffu);
        COM(context, PSSetShader, pixel_shader, NULL, 0);
        COM(context, VSSetShader, vertex_shader, NULL, 0);

        COM(context, OMSetRenderTargets, 1, &composite_view, NULL);
        COM(context, PSSetShaderResources, 2, 1, &source_view);
        COM(context, Draw, 3, 0);
        COM(context, Draw, 3, 0);
        divert_rehearsal.context = NULL;

        if (reports.count != 0) {
            check(reports.reports[0].pixel_shader == pixel_shader,
                  "The report must name the bound pixel shader, which is half of what identifies "
                  "a draw.");
            check(reports.reports[0].vertex_shader == vertex_shader,
                  "And the vertex shader, which the tap already shadowed and never reported.");
            check(reports.reports[0].blend_state == blend,
                  "And the blend state, which is what separates an interface draw that a "
                  "premultiplied layer can represent from one it cannot.");
        }
        COM(context, OMSetBlendState, NULL, NULL, 0xffffffffu);
        if (blend) {
            COM0(blend, Release);
        }

        check(reports.count == 2,
              "Both draws must be reported: a callback binding inside the hook must not stop the "
              "next draw being recognised.");
        for (size_t index = 0; index < reports.count; ++index) {
            const Report report = reports.reports[index];
            check(report.render_target == composite,
                  "The draw must still be attributed to the target the game bound, not to what "
                  "the callback bound while inside the hook.");
            check(report.input_count == 1 && report.inputs[0].slot == 2 &&
                      report.inputs[0].texture == source,
                  "The callback cleared slot two and the game's binding must survive it in the "
                  "shadow, because what the shadow describes is the game's frame.");
        }
        // Put the bindings back the way the following stages expect them.
        COM(context, OMSetRenderTargets, 1, &composite_view, NULL);
        COM(context, PSSetShaderResources, 2, 1, &source_view);
    }

    stage("the candidate prefilter");
    {
        // Nothing named: the frame pays one load per draw and nobody hears about it.
        candidates.count = 0;
        COM(context, OMSetRenderTargets, 1, &other_view, NULL);
        COM(context, Draw, 3, 0);
        check((candidates.count == 0),
              "With no candidate sets, no draw is a candidate. This is the state the game is in "
              "before anything has been identified, and it has to cost nothing.");

        // Named by input layout, which is how an interface producer is recognised.
        void* layout_set[] = {test_layout};
        rsf_frame_tap_candidates sets = {0};
        sets.struct_size = sizeof(sets);
        sets.layouts = layout_set;
        sets.layout_count = 1;
        check(rsf_frame_tap_set_candidates(&sets) == RSF_FRAME_TAP_OK,
              "Setting the candidate sets must succeed.");

        COM(context, IASetInputLayout, test_layout);
        candidates.count = 0;
        COM(context, Draw, 3, 0);
        check(candidates.count == 1,
              "A draw whose input layout is named must be reported wherever it draws: this report "
              "is about the frame, not about a target somebody watched.");
        if (candidates.count != 0) {
            check(candidates.reports[0].watch_index == RSF_FRAME_TAP_WATCH_SLOTS,
                  "A candidate must not claim a watch slot, or it reads as a watch hit.");
            check(candidates.reports[0].input_layout == test_layout,
                  "And must carry the layout that made it one.");
        }

        COM(context, IASetInputLayout, NULL);
        candidates.count = 0;
        COM(context, Draw, 3, 0);
        check((candidates.count == 0), "A draw with an unnamed layout must not be reported.");

        // Named by reading a widget target. The prefilter looks at the low pixel slots only, which
        // is where a quad reads the interface it is drawing.
        void* widget_set[] = {source};
        sets.layouts = NULL;
        sets.layout_count = 0;
        sets.widget_targets = widget_set;
        sets.widget_target_count = 1;
        check(rsf_frame_tap_set_candidates(&sets) == RSF_FRAME_TAP_OK,
              "Replacing the sets must succeed.");

        COM(context, PSSetShaderResources, 0, 1, &source_view);
        candidates.count = 0;
        COM(context, Draw, 3, 0);
        check(candidates.count == 1,
              "A draw reading a widget target in a low slot is a candidate. Whether it is drawing "
              "the interface or merely has it left bound is the classifier's question, not this "
              "one: the prefilter's job is to be cheap and to miss nothing.");

        // Slot two as well: earlier stages bound the same texture there, and it is inside the
        // examined range, so leaving it would make the next check pass for the wrong reason.
        ID3D11ShaderResourceView* unbind = NULL;
        COM(context, PSSetShaderResources, 0, 1, &unbind);
        COM(context, PSSetShaderResources, 2, 1, &unbind);
        COM(context, PSSetShaderResources, 8, 1, &source_view);
        candidates.count = 0;
        COM(context, Draw, 3, 0);
        check((candidates.count == 0),
              "The same texture in a slot beyond the examined ones must not be a candidate. Past "
              "a few slots the odds of a stale binding beat the odds of a real read.");
        COM(context, PSSetShaderResources, 8, 1, &unbind);

        // Refusal rather than truncation.
        sets.widget_target_count = 100000;
        check(rsf_frame_tap_set_candidates(&sets) == RSF_FRAME_TAP_ERROR_INVALID_ARGUMENT,
              "More candidates than can be held must be refused, not truncated: a set that stops "
              "partway is a rule that stops matching partway.");
        sets.widget_target_count = 1;
        rsf_frame_tap_candidates short_sets = {0};
        short_sets.struct_size = 4;
        check(rsf_frame_tap_set_candidates(&short_sets) == RSF_FRAME_TAP_ERROR_INVALID_ARGUMENT,
              "A short structure must be refused.");

        check(rsf_frame_tap_set_candidates(NULL) == RSF_FRAME_TAP_OK,
              "A null argument disarms the prefilter.");
        candidates.count = 0;
        COM(context, PSSetShaderResources, 0, 1, &source_view);
        COM(context, Draw, 3, 0);
        check((candidates.count == 0), "And nothing is a candidate afterwards.");
        COM(context, PSSetShaderResources, 0, 1, &unbind);
    }

    stage("diverting a draw into the layer");
    {
        // The layer stands in for the UI layer: a target of a different extent that the draw was
        // never bound to. What is being checked is that the draw lands there, that everything the
        // divert touched goes back, and that a refusal leaves the draw exactly where it was.
        ID3D11Texture2D* layer = make_target(device, 512, 288, DXGI_FORMAT_R8G8B8A8_UNORM);
        ID3D11RenderTargetView* layer_view = NULL;
        check(layer && SUCCEEDED(COM(device, CreateRenderTargetView, (ID3D11Resource*)layer, NULL, &layer_view)),
              "The stand-in layer must be created.");

        void* layout_set[] = {test_layout};
        rsf_frame_tap_candidates sets = {0};
        sets.struct_size = sizeof(sets);
        sets.layouts = layout_set;
        sets.layout_count = 1;
        rsf_frame_tap_set_candidates(&sets);

        rsf_frame_tap_divert_setup divert = {0};
        divert.struct_size = sizeof(divert);
        divert.layer_target = layer_view;
        divert.layer_width = 512;
        divert.layer_height = 288;
        divert.verdict = decide_divert;
        check(rsf_frame_tap_set_divert(&divert) == RSF_FRAME_TAP_OK, "Arming must succeed.");

        // A viewport covering half the width of a 256x144 target. After the divert it must cover
        // half the width of the layer, because a draw covers a fraction of the frame and the
        // fraction is what has to be preserved.
        COM(context, IASetInputLayout, test_layout);
        COM(context, OMSetRenderTargets, 1, &composite_view, NULL);
        set_viewport(context, 128.0f, 144.0f);
        const D3D11_RECT scissor = {0, 0, 128, 144};
        COM(context, RSSetScissorRects, 1, &scissor);

        const float clear[4] = {0.0f, 0.0f, 0.0f, 0.0f};
        COM(context, ClearRenderTargetView, layer_view, clear);
        COM(context, ClearRenderTargetView, composite_view, clear);

        verdict_to_give = RSF_FRAME_TAP_DIVERT;
        COM(context, Draw, 3, 0);

        // Where the pixels went, which is the only proof that matters. The shader writes a
        // constant colour, so a written pixel is non-zero and an untouched one is not.
        check(pixel_written(device, context, layer, 8, 8),
              "The draw must have landed in the layer. This is the whole primitive: the game asked "
              "for one target and drew into another.");
        check(!pixel_written(device, context, composite, 8, 8),
              "And must not have landed in the target the game bound, or the interface is drawn "
              "twice and the scene is not HUD-less after all.");

        ID3D11RenderTargetView* after = NULL;
        ID3D11DepthStencilView* after_depth = NULL;
        COM(context, OMGetRenderTargets, 1, &after, &after_depth);
        check(after == composite_view,
              "And the game's own target must be back afterwards, or its next draw lands in ours.");
        if (after) {
            COM0(after, Release);
        }
        if (after_depth) {
            COM0(after_depth, Release);
        }

        UINT viewport_count = 1;
        D3D11_VIEWPORT viewport_after = {0};
        COM(context, RSGetViewports, &viewport_count, &viewport_after);
        check(viewport_count == 1 && viewport_after.Width == 128.0f &&
                  viewport_after.Height == 144.0f,
              "The viewport must be restored, not left at the scaled one.");

        UINT scissor_count = 1;
        D3D11_RECT scissor_after = {0};
        COM(context, RSGetScissorRects, &scissor_count, &scissor_after);
        check(scissor_count == 1 && scissor_after.right == 128,
              "And the scissor rectangle, which the pass this grew from used to forget.");

        rsf_frame_tap_status status = {0};
        status.struct_size = sizeof(status);
        rsf_frame_tap_get_status(&status);
        check(status.draws_diverted == 1, "The divert must be counted.");
    fprintf(stderr,
                 "  [draws_diverted=%u divert_refused=%u last_refusal=%u candidate_draws=%u]\n",
                 status.draws_diverted, status.divert_refused, status.divert_last_refusal,
                 status.candidate_draws);

        stage("the viewport is scaled by what the draw covered");
        {
            // The draw covered the left half of a 256 wide target. On a 512 wide layer it has to
            // cover the left half again, not the left quarter: what a draw covers is a fraction of
            // the frame, and preserving pixels instead would put the interface in the corner at a
            // reduced render scale, which is the case this exists for.
            check(pixel_written(device, context, layer, 200, 8),
                  "A pixel inside the scaled half must be written.");
            check(!pixel_written(device, context, layer, 400, 8),
                  "And one beyond it must not: the fraction is preserved, not the pixel count.");
        }

        stage("a refusal leaves the draw where it was");
        {
            // Several targets: moving slot zero would change what the others mean.
            ID3D11RenderTargetView* two[2] = {composite_view, other_view};
            COM(context, OMSetRenderTargets, 2, two, NULL);
            COM(context, ClearRenderTargetView, composite_view, clear);
            verdict_to_give = RSF_FRAME_TAP_DIVERT;
            COM(context, Draw, 3, 0);
            check(pixel_written(device, context, composite, 8, 8),
                  "A draw with several targets must be left where it was.");

            rsf_frame_tap_status refused = {0};
            refused.struct_size = sizeof(refused);
            rsf_frame_tap_get_status(&refused);
            check(refused.divert_refused >= 1 &&
                      refused.divert_last_refusal == RSF_FRAME_TAP_REFUSED_MULTIPLE_TARGETS,
                  "And the refusal must say which rule stopped it, because a refusal that becomes "
                  "common is a rule going wrong and a bare count cannot say which.");
            COM(context, OMSetRenderTargets, 1, &composite_view, NULL);
        }

        stage("a verdict of leave changes nothing");
        {
            COM(context, ClearRenderTargetView, composite_view, clear);
            verdict_to_give = RSF_FRAME_TAP_LEAVE;
            verdicts_asked = 0;
            COM(context, Draw, 3, 0);
            check(verdicts_asked == 1, "The classifier must have been asked.");
    fprintf(stderr, "  [verdicts_asked=%u]\n", (unsigned)verdicts_asked);
            check(pixel_written(device, context, composite, 8, 8),
                  "And its answer of no must cost nothing and change nothing, which is the answer "
                  "for almost every draw in the frame.");
        }

        stage("disarming stops diverting");
        {
            check(rsf_frame_tap_set_divert(NULL) == RSF_FRAME_TAP_OK, "Disarming must succeed.");
            COM(context, ClearRenderTargetView, composite_view, clear);
            verdict_to_give = RSF_FRAME_TAP_DIVERT;
            verdicts_asked = 0;
            COM(context, Draw, 3, 0);
            check(verdicts_asked == 0,
                  "With the divert disarmed the classifier is not even asked, so the whole "
                  "mechanism can be carried switched off.");
            check(pixel_written(device, context, composite, 8, 8), "And the draw is untouched.");
        }

        rsf_frame_tap_set_candidates(NULL);
        COM(context, IASetInputLayout, NULL);
        if (layer_view) {
            COM0(layer_view, Release);
        }
        if (layer) {
            COM0(layer, Release);
        }
    }

    stage("exhausting the budget");
    reports.count = 0;
    COM(context, Draw, 3, 0);
    check((reports.count == 0), "A watch must stop reporting once its budget is spent.");

    stage("rebinding to another target");
    check(rsf_frame_tap_watch_target(0, composite, 4) == RSF_FRAME_TAP_OK,
          "Re-arming a watch must succeed.");
    set_viewport(context, 128.0f, 72.0f);
    COM(context, OMSetRenderTargets, 1, &other_view, NULL);
    COM(context, Draw, 3, 0);
    check((reports.count == 0), "A draw into a different target must not be reported.");

    COM(context, OMSetRenderTargets, 1, &composite_view, NULL);
    COM(context, Draw, 3, 0);
    check(reports.count == 1, "Binding the watched target again must resume reporting.");
    fprintf(stderr, "  [reports.count=%u]\n", (unsigned)reports.count);
    if (reports.count == 1) {
        check(reports.reports[0].draw_index == 0,
              "The draw ordinal must restart when the target is bound again.");
        check(reports.reports[0].viewport_width == 128,
              "The viewport must be the one in effect at the draw, not the one at the binding.");
    }

    stage("clearing the watch");
    reports.count = 0;
    check(rsf_frame_tap_watch_target(0, NULL, 0) == RSF_FRAME_TAP_OK,
          "Clearing a watch must succeed.");
    COM(context, Draw, 3, 0);
    check((reports.count == 0), "A cleared watch must report nothing.");

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
    ID3D11RenderTargetView* promoted_target = NULL;
    ID3D11ShaderResourceView* promoted_resource = NULL;
    ID3D11ShaderResourceView* reconstruction_resource = NULL;
    check(SUCCEEDED(COM(device, CreateRenderTargetView, (ID3D11Resource*)promoted, NULL, &promoted_target)) &&
              SUCCEEDED(COM(device, CreateShaderResourceView, (ID3D11Resource*)promoted, NULL, &promoted_resource)) &&
              SUCCEEDED(COM(device, CreateShaderResourceView, (ID3D11Resource*)reconstruction, NULL,
                                                         &reconstruction_resource)),
          "The replacement views must be created.");

    rsf_frame_tap_plan plan = {0};
    plan.struct_size = sizeof(plan);
    plan.viewport_scale_x = 2.0f;
    plan.viewport_scale_y = 2.0f;
    plan.on_gate = note_gate;
    plan.count = 2;
    // The allocation is a GBuffer earlier in the frame, then becomes the composite.
    plan.items[0].texture = composite;
    plan.items[0].render_view = promoted_target;
    plan.items[0].shader_view = promoted_resource;
    plan.items[0].after_target = composite;
    // The scene colour: substituted only once the composite has been bound this frame, which is
    // what keeps a reconstruction out of the passes still drawing the scene.
    plan.items[1].texture = source;
    plan.items[1].shader_view = reconstruction_resource;
    plan.items[1].after_target = composite;

    plan.viewport_scale_x = 0.0f;
    check(rsf_frame_tap_set_plan(&plan) == RSF_FRAME_TAP_ERROR_INVALID_ARGUMENT,
          "A viewport scale of zero must be refused.");
    plan.viewport_scale_x = 2.0f;
    plan.items[1].shader_view = NULL;
    check(rsf_frame_tap_set_plan(&plan) == RSF_FRAME_TAP_ERROR_INVALID_ARGUMENT,
          "A named texture with nothing to put in its place must be refused.");
    plan.items[1].shader_view = reconstruction_resource;
    check(rsf_frame_tap_set_plan(&plan) == RSF_FRAME_TAP_OK, "A complete plan must be accepted.");

    stage("substituting before the gate opens");
    // A target that is not in the plan, so the gate stays shut and scene colour stays itself.
    COM(context, OMSetRenderTargets, 1, &other_view, NULL);
    COM(context, PSSetShaderResources, 2, 1, &source_view);
    check(gates_seen == 0, "A target the plan does not name must not open a gate.");
    check(bound_resource(context, 2) == source_view,
          "A gated substitution must not apply before its gate opens.");

    stage("a future composite used as a GBuffer remains native");
    D3D11_TEXTURE2D_DESC gbuffer_desc = {0}; COM(composite, GetDesc, &gbuffer_desc);
    ID3D11Texture2D* second_gbuffer = NULL;
    ID3D11RenderTargetView* second_gbuffer_view = NULL;
    check(SUCCEEDED(COM(device, CreateTexture2D, &gbuffer_desc, NULL, &second_gbuffer)) &&
          SUCCEEDED(COM(device, CreateRenderTargetView, (ID3D11Resource*)second_gbuffer, NULL, &second_gbuffer_view)),
          "The MRT regression needs equal-size GBuffer targets.");
    ID3D11RenderTargetView* gbuffer_targets[] = {composite_view, second_gbuffer_view};
    COM(context, OMSetRenderTargets, 2, gbuffer_targets, NULL);
    check(gates_seen == 0 && bound_target(context) == composite_view,
          "An MRT GBuffer bind must neither open the post-process gate nor redirect its target.");
    set_viewport(context, 128.0f, 72.0f);
    D3D11_VIEWPORT native_viewport = {0}; UINT native_count = 1;
    COM(context, RSGetViewports, &native_count, &native_viewport);
    check(native_count == 1 && native_viewport.Width == 128.0f,
          "The GBuffer viewport must not inherit post-process scaling.");
    COM(context, OMSetRenderTargets, 1, &other_view, NULL);
    if (second_gbuffer_view) COM0(second_gbuffer_view, Release);
    if (second_gbuffer) COM0(second_gbuffer, Release);
    ID3D11ShaderResourceView* composite_source_view = NULL;
    check(SUCCEEDED(COM(device, CreateShaderResourceView, (ID3D11Resource*)composite, NULL, &composite_source_view)),
          "The pooled GBuffer allocation must be readable.");
    COM(context, PSSetShaderResources, 2, 1, &composite_source_view);
    check(bound_resource(context, 2) == composite_source_view,
          "Lighting must read the real GBuffer, not the future composite stand-in.");
    ID3D11ShaderResourceView* clear_gbuffer = NULL;
    COM(context, PSSetShaderResources, 2, 1, &clear_gbuffer);
    COM0(composite_source_view, Release);

    stage("substituting after the gate opens");
    COM(context, OMSetRenderTargets, 1, &composite_view, NULL);
    check(gates_seen == 1, "Binding the gate's target must open it exactly once.");
    check(bound_target(context) == promoted_target,
          "A planned render target must be replaced by the one the plan names.");
    COM(context, OMSetRenderTargets, 1, &composite_view, NULL);
    check(gates_seen == 1, "A gate must not reopen within the same frame.");

    set_viewport(context, 128.0f, 72.0f);
    D3D11_VIEWPORT actual = {0};
    UINT actual_count = 1;
    COM(context, RSGetViewports, &actual_count, &actual);
    check(actual_count == 1 && actual.Width == 256.0f && actual.Height == 144.0f,
          "A viewport must be scaled while a substituted target is bound.");

    // Rebinding scene colour now that the gate is open. The same call as before the gate, and a
    // different view reaches the runtime.
    ID3D11ShaderResourceView* nothing = NULL;
    COM(context, PSSetShaderResources, 2, 1, &nothing);
    COM(context, PSSetShaderResources, 2, 1, &source_view);
    check(bound_resource(context, 2) == reconstruction_resource,
          "A planned shader resource must be replaced once its gate is open.");

    stage("leaving the substituted target");
    COM(context, OMSetRenderTargets, 1, &other_view, NULL);
    actual_count = 1;
    COM(context, RSGetViewports, &actual_count, &actual);
    check(actual_count == 1 && actual.Width == 128.0f && actual.Height == 72.0f,
          "The game's own viewport must come back when an unsubstituted target is bound.");

    stage("ending the frame");
    check(rsf_frame_tap_end_frame() == RSF_FRAME_TAP_OK, "Ending a frame must succeed.");
    COM(context, PSSetShaderResources, 2, 1, &nothing);
    COM(context, PSSetShaderResources, 2, 1, &source_view);
    check(bound_resource(context, 2) == source_view,
          "Ending the frame must shut the gates again.");
    COM(context, OMSetRenderTargets, 1, &composite_view, NULL);
    check(gates_seen == 2, "A gate must open again in the next frame.");

    stage("clearing the plan");
    check(rsf_frame_tap_set_plan(NULL) == RSF_FRAME_TAP_OK, "Clearing a plan must succeed.");
    COM(context, OMSetRenderTargets, 1, &other_view, NULL);
    COM(context, OMSetRenderTargets, 1, &composite_view, NULL);
    check(bound_target(context) == composite_view,
          "A cleared plan must leave the game's own render target alone.");
    check(gates_seen == 2, "A cleared plan must open no further gates.");

    stage("checking status");
    rsf_frame_tap_status status = {0};
    status.struct_size = sizeof(status);
    check(rsf_frame_tap_get_status(&status) == RSF_FRAME_TAP_OK, "Status must be readable.");
    check(status.installed == 1, "Status must say the tap is installed.");
    check(status.target_draws_reported == 6,
          "Status must count every reported draw, across watches and re-arms.");
    fprintf(stderr,
                 "  [target_draws_reported=%u candidate_draws=%u draws_diverted=%u calls_seen=%u "
                 "vtable_refreshes=%u]\n",
                 status.target_draws_reported, status.candidate_draws, status.draws_diverted,
                 status.calls_seen, status.vtable_refreshes);
    // Zero under DXVK, whose table is static. On the Windows runtime the Map in the depth replay
    // stage rewrote the table and every draw after it went unobserved until the refresh existed,
    // so a Windows run reporting zero here while the draw checks above passed would be a runtime
    // this test has not met.
    fprintf(stderr, "  [runtime rewrote its vtable %u time%s]\n", status.vtable_refreshes,
                 status.vtable_refreshes == 1 ? "" : "s");

    stage("biasing material samplers");
    {
        D3D11_SAMPLER_DESC desc = {0};
        desc.Filter = D3D11_FILTER_ANISOTROPIC; desc.MaxAnisotropy = 8;
        desc.AddressU = desc.AddressV = desc.AddressW = D3D11_TEXTURE_ADDRESS_WRAP;
        desc.MaxLOD = D3D11_FLOAT32_MAX;
        ID3D11SamplerState* material = NULL;
        check(SUCCEEDED(COM(device, CreateSamplerState, &desc, &material)), "Creating a material sampler must succeed.");
        desc.Filter = D3D11_FILTER_MIN_MAG_MIP_POINT; desc.MaxAnisotropy = 1;
        ID3D11SamplerState* point = NULL;
        check(SUCCEEDED(COM(device, CreateSamplerState, &desc, &point)), "Creating a point sampler must succeed.");
        ID3D11SamplerState* set[] = {material, point};
        COM(context, PSSetSamplers, 0, 2, set);
        check(rsf_frame_tap_set_sampler_bias(-1.25f) == RSF_FRAME_TAP_OK, "Setting a bias must succeed.");
        ID3D11SamplerState* bound[3] = {0};
        COM(context, PSGetSamplers, 0, 2, bound);
        D3D11_SAMPLER_DESC seen = {0};
        if (bound[0]) COM(bound[0], GetDesc, &seen);
        check(bound[0] && bound[0] != material && seen.MipLODBias == -1.25f && seen.Filter == D3D11_FILTER_ANISOTROPIC,
              "An already bound mip-filtered sampler must be replaced by a biased clone.");
        check(bound[1] == point, "A point-mip sampler must be left alone.");
        // The engine rebinding its own sampler inside the scope also receives the clone.
        COM(context, PSSetSamplers, 2, 1, &material);
        COM(context, PSGetSamplers, 2, 1, &bound[2]);
        check(bound[2] == bound[0], "A sampler set inside the scope must be biased too.");
        for (size_t index = 0; index < sizeof(bound) / sizeof(bound[0]); ++index) {
            if (bound[index]) {
                COM0(bound[index], Release);
                bound[index] = NULL;
            }
        }
        check(rsf_frame_tap_set_sampler_bias(0.0f) == RSF_FRAME_TAP_OK, "Clearing the bias must succeed.");
        COM(context, PSGetSamplers, 0, 3, bound);
        check(bound[0] == material && bound[1] == point && bound[2] == material,
              "Clearing the bias must restore the game's own samplers in every slot.");
        for (size_t index = 0; index < sizeof(bound) / sizeof(bound[0]); ++index) {
            if (bound[index]) {
                COM0(bound[index], Release);
            }
        }
        check(rsf_frame_tap_set_sampler_bias(0.5f) == RSF_FRAME_TAP_ERROR_INVALID_ARGUMENT,
              "A positive bias must be refused.");
        ID3D11SamplerState* none[3] = {0};
        COM(context, PSSetSamplers, 0, 3, none);
        COM0(material, Release); COM0(point, Release);
    }

    stage("uninstalling");
    check(rsf_frame_tap_uninstall() == RSF_FRAME_TAP_OK, "Uninstalling must succeed.");
    check(rsf_frame_tap_uninstall() == RSF_FRAME_TAP_ERROR_NOT_INSTALLED,
          "Uninstalling twice must be refused.");

    // After the vtable is restored the game's own calls must still work, which is the one failure
    // this whole module cannot be allowed to have.
    stage("drawing after uninstall");
    reports.count = 0;
    COM(context, OMSetRenderTargets, 1, &composite_view, NULL);
    COM(context, Draw, 3, 0);
    check((reports.count == 0), "Nothing may be reported once the tap is uninstalled.");

    // Nothing reads the shader resource shadow in a tap with no callbacks, so it is not kept; when
    // something starts to read it, it is seeded from what the context already has bound.
    stage("a shadow wanted late is seeded from the context");
    {
        rsf_frame_tap_options bare = {0};
        bare.struct_size = sizeof(bare);
        bare.abi_version = RSF_FRAME_TAP_ABI_VERSION;
        check(rsf_frame_tap_install(context, &bare) == RSF_FRAME_TAP_OK,
              "A tap with no callbacks must install.");
        reports.count = 0;
        set_viewport(context, 128.0f, 72.0f);
        COM(context, OMSetRenderTargets, 1, &other_view, NULL);
        COM(context, PSSetShaderResources, 2, 1, &source_view);
        COM(context, Draw, 3, 0);
        check(rsf_frame_tap_set_research_callbacks(collect_target_draw, NULL, NULL) ==
                  RSF_FRAME_TAP_OK,
              "Research callbacks must be accepted.");
        COM(context, Draw, 3, 0);
        check(reports.count == 1 && reports.reports[0].input_count == 1 &&
                  reports.reports[0].inputs[0].slot == 2 && reports.reports[0].inputs[0].texture == source,
              "A binding made before anything read the shadow must be seen once something does.");
        rsf_frame_tap_set_research_callbacks(NULL, NULL, NULL);
        check(rsf_frame_tap_uninstall() == RSF_FRAME_TAP_OK, "Uninstalling must succeed.");
        ID3D11ShaderResourceView* no_view = NULL;
        COM(context, PSSetShaderResources, 2, 1, &no_view);
    }

    // The input set is whatever the caller's role table says it is. Sizes are judged against the
    // 512x288 presented size, so 384x216 is a render scale of three quarters.
    stage("recognising the input set through a role table");
    {
        ID3D11Texture2D* motion = make_target(device, 384, 216, DXGI_FORMAT_R16G16_UNORM);
        ID3D11Texture2D* colour = make_target(device, 384, 216, DXGI_FORMAT_R11G11B10_FLOAT);
        ID3D11Texture2D* history = make_target(device, 384, 216, DXGI_FORMAT_R16G16B16A16_FLOAT);
        ID3D11Texture2D* exposure = make_target(device, 1, 1, DXGI_FORMAT_R32G32_FLOAT);
        D3D11_TEXTURE2D_DESC depth_description = {0};
        depth_description.Width = 384;
        depth_description.Height = 216;
        depth_description.MipLevels = 1;
        depth_description.ArraySize = 1;
        depth_description.Format = DXGI_FORMAT_R32_TYPELESS;
        depth_description.SampleDesc.Count = 1;
        depth_description.Usage = D3D11_USAGE_DEFAULT;
        depth_description.BindFlags = D3D11_BIND_DEPTH_STENCIL | D3D11_BIND_SHADER_RESOURCE;
        ID3D11Texture2D* depth = NULL;
        COM(device, CreateTexture2D, &depth_description, NULL, &depth);
        check(motion && colour && history && exposure && depth,
              "The input set textures must be created.");
        if (motion && colour && history && exposure && depth) {
            D3D11_SHADER_RESOURCE_VIEW_DESC depth_view_description = {0};
            depth_view_description.Format = DXGI_FORMAT_R32_FLOAT;
            depth_view_description.ViewDimension = D3D11_SRV_DIMENSION_TEXTURE2D;
            depth_view_description.Texture2D.MipLevels = 1;
            ID3D11ShaderResourceView *motion_view = NULL, *colour_view = NULL,
                                     *history_view = NULL, *exposure_view = NULL,
                                     *depth_view = NULL;
            check(SUCCEEDED(COM(device, CreateShaderResourceView, (ID3D11Resource*)motion, NULL, &motion_view)) &&
                      SUCCEEDED(COM(device, CreateShaderResourceView, (ID3D11Resource*)colour, NULL, &colour_view)) &&
                      SUCCEEDED(COM(device, CreateShaderResourceView, (ID3D11Resource*)history, NULL, &history_view)) &&
                      SUCCEEDED(
                          COM(device, CreateShaderResourceView, (ID3D11Resource*)exposure, NULL, &exposure_view)) &&
                      SUCCEEDED(COM(device, CreateShaderResourceView, (ID3D11Resource*)depth, &depth_view_description,
                                                                 &depth_view)),
                  "The input set views must be created.");
            // One slot at a time, the way the engine binds them.
            set_viewport(context, 128.0f, 72.0f);

            PassRecord record = {0};
            check(install_for_passes(context, &record, &test_roles) == RSF_FRAME_TAP_OK,
                  "A tap with a role table must install.");
            COM(context, OMSetRenderTargets, 1, &other_view, NULL);
            bind_pass_inputs(context, history_view, colour_view, motion_view, depth_view,
                             exposure_view);
            COM(context, Draw, 3, 0);
            check(record.count == 1, "The set must be recognised once when it is first drawn with.");
            check(record.scene_color == colour && record.history == history &&
                      record.motion == motion && record.depth == depth &&
                      record.exposure == exposure,
                  "Each role must come back as the texture the table matched to it.");
            check(record.render_width == 384 && record.render_height == 216 &&
                      record.scene_color_format == DXGI_FORMAT_R11G11B10_FLOAT,
                  "The render size is the motion target's and the format is the colour's.");
            COM(context, Draw, 3, 0);
            check(record.count == 1, "A set that stays bound is recognised once, not per draw.");
            ID3D11ShaderResourceView* none = NULL;
            COM(context, PSSetShaderResources, 5, 1, &none);
            COM(context, Draw, 3, 0);
            COM(context, PSSetShaderResources, 5, 1, &motion_view);
            COM(context, Draw, 3, 0);
            check(record.count == 2, "The set must be recognised again once it has been broken.");
            check(rsf_frame_tap_uninstall() == RSF_FRAME_TAP_OK, "Uninstalling must succeed.");

            // The same bindings and no table: the tap decides nothing about formats itself.
            PassRecord unrecognised = {0};
            check(install_for_passes(context, &unrecognised, NULL) == RSF_FRAME_TAP_OK,
                  "A tap with no role table must install.");
            bind_pass_inputs(context, history_view, colour_view, motion_view, depth_view,
                             exposure_view);
            COM(context, Draw, 3, 0);
            check(unrecognised.count == 0, "A tap given no role table must recognise nothing.");
            check(rsf_frame_tap_uninstall() == RSF_FRAME_TAP_OK, "Uninstalling must succeed.");

            const UINT slots[] = {1, 3, 5, 6, 9};
            for (size_t index = 0; index < sizeof(slots) / sizeof(slots[0]); ++index) {
                COM(context, PSSetShaderResources, slots[index], 1, &none);
            }
            ID3D11ShaderResourceView* views[] = {motion_view, colour_view, history_view, exposure_view, depth_view};
            for (size_t index = 0; index < sizeof(views) / sizeof(views[0]); ++index) {
                ID3D11ShaderResourceView* view = views[index];
                if (view) {
                    COM0(view, Release);
                }
            }
        }
        ID3D11Texture2D* textures[] = {motion, colour, history, exposure, depth};
        for (size_t index = 0; index < sizeof(textures) / sizeof(textures[0]); ++index) {
            ID3D11Texture2D* texture = textures[index];
            if (texture) {
                COM0(texture, Release);
            }
        }
    }

    stage("releasing");
    COM0(reconstruction_resource, Release);
    COM0(promoted_resource, Release);
    COM0(promoted_target, Release);
    COM0(reconstruction, Release);
    COM0(promoted, Release);
    COM0(indices, Release);
    if (test_layout) {
        COM0(test_layout, Release);
    }
    COM0(pixel_shader, Release);
    COM0(vertex_shader, Release);
    COM0(source_view, Release);
    COM0(other_view, Release);
    COM0(composite_view, Release);
    COM0(source, Release);
    COM0(other, Release);
    COM0(composite, Release);
    COM0(context, Release);
    COM0(device, Release);

    free(candidates.reports);
    free(reports.reports);
    fprintf(stderr, passed ? "frame_tap: pass\n" : "frame_tap: fail\n");
    return passed ? 0 : 1;
}
