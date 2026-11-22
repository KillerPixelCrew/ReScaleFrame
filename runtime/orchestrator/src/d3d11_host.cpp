// SPDX-License-Identifier: MIT
// Stable D3D11 host exports and the native leaf calls behind them. Admission,
// callback leases, lifecycle, recovery and status live in d3d11_host.rs.
#include "d3d11_host_platform.h"
#include "dlss_pipeline_platform.h"
#include "d3d11_output_platform.h"
#include <rescaleframe/native_sr.h>
#include <rescaleframe/native_cpu.h>
#include <rescaleframe/native_scene.h>
#include <rescaleframe/native_fg.h>
#include <rescaleframe/d3d11_present_bridge.h>
#include <rescaleframe/dlss_pipeline.h>
#include <rescaleframe/overlay_input.h>
#include <rescaleframe/input_dump.h>
#include <rescaleframe/frame_generation.h>
#include "render_settings_store.h"
#include "native_sr_completion.h"
#include <windows.h>
#include <d3d11.h>
#include <dxgi.h>
#include <wrl/client.h>
#include <cstddef>

extern "C" {
const rsf_aux_platform* rsf_auxiliary_sr_platform(void);
int32_t rsf_core_d3d11_host_create(const rsf_d3d11h_platform*, const rsf_d3d11_host_setup*, rsf_game_host_services*);
}

namespace {
using Microsoft::WRL::ComPtr;

// Layouts mirrored by hand in d3d11_host_abi.rs.
static_assert(sizeof(rsf_d3d11_host_setup) == 168 && offsetof(rsf_d3d11_host_setup, depth_far_plane) == 160 && offsetof(rsf_d3d11_host_setup, engine_type) == 48 &&
    offsetof(rsf_d3d11_host_setup, units_to_meters) == 72 && offsetof(rsf_d3d11_host_setup, motion) == 80 &&
    offsetof(rsf_d3d11_host_setup, quality_choices) == 128 && offsetof(rsf_d3d11_host_setup, accept_window) == 136 &&
    offsetof(rsf_d3d11_host_setup, capabilities) == 152);
static_assert(sizeof(rsf_d3d11_present_dispatch) == 32 && offsetof(rsf_d3d11_present_dispatch, native_result) == 28);
static_assert(sizeof(rsf_d3d11_chain_dispatch) == 72 && offsetof(rsf_d3d11_chain_dispatch, receiver) == 16 &&
    offsetof(rsf_d3d11_chain_dispatch, args) == 24 && offsetof(rsf_d3d11_chain_dispatch, result) == 64);
static_assert(sizeof(rsf_d3d11_native_chain) == 32 && sizeof(rsf_d3d11_creation_scope) == 24 &&
    sizeof(rsf_d3d11_presentation_binding) == 16 && sizeof(rsf_d3d11_capture_control) == 16);
static_assert(sizeof(rsf_d3d11_host_capture) == 64 && offsetof(rsf_d3d11_host_capture, view) == 56);
static_assert(sizeof(rsf_d3d11_auxiliary_pass) == 32 && offsetof(rsf_d3d11_auxiliary_pass, surfaces) == 24 &&
    sizeof(rsf_d3d11_auxiliary_scope) == 32);
static_assert(sizeof(rsf_d3d11_reduced_frame) == 56 && offsetof(rsf_d3d11_reduced_frame, token) == 16 &&
    offsetof(rsf_d3d11_reduced_frame, owner) == 24 && offsetof(rsf_d3d11_reduced_frame, surfaces) == 40 &&
    offsetof(rsf_d3d11_reduced_frame, temporal_result) == 48);
static_assert(sizeof(rsf_d3d11_sampler_bias) == 160 && offsetof(rsf_d3d11_sampler_bias, slot_mask) == 144 &&
    offsetof(rsf_d3d11_sampler_bias, applied_mask) == 152);
static_assert(sizeof(rsf_d3d11h_strings) == 64 &&
    sizeof(rsf_d3d11h_pipeline_start) == 48 && sizeof(rsf_d3d11h_pipeline_status) == 20);
static_assert(sizeof(rsf_d3d11h_platform) == 68 * sizeof(void*) &&
    offsetof(rsf_d3d11h_platform, present_install) == 9 * sizeof(void*) &&
    offsetof(rsf_d3d11h_platform, fg_options_set) == 17 * sizeof(void*) &&
    offsetof(rsf_d3d11h_platform, wrap_native) == 23 * sizeof(void*) &&
    offsetof(rsf_d3d11h_platform, com_release) == 26 * sizeof(void*) &&
    offsetof(rsf_d3d11h_platform, auxiliary) == 67 * sizeof(void*));
// Existing Rust layouts in fg12_abi.rs and render_abi.rs.
static_assert(sizeof(rsf_d3d11_present_setup) == 184 && alignof(rsf_d3d11_present_setup) == 8 &&
    sizeof(rsf_native_fg_options) == 24 && alignof(rsf_native_fg_options) == 4);
static_assert(RSF_UI_MODE_BACKBUFFER_HUDLESS == 4);
static_assert(RSF_DUMP_VIEW_RAW == 0 && RSF_DUMP_VIEW_HDR == 3 && RSF_DUMP_VIEW_DEPTH == 4 &&
    RSF_DUMP_VIEW_DENSE_MOTION == 7 && DXGI_PRESENT_TEST == 1 && RSF_SCREEN_GAMEPLAY == 8);
static_assert(RSF_NATIVE_SCENE_PRESENT_NONE == 0 && RSF_SR_DLSS == 1 && RSF_DLSS_OK == 0 && RSF_DLSS_PIPELINE_OK == 0);
static_assert(RSF_FG_BACKEND_DLSS == 1 && RSF_FG_BACKEND_FSR3 == 3 && RSF_FG_BACKEND_FSR4 == 4 &&
    RSF_FG_BACKEND_XESS == 5 && RSF_FG_BACKEND_AUTO == 7 && RSF_REFLEX_OFF == 0 && RSF_REFLEX_ON == 1 &&
    RSF_REFLEX_BOOST == 2);

rsf_overlay_api overlay{sizeof(rsf_overlay_api), RSF_OVERLAY_API_VERSION};

int32_t same_object(void* left, void* right) noexcept
{
    if (!left || !right) return 0;
    ComPtr<IUnknown> a, b;
    return SUCCEEDED(static_cast<IUnknown*>(left)->QueryInterface(IID_PPV_ARGS(&a))) &&
        SUCCEEDED(static_cast<IUnknown*>(right)->QueryInterface(IID_PPV_ARGS(&b))) && a.Get() == b.Get();
}

void* child_device(void* child) noexcept
{
    return child ? rsf_d3d11_output_child_device(child) : nullptr;
}

void* immediate_context(void* device) noexcept
{
    if (!device) return nullptr;
    ID3D11DeviceContext* context{};
    static_cast<ID3D11Device*>(device)->GetImmediateContext(&context);
    return context;
}

int32_t device_vendor(void* device, uint32_t* vendor) noexcept
{
    ComPtr<IDXGIDevice> dxgi; ComPtr<IDXGIAdapter> adapter; DXGI_ADAPTER_DESC desc{};
    if (FAILED(static_cast<ID3D11Device*>(device)->QueryInterface(IID_PPV_ARGS(&dxgi))) ||
        FAILED(dxgi->GetAdapter(&adapter)) || FAILED(adapter->GetDesc(&desc))) return 0;
    *vendor = desc.VendorId;
    return 1;
}

void* chain_device(void* chain) noexcept
{
    ID3D11Device* device{};
    return SUCCEEDED(static_cast<IDXGISwapChain*>(chain)->GetDevice(IID_PPV_ARGS(&device))) ? device : nullptr;
}

void* chain_window(void* chain) noexcept
{
    DXGI_SWAP_CHAIN_DESC desc{};
    return chain && SUCCEEDED(static_cast<IDXGISwapChain*>(chain)->GetDesc(&desc)) ? desc.OutputWindow : nullptr;
}

void* chain_buffer(void* chain) noexcept
{
    ID3D11Texture2D* buffer{};
    return chain && SUCCEEDED(static_cast<IDXGISwapChain*>(chain)->GetBuffer(0, IID_PPV_ARGS(&buffer))) ? buffer : nullptr;
}

int present_install(const rsf_d3d11_present_setup* setup) noexcept try
{
    return rsf_d3d11_present_install(setup);
} catch (...) { return 0; }

int present_prepare(const rsf_d3d11_present_setup* setup) noexcept try
{
    return rsf_d3d11_present_prepare(setup);
} catch (...) { return 0; }

int present_install_scoped(const rsf_d3d11_present_setup* setup) noexcept try
{
    return rsf_d3d11_present_install_scoped(setup);
} catch (...) { return 0; }

void fg_latency_event(void*, const rsf_observer_present_event* event) noexcept
{
    rsf_native_fg_present(event);
}

int32_t observer_install(rsf_game_log_fn log, void* user, rsf_d3d11h_present_fn present) noexcept try
{
    rsf_observer_options observe{}; observe.struct_size = sizeof(observe); observe.abi_version = RSF_OBSERVER_ABI_VERSION;
    observe.format = UINT32_MAX; observe.minimum_width = UINT32_MAX; observe.capacity = 1;
    observe.log = log; observe.log_user = user; observe.on_present_event = present; observe.on_present_event_user = user;
    return rsf_observer_install(&observe) == RSF_OBSERVER_OK;
} catch (...) { return 0; }

int32_t present_is_owner(void* chain) noexcept { return rsf_d3d11_present_is_owner(chain); }
void chain_dispatch(rsf_d3d11_chain_dispatch* request) noexcept { rsf_d3d11_chain_dispatch_owned(request); }
int32_t creation_begin(void* window, uint64_t* token) noexcept { return rsf_d3d11_present_creation_begin(window, token); }
int32_t creation_end(void* window, uint64_t token, void* device, void* context, void* outer) noexcept
{
    return rsf_d3d11_present_creation_end(window, token, device, context, outer);
}

// The bridge catches its own exceptions. A throw escaping it here reports
// E_FAIL, which the Rust owner refuses with NOT_READY as the former wrapper's
// catch did, before the caller slot or reference is touched.
int32_t wrap_native(void* device, void* native_chain, void** facade) noexcept try
{
    return rsf_d3d11_present_wrap_native_chain(device, native_chain, facade);
} catch (...) { return E_FAIL; }

void* com_identity(void* object) noexcept
{
    IUnknown* identity{};
    return SUCCEEDED(static_cast<IUnknown*>(object)->QueryInterface(IID_PPV_ARGS(&identity))) ? identity : nullptr;
}

// IDXGISwapChain::Present is vtable slot 8: IUnknown 0-2, IDXGIObject 3-6,
// IDXGIDeviceSubObject 7. The address is only logged.
void* chain_present(void* chain) noexcept { return (*static_cast<void***>(chain))[8]; }

uint32_t com_release(void* object) noexcept { return static_cast<IUnknown*>(object)->Release(); }

void fg_frame(const rsf_game_render_pass* pass, uint32_t begin) noexcept { rsf_native_fg_frame(pass, begin); }
void fg_submission(const rsf_game_render_pass* pass, uint32_t begin) noexcept { rsf_native_fg_submission(pass, begin); }
void fg_final(void* context, const rsf_game_render_pass* pass) noexcept { rsf_native_fg_final(context, pass); }
int32_t fg_capture(void* context, const rsf_game_render_pass* pass, const rsf_motion_decode_params* motion) noexcept
{
    return rsf_native_sr_capture_fg(context, pass, motion);
}
int32_t scene_pass(const rsf_game_render_pass* pass, uint32_t begin) noexcept { return rsf_native_scene_pass(pass, begin); }
uint32_t scene_present_path(const rsf_game_render_pass* window) noexcept { return rsf_native_scene_present_path(window); }
int32_t window_scope(const rsf_game_render_pass* pass, uint32_t begin) noexcept { return rsf_native_window_scope(pass, begin); }
int32_t window_present(const rsf_observer_present_event* event) noexcept { return rsf_native_window_present(event); }
int32_t window_last_present(rsf_native_present_record* record) noexcept { return rsf_native_window_last_present(record); }
int32_t window_match(void* chain, rsf_game_render_pass* pass) noexcept { return rsf_native_window_match(chain, pass); }
void latency_window_bind(uint64_t session_id, void* window) noexcept
{
    const rsf_native_latency_window latency_window{sizeof(latency_window), 1, session_id,
        uint64_t(reinterpret_cast<uintptr_t>(window))};
    (void)rsf_native_latency_window_bind(&latency_window);
}
int32_t cpu_dispatch_fg(const rsf_game_cpu_event* event) noexcept
{
    return rsf_native_cpu_dispatch(event, [](void*, const rsf_game_cpu_event* fact) { rsf_native_fg_cpu(fact); }, nullptr);
}
void cpu_event(const rsf_game_cpu_event* event) noexcept { (void)rsf_native_cpu_event(event); }

int32_t dlss_native_present(void* chain, uint32_t sync_interval, uint32_t flags, uint32_t begin) noexcept
{
    return rsf_dlss_native_present(chain, sync_interval, flags, begin);
}
int32_t pipeline_start(void* device, const rsf_d3d11h_pipeline_start* request) noexcept try
{
    const auto& setup = *request->setup; const auto& text = *request->strings;
    rsf_dlss_pipeline_setup create{}; create.struct_size = sizeof(create); create.abi_version = RSF_DLSS_PIPELINE_ABI_VERSION;
    create.streamline_directory_utf8 = text.streamline; create.require_signature = 1;
    create.output_width = request->output_width; create.output_height = request->output_height;
    create.quality = request->quality; create.motion = setup.motion; create.log = request->log; create.log_user = request->user;
    create.engine = setup.engine_type; create.engine_version_utf8 = text.engine_version; create.project_id_utf8 = text.project;
    create.fsr2_directory_utf8 = create.fsr3_directory_utf8 = create.fsr4_directory_utf8 = text.fidelityfx;
    create.xess_directory_utf8 = text.xess; create.view_space_to_meters = setup.units_to_meters;
    create.standard_depth = !setup.depth_inverted;
    return rsf_dlss_pipeline_start_backend(device, &create, request->backend);
} catch (...) { return RSF_ERROR_NOT_READY; }
int32_t pipeline_resize(uint32_t width, uint32_t height) noexcept try
{
    return rsf_dlss_pipeline_resize_output(width, height, nullptr, nullptr);
} catch (...) { return RSF_ERROR_NOT_READY; }
int32_t pipeline_select(uint32_t backend) noexcept try
{
    return rsf_dlss_pipeline_select_backend(backend, nullptr, nullptr);
} catch (...) { return RSF_ERROR_NOT_READY; }
int32_t pipeline_quality(uint32_t quality) noexcept try
{
    return rsf_dlss_pipeline_set_quality(quality, nullptr, nullptr);
} catch (...) { return RSF_ERROR_NOT_READY; }
int32_t pipeline_preset(uint32_t preset) noexcept try
{
    return rsf_dlss_pipeline_set_preset(preset);
} catch (...) { return RSF_ERROR_NOT_READY; }
void pipeline_sharpen(const rsf_render_settings* settings) noexcept try
{
    const auto sharpen = rsf::render_settings::sharpening(*settings);
    (void)rsf_dlss_pipeline_set_sharpening(&sharpen);
} catch (...) {}
int32_t pipeline_status(rsf_d3d11h_pipeline_status* output) noexcept try
{
    rsf_dlss_pipeline_status status{}; status.struct_size = sizeof(status);
    const auto result = rsf_dlss_pipeline_get_status(&status);
    *output = {status.running, status.output_width, status.output_height, status.render_width, status.render_height};
    return result;
} catch (...) { return RSF_ERROR_NOT_READY; }
void pipeline_colour(uint32_t transport, uint32_t correction) noexcept try
{
    rsf_dlss_pipeline_set_colour_transport(transport);
    rsf_dlss_pipeline_set_colour_correction(correction);
} catch (...) {}
void* pipeline_output() noexcept { return rsf_dlss_pipeline_output_texture(); }
int32_t sr_evaluate(void* context, const rsf_game_render_pass* pass, uint32_t* ordered) noexcept try
{
    return rsf_native_sr_evaluate_ordered(context, pass, ordered);
} catch (...) {
    if (ordered) *ordered = 0;
    return RSF_ERROR_NOT_READY;
}

void dump_configure(const rsf_d3d11h_strings* text, rsf_game_log_fn log, void* user) noexcept try
{
    rsf_input_dump_configure(text->captures, text->identity, text->fingerprint, log, user);
} catch (...) {}
void dump_request() noexcept try { rsf_input_dump_request(); } catch (...) {}
int32_t dump_requested() noexcept try { return rsf_input_dump_requested(); } catch (...) { return 0; }
void dump_poll() noexcept try { rsf_input_dump_poll(); } catch (...) {}
int32_t dump_begin(const rsf_frame_record* record, uint32_t backend) noexcept try
{
    return rsf_input_dump_begin_record(record, backend);
} catch (...) { return 0; }
int32_t dump_wants(const rsf_frame_record* record) noexcept try { return rsf_input_dump_wants(record); } catch (...) { return 0; }
void dump_inputs(void* context, const rsf_game_render_pass* pass, const rsf_frame_record* record, void* original) noexcept try
{
    rsf_input_dump_pass_metadata(pass);
    rsf_input_dump_pass(context, pass, 1);
    (void)rsf_input_dump_texture_record(context, record, "native_color", "Native pre-tonemap scene colour",
        original ? original : pass->color_input, RSF_DUMP_VIEW_HDR, 1);
    if (pass->temporal_mask)
        (void)rsf_input_dump_texture_record(context, record, "native_temporal_mask",
            "Native numeric temporal rejection mask before weighted normalization", pass->temporal_mask, RSF_DUMP_VIEW_RAW, 1);
    (void)rsf_input_dump_texture_record(context, record, "native_depth", "Native scene device depth", pass->depth, RSF_DUMP_VIEW_DEPTH, 1);
    (void)rsf_input_dump_texture_record(context, record, "native_motion", "Native motion in adapter-declared encoding",
        pass->motion, RSF_DUMP_VIEW_DENSE_MOTION, 8);
} catch (...) {}
void dump_pass(void* context, const rsf_game_render_pass* pass, uint32_t begin) noexcept try
{
    rsf_input_dump_pass(context, pass, begin);
} catch (...) {}
int32_t dump_texture(void* context, const rsf_frame_record* record, const char* name, const char* description,
    void* texture, uint32_t view, float scale) noexcept try
{
    return rsf_input_dump_texture_record(context, record, name, description, texture, view, scale);
} catch (...) { return 0; }
void dump_result(const rsf_frame_record* record, const char* stage, uint32_t backend, int32_t result) noexcept try
{
    rsf_input_dump_result(record, stage, backend, result);
} catch (...) {}
void dump_complete(const rsf_frame_record* record, int32_t result) noexcept try
{
    rsf_input_dump_complete(record, result);
} catch (...) {}

int32_t overlay_configure(const rsf_overlay_service_setup* panel, rsf_render_settings* applied) noexcept try
{
    if (rsf_get_overlay_api(&overlay) != RSF_OK || overlay.configure(panel) != RSF_OK) return RSF_ERROR_NOT_READY;
    *applied = {sizeof(rsf_render_settings), RSF_RENDER_FRONTEND_ABI_VERSION};
    return overlay.settings_get(applied);
} catch (...) { return RSF_ERROR_NOT_READY; }
void overlay_present(void* chain) noexcept try
{
    if (overlay.present) (void)overlay.present(chain, nullptr);
} catch (...) {}
void overlay_poll_toggle() noexcept try { rsf_overlay_input_poll_toggle(); } catch (...) {}

int32_t ini_read(const wchar_t* path, const wchar_t* section, const wchar_t* key, int32_t fallback) noexcept
{
    return int32_t(GetPrivateProfileIntW(section, key, fallback, path));
}
int32_t ini_write(const wchar_t* path, const wchar_t* section, const wchar_t* key, const wchar_t* value) noexcept
{
    return WritePrivateProfileStringW(section, key, value, path) != 0;
}

const rsf_d3d11h_platform& platform()
{
    static const rsf_d3d11h_platform table{
        .same_object = same_object,
        .child_device = child_device,
        .is_immediate = rsf_d3d11_output_is_immediate,
        .device_ok = rsf_d3d11_output_device_ok,
        .immediate_context = immediate_context,
        .device_vendor = device_vendor,
        .chain_device = chain_device,
        .chain_window = chain_window,
        .chain_buffer = chain_buffer,
        .present_install = present_install,
        .present_prepare = present_prepare,
        .present_install_scoped = present_install_scoped,
        .fg_prepare = rsf_native_fg_prepare,
        .fg_retire = rsf_native_fg_retire,
        .fg_latency_event = fg_latency_event,
        .fg_rendered_source_set = rsf_native_fg_rendered_source_set,
        .fg_set_log = rsf_native_fg_set_log,
        .fg_options_set = rsf_native_fg_options_set,
        .observer_install = observer_install,
        .present_is_owner = present_is_owner,
        .chain_dispatch = chain_dispatch,
        .creation_begin = creation_begin,
        .creation_end = creation_end,
        .wrap_native = wrap_native,
        .com_identity = com_identity,
        .chain_present = chain_present,
        .com_release = com_release,
        .fg_frame = fg_frame,
        .fg_submission = fg_submission,
        .fg_final = fg_final,
        .fg_capture = fg_capture,
        .scene_pass = scene_pass,
        .scene_present_path = scene_present_path,
        .window_scope = window_scope,
        .window_present = window_present,
        .window_last_present = window_last_present,
        .window_match = window_match,
        .latency_window_bind = latency_window_bind,
        .cpu_dispatch_fg = cpu_dispatch_fg,
        .cpu_event = cpu_event,
        .dlss_native_present = dlss_native_present,
        .pipeline_start = pipeline_start,
        .pipeline_resize = pipeline_resize,
        .pipeline_select = pipeline_select,
        .pipeline_quality = pipeline_quality,
        .pipeline_preset = pipeline_preset,
        .pipeline_sharpen = pipeline_sharpen,
        .pipeline_status = pipeline_status,
        .pipeline_colour = pipeline_colour,
        .pipeline_output = pipeline_output,
        .sr_evaluate = sr_evaluate,
        .dump_configure = dump_configure,
        .dump_request = dump_request,
        .dump_requested = dump_requested,
        .dump_poll = dump_poll,
        .dump_begin = dump_begin,
        .dump_wants = dump_wants,
        .dump_inputs = dump_inputs,
        .dump_pass = dump_pass,
        .dump_texture = dump_texture,
        .dump_result = dump_result,
        .dump_complete = dump_complete,
        .overlay_configure = overlay_configure,
        .overlay_present = overlay_present,
        .overlay_poll_toggle = overlay_poll_toggle,
        .ini_read = ini_read,
        .ini_write = ini_write,
        .auxiliary = rsf_auxiliary_sr_platform(),
    };
    return table;
}
}

extern "C" RSF_RUNTIME_API int32_t rsf_d3d11_host_create(const rsf_d3d11_host_setup* setup, rsf_game_host_services* services)
{
    return rsf_core_d3d11_host_create(&platform(), setup, services);
}
