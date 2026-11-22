// SPDX-License-Identifier: MIT
// Stable D3D11 host create export and foreign callback guards. The Rust owner
// handles admission, leases, lifecycle and recovery.
#include "d3d11_host_platform.h"
#include "d3d11_output_platform.h"
#include <rescaleframe/native_sr.h>
#include <rescaleframe/native_scene.h>
#include <rescaleframe/native_fg.h>
#include <rescaleframe/d3d11_present_bridge.h>
#include <rescaleframe/dlss_pipeline.h>
#include <rescaleframe/input_dump.h>
#include <rescaleframe/overlay_input.h>
#include <rescaleframe/frame_generation.h>
#include "render_settings_store.h"
#include "native_sr_completion.h"
#include <windows.h>
#include <dxgi.h>
#include <cstddef>

extern "C" {
const rsf_aux_platform* rsf_auxiliary_sr_platform(void);
int32_t rsf_core_d3d11_host_create(const rsf_d3d11h_platform*, const rsf_d3d11_host_setup*, rsf_game_host_services*);
}

namespace {
// Public wire layouts also checked by the generated Rust SDK bindings.
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
// Private table layouts mirrored in d3d11_host_abi.rs.
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

const rsf_d3d11h_platform& platform()
{
    static const rsf_d3d11h_platform table{
        .same_object = rsf_d3d11h_same_object,
        .child_device = rsf_d3d11_output_child_device,
        .is_immediate = rsf_d3d11_output_is_immediate,
        .device_ok = rsf_d3d11_output_device_ok,
        .immediate_context = rsf_d3d11h_immediate_context,
        .device_vendor = rsf_d3d11h_device_vendor,
        .chain_device = rsf_d3d11h_chain_device,
        .chain_window = rsf_d3d11h_chain_window,
        .chain_buffer = rsf_d3d11h_chain_buffer,
        .present_install = present_install,
        .present_prepare = present_prepare,
        .present_install_scoped = present_install_scoped,
        .fg_prepare = rsf_native_fg_prepare,
        .fg_retire = rsf_native_fg_retire,
        .fg_latency_event = rsf_d3d11h_fg_latency_event,
        .fg_rendered_source_set = rsf_native_fg_rendered_source_set,
        .fg_set_log = rsf_native_fg_set_log,
        .fg_options_set = rsf_native_fg_options_set,
        .observer_install = observer_install,
        .present_is_owner = present_is_owner,
        .chain_dispatch = chain_dispatch,
        .creation_begin = creation_begin,
        .creation_end = creation_end,
        .wrap_native = wrap_native,
        .com_identity = rsf_d3d11h_com_identity,
        .chain_present = rsf_d3d11h_chain_present,
        .com_release = rsf_d3d11h_com_release,
        .fg_frame = rsf_native_fg_frame,
        .fg_submission = rsf_native_fg_submission,
        .fg_final = rsf_native_fg_final,
        .fg_capture = rsf_native_sr_capture_fg,
        .scene_pass = rsf_native_scene_pass,
        .scene_present_path = rsf_native_scene_present_path,
        .window_scope = rsf_native_window_scope,
        .window_present = rsf_native_window_present,
        .window_last_present = rsf_native_window_last_present,
        .window_match = rsf_native_window_match,
        .latency_window_bind = rsf_d3d11h_latency_window_bind,
        .cpu_dispatch_fg = rsf_d3d11h_cpu_dispatch_fg,
        .cpu_event = rsf_d3d11h_cpu_event,
        .dlss_native_present = rsf_d3d11h_native_present,
        .pipeline_start = rsf_d3d11h_start_pipeline,
        .pipeline_resize = rsf_d3d11h_pipeline_resize,
        .pipeline_select = rsf_d3d11h_pipeline_select,
        .pipeline_quality = rsf_d3d11h_pipeline_quality,
        .pipeline_preset = rsf_d3d11h_pipeline_preset,
        .pipeline_sharpen = rsf_core_dlss_pipeline_apply_sharpening,
        .pipeline_status = rsf_d3d11h_read_pipeline_status,
        .pipeline_colour = rsf_d3d11h_pipeline_colour,
        .pipeline_output = rsf_dlss_pipeline_output_texture,
        .sr_evaluate = rsf_native_sr_evaluate_ordered_guarded,
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
        .overlay_poll_toggle = rsf_overlay_input_poll_toggle,
        .ini_read = rsf_d3d11h_ini_read,
        .ini_write = rsf_d3d11h_ini_write,
        .auxiliary = rsf_auxiliary_sr_platform(),
    };
    return table;
}
}

extern "C" RSF_RUNTIME_API int32_t rsf_d3d11_host_create(const rsf_d3d11_host_setup* setup, rsf_game_host_services* services)
{
    return rsf_core_d3d11_host_create(&platform(), setup, services);
}
