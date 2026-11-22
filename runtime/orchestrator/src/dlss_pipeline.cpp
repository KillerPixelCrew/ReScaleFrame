// SPDX-License-Identifier: MIT
// Public D3D11 SR pipeline exports and the native leaves behind them. dlss_pipeline.rs owns the
// provider session, backend and quality switching, frame sequencing, reset and fallback, resource
// leases, output publication, the layer feature and teardown. Each leaf below is one existing SDK,
// D3D11, shader-pass or capture call and contains its own exceptions.

#include <rescaleframe/dlss_pipeline.h>

#include <rescaleframe/texture_dump.h>
#include <rescaleframe/native_sr.h>
#include <rescaleframe/native_fg.h>
#include <rescaleframe/motion_resolve.h>
#include <rescaleframe/colour_fidelity.h>
#include <rescaleframe/colour_transport.h>
#include "dlss_pipeline_platform.h"
#include "sr_legacy_adapter.h"
#include "../../graphics/src/sharpening/d3d11.h"
#include <rescaleframe/input_dump.h>

#include <windows.h>

#include <d3d11.h>
#include <wrl/client.h>
#include <cstddef>
#include <new>
#include <vector>

static_assert(sizeof(rsf_dlss_pipeline_setup) == 168 && offsetof(rsf_dlss_pipeline_setup, require_signature) == 24 &&
    offsetof(rsf_dlss_pipeline_setup, motion) == 40 && offsetof(rsf_dlss_pipeline_setup, log) == 88 &&
    offsetof(rsf_dlss_pipeline_setup, engine) == 104 && offsetof(rsf_dlss_pipeline_setup, engine_version_utf8) == 112 &&
    offsetof(rsf_dlss_pipeline_setup, xess_directory_utf8) == 152 &&
    offsetof(rsf_dlss_pipeline_setup, view_space_to_meters) == 160 && offsetof(rsf_dlss_pipeline_setup, standard_depth) == 164);
static_assert(sizeof(rsf_dlss_pipeline_status) == 96 && offsetof(rsf_dlss_pipeline_status, frames_evaluated) == 32 &&
    offsetof(rsf_dlss_pipeline_status, last_result) == 48 && offsetof(rsf_dlss_pipeline_status, layer_frames_evaluated) == 56 &&
    offsetof(rsf_dlss_pipeline_status, layer_width) == 72 && offsetof(rsf_dlss_pipeline_status, layer_last_result) == 80 &&
    offsetof(rsf_dlss_pipeline_status, last_switch_result) == 92);
static_assert(sizeof(rsf_dlss_pipeline_layer) == 40 && offsetof(rsf_dlss_pipeline_layer, width) == 24 &&
    offsetof(rsf_dlss_pipeline_layer, camera) == 32);
static_assert(sizeof(rsf_dlssp_sharpen_frame) == 20);
static_assert(sizeof(rsf_dlssp_platform) == 40 * sizeof(void*));
static_assert(sizeof(rsf_dlss_engine) == 4 && sizeof(rsf_dlss_quality) == 4);
static_assert(RSF_DLSS_PIPELINE_ABI_VERSION == 5u && RSF_FRAME_ASSEMBLY_ABI_VERSION == 1u && RSF_SHARPENING_ABI_VERSION == 1u);
static_assert(RSF_DLSS_ERROR_NOT_SUPPORTED == -7 && RSF_DLSS_QUALITY_NATIVE == 0 && RSF_SR_NONE == 0 &&
    RSF_SR_DLSS == 1 && RSF_SR_XESS == 5 && RSF_TEXTURE_OK == 0 && RSF_MOTION_DECODE_OK == 0);
static_assert(RSF_DUMP_VIEW_RAW == 0 && RSF_DUMP_VIEW_VELOCITY == 1 && RSF_DUMP_VIEW_DECODED_MOTION == 2 &&
    RSF_DUMP_VIEW_HDR == 3 && RSF_DUMP_VIEW_DEPTH == 4 && RSF_DUMP_VIEW_SCALAR == 5 && RSF_DUMP_VIEW_DENSE_MOTION == 7);
static_assert(D3D11_REQ_TEXTURE2D_U_OR_V_DIMENSION == 16384 && D3D11_BIND_UNORDERED_ACCESS == 128 &&
    RSF_D3D11_FORMAT_RG32_FLOAT == DXGI_FORMAT_R32G32_FLOAT && RSF_D3D11_FORMAT_RG16_FLOAT == DXGI_FORMAT_R16G16_FLOAT);
static_assert(RSF_DLSS_PIPELINE_OK == 0 && RSF_DLSS_PIPELINE_ERROR_INVALID_ARGUMENT == -1 &&
    RSF_DLSS_PIPELINE_ERROR_ABI_MISMATCH == -2 && RSF_DLSS_PIPELINE_ERROR_ALREADY_RUNNING == -3 &&
    RSF_DLSS_PIPELINE_ERROR_NOT_RUNNING == -4 && RSF_DLSS_PIPELINE_ERROR_STREAMLINE_FAILED == -5 &&
    RSF_DLSS_PIPELINE_ERROR_NOT_SUPPORTED == -6 && RSF_DLSS_PIPELINE_ERROR_RESOURCE_FAILED == -7 &&
    RSF_DLSS_PIPELINE_ERROR_MOTION_DECODE_FAILED == -8 && RSF_DLSS_PIPELINE_ERROR_FRAME_REFUSED == -9 &&
    RSF_DLSS_PIPELINE_ERROR_EVALUATE_FAILED == -10);

extern "C" {
int32_t rsf_core_dlss_pipeline_start(const rsf_dlssp_platform*, void*, const rsf_dlss_pipeline_setup*, uint32_t);
int32_t rsf_core_dlss_pipeline_on_frame(const rsf_dlssp_platform*, void*, const rsf_dlss_pipeline_frame*);
void* rsf_core_dlss_pipeline_output_texture(void);
int32_t rsf_core_dlss_pipeline_request_dump(const char*);
int32_t rsf_core_dlss_pipeline_get_status(rsf_dlss_pipeline_status*);
int32_t rsf_core_dlss_pipeline_prepare_layer(const rsf_dlssp_platform*, uint32_t, uint32_t, void**);
int32_t rsf_core_dlss_pipeline_on_layer(const rsf_dlssp_platform*, void*, const rsf_dlss_pipeline_layer*);
void* rsf_core_dlss_pipeline_layer_output(void);
void rsf_core_dlss_pipeline_set_colour_correction(uint32_t);
void rsf_core_dlss_pipeline_set_colour_transport(uint32_t);
int32_t rsf_core_dlss_pipeline_set_sharpening(const rsf_sharpening_options*);
int32_t rsf_core_dlss_pipeline_set_preset(const rsf_dlssp_platform*, uint32_t);
int32_t rsf_core_dlss_pipeline_set_quality(const rsf_dlssp_platform*, uint32_t, uint32_t*, uint32_t*);
int32_t rsf_core_dlss_pipeline_resize_output(const rsf_dlssp_platform*, uint32_t, uint32_t, uint32_t*, uint32_t*);
int32_t rsf_core_dlss_pipeline_select_backend(const rsf_dlssp_platform*, uint32_t, uint32_t*, uint32_t*);
int32_t rsf_core_dlss_pipeline_stop(const rsf_dlssp_platform*);
}

namespace {

/* Streamline and the DLSS feature. */
void* present_host() noexcept { return rsf_d3d11_present_host(); }

int32_t share_host(void* host, rsf_dlssp_log_fn sink, void* user) noexcept try
{
    return rsf_dlss_share_host(host, sink, user);
}
catch (...) { return RSF_DLSS_ERROR_LOAD_FAILED; }

int32_t load(const rsf_dlss_setup* setup) noexcept try { return rsf_dlss_load(setup); }
catch (...) { return RSF_DLSS_ERROR_LOAD_FAILED; }

int32_t set_device(void* device) noexcept try { return rsf_dlss_set_device(device); }
catch (...) { return RSF_DLSS_ERROR_INIT_FAILED; }

int32_t query_support(rsf_dlss_support* support) noexcept try { return rsf_dlss_query_support(support); }
catch (...) { return RSF_DLSS_ERROR_INIT_FAILED; }

int32_t plan_render(rsf_dlss_plan* plan) noexcept try { return rsf_dlss_plan_render_size(plan); }
catch (...) { return RSF_DLSS_ERROR_NOT_READY; }

int32_t release_resources() noexcept try { return rsf_dlss_release_resources(); }
catch (...) { return RSF_DLSS_ERROR_NOT_READY; }

int32_t release_viewport(uint32_t viewport) noexcept try { return rsf_dlss_release_viewport(viewport); }
catch (...) { return RSF_DLSS_ERROR_NOT_READY; }

int32_t shutdown_streamline() noexcept try { return rsf_dlss_shutdown(); }
catch (...) { return RSF_DLSS_ERROR_NOT_READY; }

int32_t evaluate_layer(void* context, const rsf_dlss_frame* frame) noexcept try
{
    return rsf_dlss_evaluate(context, frame);
}
catch (...) { return RSF_DLSS_ERROR_FEATURE_FAILED; }

int32_t preset_valid(uint32_t preset) noexcept { return rsf_dlss_preset_valid(preset); }

/* Native FG owner and the native SR owner's cached resources. */
int32_t fg_evaluate(void* context, const rsf_dlss_frame* frame) noexcept try
{
    return rsf_native_fg_evaluate(context, frame);
}
catch (...) { return RSF_DLSS_ERROR_FEATURE_FAILED; }

void fg_capture(void* context, const rsf_dlss_frame* frame) noexcept try { rsf_native_fg_capture(context, frame); }
catch (...) {}

/* D3D11 textures. */
void clear_black(void* raw_device, void* texture) noexcept try
{
    auto* device = static_cast<ID3D11Device*>(raw_device);
    Microsoft::WRL::ComPtr<ID3D11DeviceContext> context;
    Microsoft::WRL::ComPtr<ID3D11RenderTargetView> view;
    device->GetImmediateContext(&context);
    if (context && SUCCEEDED(device->CreateRenderTargetView(static_cast<ID3D11Texture2D*>(texture), nullptr, &view)) &&
        view) {
        const FLOAT black[4] = {0.0f, 0.0f, 0.0f, 1.0f};
        context->ClearRenderTargetView(view.Get(), black);
    }
}
catch (...) {}

void* create_zeroed(void* device, const rsf_d3d11_texture_desc* desc, uint32_t row_pitch) noexcept try
{
    const D3D11_TEXTURE2D_DESC native{desc->width, desc->height, desc->mips, desc->layers,
        static_cast<DXGI_FORMAT>(desc->format), {desc->samples, desc->sample_quality},
        static_cast<D3D11_USAGE>(desc->usage), desc->bind_flags, desc->cpu_access_flags, desc->misc_flags};
    std::vector<uint8_t> zeros(size_t(row_pitch) * size_t(desc->height), 0u);
    D3D11_SUBRESOURCE_DATA initial{};
    initial.pSysMem = zeros.data();
    initial.SysMemPitch = row_pitch;
    ID3D11Texture2D* result = nullptr;
    if (FAILED(static_cast<ID3D11Device*>(device)->CreateTexture2D(&native, &initial, &result))) return nullptr;
    return result;
}
catch (...) { return nullptr; }

void* create_texture(void* device, const rsf_d3d11_texture_desc* desc) noexcept try
{
    return rsf_d3d11_output_create_texture(device, desc);
}
catch (...) { return nullptr; }

void texture_desc(void* texture, rsf_d3d11_texture_desc* desc) noexcept try { rsf_d3d11_output_texture_desc(texture, desc); }
catch (...) { *desc = {}; }

/* Motion decode and dense resolve passes. */
int32_t decode_create(void* device, uint32_t width, uint32_t height, uint32_t format, rsf_dlssp_log_fn sink,
    void** out) noexcept try
{
    rsf_motion_decode_setup setup{};
    setup.struct_size = uint32_t(sizeof(setup));
    setup.abi_version = RSF_MOTION_DECODE_ABI_VERSION;
    setup.width = width;
    setup.height = height;
    setup.output_format = format;
    setup.log = sink;
    rsf_motion_decode* decode = nullptr;
    const rsf_motion_decode_result result = rsf_motion_decode_create(device, &setup, &decode);
    *out = decode;
    return result;
}
catch (...) { *out = nullptr; return RSF_MOTION_DECODE_ERROR_RESOURCE_FAILED; }

int32_t decode_run(void* decode, void* context, void* motion, const rsf_motion_decode_params* params) noexcept try
{
    return rsf_motion_decode_run(static_cast<rsf_motion_decode*>(decode), context, motion, params);
}
catch (...) { return RSF_MOTION_DECODE_ERROR_RESOURCE_FAILED; }

void* decode_texture(void* decode) noexcept try { return rsf_motion_decode_texture(static_cast<rsf_motion_decode*>(decode)); }
catch (...) { return nullptr; }

void decode_destroy(void* decode) noexcept try { rsf_motion_decode_destroy(static_cast<rsf_motion_decode*>(decode)); }
catch (...) {}

void* resolve_create(void* device, uint32_t width, uint32_t height) noexcept try
{
    rsf_motion_resolve* resolve = nullptr;
    return rsf_motion_resolve_create(device, width, height, &resolve) ? resolve : nullptr;
}
catch (...) { return nullptr; }

int32_t resolve_run(void* resolve, void* context, void* motion, void* depth,
    const rsf_motion_resolve_params* params) noexcept try
{
    return rsf_motion_resolve_run(static_cast<rsf_motion_resolve*>(resolve), context, motion, depth, params);
}
catch (...) { return 0; }

void* resolve_motion(void* resolve) noexcept try { return rsf_motion_resolve_motion(static_cast<rsf_motion_resolve*>(resolve)); }
catch (...) { return nullptr; }

void* resolve_depth(void* resolve) noexcept try { return rsf_motion_resolve_depth(static_cast<rsf_motion_resolve*>(resolve)); }
catch (...) { return nullptr; }

void resolve_destroy(void* resolve) noexcept try { rsf_motion_resolve_destroy(static_cast<rsf_motion_resolve*>(resolve)); }
catch (...) {}

/* Colour correction and colour transport passes. */
void* fidelity_create(void* device) noexcept try
{
    rsf_colour_fidelity* pass = nullptr;
    return rsf_colour_fidelity_create(device, &pass) ? pass : nullptr;
}
catch (...) { return nullptr; }

int32_t fidelity_run(void* pass, void* context, void* scene, void* output, void* depth, uint32_t depth_inverted,
    const float jitter[2]) noexcept try
{
    return rsf_colour_fidelity_run(static_cast<rsf_colour_fidelity*>(pass), context, scene, output, depth,
        depth_inverted, jitter);
}
catch (...) { return 0; }

void fidelity_destroy(void* pass) noexcept try { rsf_colour_fidelity_destroy(static_cast<rsf_colour_fidelity*>(pass)); }
catch (...) {}

void* transport_create(void* device) noexcept try
{
    rsf_colour_transport* transport = nullptr;
    return rsf_colour_transport_create(device, &transport) ? transport : nullptr;
}
catch (...) { return nullptr; }

int32_t transport_encode(void* transport, void* context, void* source, void* exposure, uint32_t width,
    uint32_t height, void** encoded) noexcept try
{
    return rsf_colour_transport_encode(static_cast<rsf_colour_transport*>(transport), context, source, exposure,
        width, height, encoded);
}
catch (...) { return 0; }

int32_t transport_decode(void* transport, void* context, void* target, void* exposure, uint32_t width,
    uint32_t height, void* scene, uint32_t scene_width, uint32_t scene_height, const float jitter[2]) noexcept try
{
    return rsf_colour_transport_decode(static_cast<rsf_colour_transport*>(transport), context, target, exposure,
        width, height, scene, scene_width, scene_height, jitter);
}
catch (...) { return 0; }

void transport_destroy(void* transport) noexcept try
{
    rsf_colour_transport_destroy(static_cast<rsf_colour_transport*>(transport));
}
catch (...) {}

/* Sharpening settings and the post-SR pass. */
// One when the options are inactive or the pass ran, zero when an active pass was unavailable or
// refused. No allocation happens while the options are inactive.
int32_t sharpen(void** pass, void* context, void* output, void* motion, void* depth, void* exposure,
    const rsf_sharpening_options* request, const rsf_dlssp_sharpen_frame* frame) noexcept try
{
    if (!rsf_sharpening_requested(request)) return 1;
    rsf_sharpen11_frame mapping{};
    mapping.hdr = 1;
    mapping.exposure = 1.0f;
    mapping.depth_inverted = frame->depth_inverted;
    mapping.motion_x = frame->motion_x;
    mapping.motion_y = frame->motion_y;
    mapping.near_plane = frame->near_plane;
    mapping.far_plane = frame->far_plane;
    void* inputs[]{output, motion, depth, exposure};
    return rsf_core_sharpen11_run(pass, context, inputs, request, &mapping) != 0;
}
catch (...) { return 0; }

void sharpen_destroy(void* pass) noexcept { rsf_core_sharpen11_destroy(pass); }

/* The legacy FSR/XeSS adapter. */
int32_t alternate_create(void* device, uint32_t width, uint32_t height, const char* const* directories,
    float units_to_meters, uint32_t standard_depth, uint32_t auto_exposure, rsf_dlssp_log_fn sink, void* user,
    void** out) noexcept try
{
    rsf_sr_legacy_adapter* adapter = nullptr;
    const rsf_backend_result result = rsf_sr_legacy_create(device, width, height, directories[0], directories[1],
        directories[2], directories[3], units_to_meters, standard_depth, auto_exposure, sink, user, &adapter);
    *out = adapter;
    return result;
}
catch (...) { *out = nullptr; return RSF_BACKEND_ERROR_INIT_FAILED; }

/* Frame capture and the request dump. */
void input_dump(void* context, const char* name, const char* description, void* texture, uint32_t view,
    float scale) noexcept try
{
    rsf_input_dump_texture(context, name, description, texture, view, scale);
}
catch (...) {}

rsf_texture_dump_options dump_options(const char* prefix, uint32_t view, float scale, rsf_dlssp_log_fn sink)
{
    rsf_texture_dump_options options{};
    options.struct_size = uint32_t(sizeof(options));
    options.abi_version = RSF_TEXTURE_DUMP_ABI_VERSION;
    options.output_prefix_utf8 = prefix;
    options.view = view;
    options.scale = scale;
    options.log = sink;
    return options;
}

int32_t dump_bytes(void* device, void* context, void* texture, const char* prefix, uint32_t view, float scale,
    rsf_dlssp_log_fn sink) noexcept try
{
    const auto options = dump_options(prefix, view, scale, sink);
    return rsf_dump_texture_bytes(device, context, texture, &options);
}
catch (...) { return RSF_TEXTURE_ERROR_INVALID_ARGUMENT; }

int32_t dump_image(void* device, void* context, void* texture, const char* prefix, uint32_t view, float scale,
    rsf_dlssp_log_fn sink) noexcept try
{
    const auto options = dump_options(prefix, view, scale, sink);
    rsf_texture_dump_report report{};
    report.struct_size = uint32_t(sizeof(report));
    return rsf_dump_texture(device, context, texture, &options, &report);
}
catch (...) { return RSF_TEXTURE_ERROR_INVALID_ARGUMENT; }

constexpr rsf_dlssp_platform leaves{
    present_host, share_host, load, set_device, query_support, plan_render, release_resources, release_viewport,
    shutdown_streamline, evaluate_layer, preset_valid,
    fg_evaluate, fg_capture, rsf_native_sr_release_resources,
    texture_desc, create_texture, clear_black, create_zeroed,
    decode_create, decode_run, decode_texture, decode_destroy,
    resolve_create, resolve_run, resolve_motion, resolve_depth, resolve_destroy,
    fidelity_create, fidelity_run, fidelity_destroy,
    transport_create, transport_encode, transport_decode, transport_destroy,
    sharpen, sharpen_destroy,
    alternate_create,
    input_dump, dump_bytes, dump_image,
};

} // namespace

extern "C" rsf_dlss_pipeline_result rsf_dlss_pipeline_start_backend(
    void* device, const rsf_dlss_pipeline_setup* setup, uint32_t backend)
{
    return rsf_core_dlss_pipeline_start(&leaves, device, setup, backend);
}

extern "C" rsf_dlss_pipeline_result rsf_dlss_pipeline_start(void* device, const rsf_dlss_pipeline_setup* setup)
{
    return rsf_dlss_pipeline_start_backend(device, setup, RSF_SR_DLSS);
}

extern "C" rsf_dlss_pipeline_result rsf_dlss_pipeline_on_frame(void* context, const rsf_dlss_pipeline_frame* frame)
{
    return rsf_core_dlss_pipeline_on_frame(&leaves, context, frame);
}

extern "C" void* rsf_dlss_pipeline_output_texture(void) { return rsf_core_dlss_pipeline_output_texture(); }

extern "C" rsf_dlss_pipeline_result rsf_dlss_pipeline_request_dump(const char* prefix_utf8)
{
    return rsf_core_dlss_pipeline_request_dump(prefix_utf8);
}

extern "C" rsf_dlss_pipeline_result rsf_dlss_pipeline_get_status(rsf_dlss_pipeline_status* status)
{
    return rsf_core_dlss_pipeline_get_status(status);
}

extern "C" rsf_dlss_pipeline_result rsf_dlss_pipeline_prepare_layer(uint32_t width, uint32_t height, void** output)
{
    return rsf_core_dlss_pipeline_prepare_layer(&leaves, width, height, output);
}

extern "C" rsf_dlss_pipeline_result rsf_dlss_pipeline_on_layer(void* context, const rsf_dlss_pipeline_layer* layer)
{
    return rsf_core_dlss_pipeline_on_layer(&leaves, context, layer);
}

// No caller in the repository; kept as a public entry point. prepare_layer returns the same texture.
extern "C" void* rsf_dlss_pipeline_layer_output(void) { return rsf_core_dlss_pipeline_layer_output(); }

extern "C" void rsf_dlss_pipeline_set_colour_correction(uint32_t enabled)
{
    rsf_core_dlss_pipeline_set_colour_correction(enabled);
}

extern "C" void rsf_dlss_pipeline_set_colour_transport(uint32_t enabled)
{
    rsf_core_dlss_pipeline_set_colour_transport(enabled);
}

extern "C" rsf_dlss_pipeline_result rsf_dlss_pipeline_set_sharpening(const rsf_sharpening_options* options)
{
    return rsf_core_dlss_pipeline_set_sharpening(options);
}

extern "C" rsf_dlss_pipeline_result rsf_dlss_pipeline_set_preset(uint32_t preset)
{
    return rsf_core_dlss_pipeline_set_preset(&leaves, preset);
}

extern "C" rsf_dlss_pipeline_result rsf_dlss_pipeline_set_quality(rsf_dlss_quality quality, uint32_t* render_width,
    uint32_t* render_height)
{
    return rsf_core_dlss_pipeline_set_quality(&leaves, quality, render_width, render_height);
}

extern "C" rsf_dlss_pipeline_result rsf_dlss_pipeline_resize_output(uint32_t width, uint32_t height,
    uint32_t* render_width, uint32_t* render_height)
{
    return rsf_core_dlss_pipeline_resize_output(&leaves, width, height, render_width, render_height);
}

extern "C" rsf_dlss_pipeline_result rsf_dlss_pipeline_select_backend(uint32_t backend, uint32_t* render_width,
    uint32_t* render_height)
{
    return rsf_core_dlss_pipeline_select_backend(&leaves, backend, render_width, render_height);
}

extern "C" rsf_dlss_pipeline_result rsf_dlss_pipeline_stop(void) { return rsf_core_dlss_pipeline_stop(&leaves); }
