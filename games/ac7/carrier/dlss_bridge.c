/* SPDX-License-Identifier: MIT */
/* AC7 carrier: native game-controller preparation and the input work that stays with the game.
   The game controller reports its passes, CPU events and Present to the core D3D11 host, which
   evaluates SR, generates frames, paces and draws the overlay. A refused native start leaves the
   game rendering as it does without ReScaleFrame. */

#include <windows.h>
#include <dxgi.h>

#include <rescaleframe/ac7_carrier.h>
#include <rescaleframe/ac7_motion_capture.h>
#include <rescaleframe/ac7_velocity_pass.h>
#include <rescaleframe/d3d11_host.h>
#include <rescaleframe/d3d11_observer.h>
#include <rescaleframe/dlss_pipeline.h>
#include <rescaleframe/frame_tap.h>
#include <rescaleframe/input_dump.h>
#include <rescaleframe/resource_ref.h>
#include <rescaleframe/texture_dump.h>
#include <rescaleframe/plugin_session.h>
#include <rescaleframe/native_composition.h>
#include <rescaleframe/native_cpu.h>
#include <rescaleframe/native_window.h>

#include <stdarg.h>
#include <stdio.h>
#include <string.h>

#include "dlss_bridge.h"

static rsf_plugin_session* game_session;
static int game_prepared;
static SRWLOCK game_prepare_guard = SRWLOCK_INIT;
static rsf_game_host_services host;

static struct {
    /* The native renderer runs; `refused` spends its one start attempt. */
    int started;
    int refused;
    void* device;
    void* context;
    rsf_bridge_log_fn log;
    void* log_user;
    void (*set_jitter)(unsigned long open);

    /* A route dump: the pictures of the native passes for a few consecutive frames, written from
       the render thread as the frame passes them. RenderDoc cannot do this here: Streamline
       crashes inside RenderDoc's device wrapper the moment it is handed the device. */
    char route_dump_prefix[520];
    unsigned long route_dump_frames;
    unsigned long route_dump_frame;
    unsigned long route_dump_serial;
} bridge;

/* Whether RSF_MOTION_CAPTURE is on, told by the proxy that reads the setting. The constant watch
   only feeds the motion capture, so it is armed only when a capture was asked for. */
static volatile LONG motion_capture_on;
void rsf_bridge_set_motion_capture(int on) { InterlockedExchange(&motion_capture_on, on != 0); }
void rsf_bridge_set_jitter_control(void (*set_jitter)(unsigned long open)) { bridge.set_jitter = set_jitter; }
void rsf_bridge_set_host(const rsf_game_host_services* services) { host = *services; }

static void say(const char* format, ...)
{
    char message[512];
    va_list arguments;
    if (!bridge.log) {
        return;
    }
    va_start(arguments, format);
    vsnprintf(message, sizeof(message), format, arguments);
    va_end(arguments);
    bridge.log(bridge.log_user, message);
}

/* Write one texture of the route for the frame being dumped. Through a staging copy, so the
   frame's state is left alone; a stall, which a capture can afford. */
static void route_dump(void* texture, const char* name)
{
    rsf_texture_dump_options options;
    rsf_texture_dump_report report;
    char path[600];
    if (!texture || !bridge.route_dump_frames) {
        return;
    }
    snprintf(path, sizeof(path), "%s_f%lu_s%lu_%s", bridge.route_dump_prefix, bridge.route_dump_frame,
             bridge.route_dump_serial++, name);
    memset(&options, 0, sizeof(options));
    options.struct_size = sizeof(options);
    options.abi_version = RSF_TEXTURE_DUMP_ABI_VERSION;
    options.output_prefix_utf8 = path;
    options.view = RSF_DUMP_VIEW_RAW;
    options.scale = 1.0f;
    memset(&report, 0, sizeof(report));
    report.struct_size = sizeof(report);
    rsf_dump_texture_result raw_result = rsf_dump_texture_bytes(bridge.device, bridge.context, texture, &options);
    rsf_dump_texture_result preview_result = rsf_dump_texture(bridge.device, bridge.context, texture, &options, &report);
    if (raw_result == RSF_TEXTURE_OK || preview_result == RSF_TEXTURE_OK) {
        say("route dump: %s, %ux%u format %u", path, report.width, report.height, report.format);
    } else {
        say("route dump: %s could not be written, raw %d preview %d", path, raw_result, preview_result);
    }
}

/* The UI branch the core composed, with the CPU identity it belongs to. */
static void route_dump_composition(const rsf_game_render_pass* pass)
{
    rsf_native_composition_frame frame = {0}; frame.struct_size = sizeof(frame);
    if (!rsf_native_composition_read(&frame) || frame.scope_id != pass->scope_id ||
        frame.session_id != pass->session_id || frame.native_frame != pass->native_frame) return;
    route_dump(frame.scene, "native_ui_branch_input");
    route_dump(frame.ui_raster, "native_ui_raster");
    route_dump(frame.composed, "native_composed");
    {
        rsf_native_cpu_frame cpu = {0}; cpu.struct_size = sizeof(cpu);
        const int found = rsf_native_cpu_read(frame.session_id, frame.source_frame_id, &cpu);
        char path[600];
        FILE* stream;
        snprintf(path, sizeof(path), "%s_f%lu_s%lu_native_identity.json", bridge.route_dump_prefix,
            bridge.route_dump_frame, bridge.route_dump_serial++);
        stream = fopen(path, "wb");
        if (!stream) return;
        fprintf(stream, "{\n  \"session\": %llu,\n  \"source_frame\": %llu,\n  \"submission\": %llu,\n"
            "  \"native_frame\": %llu,\n  \"scope\": %llu,\n  \"viewport_key\": %llu,\n  \"flags\": %u,\n  \"cpu_found\": %s,\n"
            "  \"cpu_stage_mask\": %u,\n  \"cpu_ended\": %u,\n  \"cpu_failed\": %u,\n"
            "  \"qpc_frequency\": %llu,\n  \"timestamps_qpc\": [%llu,%llu,%llu,%llu,%llu]\n}\n",
            (unsigned long long)frame.session_id, (unsigned long long)frame.source_frame_id,
            (unsigned long long)frame.submission_id, (unsigned long long)frame.native_frame,
            (unsigned long long)frame.scope_id, (unsigned long long)frame.viewport_key,
            frame.flags, found ? "true" : "false",
            cpu.stage_mask, cpu.ended, cpu.failed, (unsigned long long)cpu.qpc_frequency,
            (unsigned long long)cpu.timestamps_qpc[0], (unsigned long long)cpu.timestamps_qpc[1],
            (unsigned long long)cpu.timestamps_qpc[2], (unsigned long long)cpu.timestamps_qpc[3],
            (unsigned long long)cpu.timestamps_qpc[4]);
        fclose(stream);
    }
}

static void on_native_scope(void* user, void* list, const rsf_game_render_pass* pass, uint32_t begin)
{ (void)user; (void)list; rsf_ac7_motion_capture_native_pass(pass, begin); rsf_ac7_velocity_scope(pass); }

/* AC7's motion work at its native boundaries, then the pass as the engine reported it. */
static void on_native_pass(void* user, void* list, const rsf_game_render_pass* pass, uint32_t begin)
{
    (void)user; (void)list;
    if (!pass || pass->struct_size != sizeof(*pass)) return;
    rsf_input_dump_pass(bridge.context, pass, begin);
    if (pass->role == RSF_GAME_RENDER_VELOCITY_FINALIZE && begin && bridge.started)
        rsf_ac7_velocity_commit(bridge.context, pass);
    if (pass->role == RSF_GAME_RENDER_FRAME && !begin) rsf_ac7_velocity_frame_end();
    host.render_pass(host.user, bridge.context, pass, begin);
    if (!bridge.route_dump_frames) return;
    if (!begin && pass->color_output && (pass->flags & RSF_GAME_RENDER_PRIMARY)) {
        char name[64];
        snprintf(name, sizeof(name), "native_pass_%u_output", pass->role);
        route_dump(pass->color_output, name);
    }
    if (!begin && pass->role == RSF_GAME_RENDER_UI_COMPOSITE) route_dump_composition(pass);
    if (begin && pass->role == RSF_GAME_RENDER_SR) {
        route_dump(pass->color_input, "native_sr_input");
        route_dump(rsf_dlss_pipeline_output_texture(), "native_sr_backend_output");
        route_dump(pass->color_output, "native_sr_graph_output");
    }
}

int rsf_bridge_prepare_game(const wchar_t* plugin_path, const rsf_game_probe* probe)
{
    rsf_plugin_session_options options;
    rsf_result result;
    AcquireSRWLockExclusive(&game_prepare_guard);
    if (game_session) {
        const int ready = game_prepared;
        ReleaseSRWLockExclusive(&game_prepare_guard); return ready;
    }
    if (!host.render_pass) {
        ReleaseSRWLockExclusive(&game_prepare_guard);
        say("native AC7 renderer not prepared: the core D3D11 host is not running");
        return 0;
    }
    memset(&options, 0, sizeof(options)); options.struct_size = sizeof(options);
    options.abi_version = RSF_PLUGIN_SESSION_ABI_VERSION; options.plugin_path = plugin_path; options.probe = probe;
    /* The host's services, with AC7's own scope and velocity work in front of its passes. */
    options.services = host;
    options.services.render_pass = on_native_pass;
    options.services.scope_state = on_native_scope;
    result = rsf_plugin_session_prepare(&options, &game_session);
    game_prepared = result == RSF_OK;
    rsf_ac7_motion_capture_native_owner(game_prepared);
    ReleaseSRWLockExclusive(&game_prepare_guard);
    return result == RSF_OK;
}

/* AC7 motion capture: the frame tap's geometry reports, and the uploads the capture asked for. */
static void on_geometry(void* user, const rsf_frame_tap_geometry* draw)
{
    (void)user;
    rsf_ac7_motion_capture_geometry(draw);
    rsf_ac7_velocity_geometry(draw);
}

static void observe_constants(void* user, void* buffer, const void* contents, uint32_t bytes)
{
    (void)user;
    rsf_ac7_motion_capture_upload(buffer, contents, bytes);
}

/* Bind the presenting chain to the host, then, once the decrypted image is patched, start the
   native renderer on the game's own device: the frame tap that feeds the velocity pass first,
   then the game controller. One attempt; a refusal keeps the game's own rendering. */
static void start_native(void* swapchain)
{
    rsf_d3d11_presentation_binding binding;
    rsf_frame_tap_options tap;
    rsf_frame_tap_result tapped;
    if (bridge.started || bridge.refused || !host.user) return;
    memset(&binding, 0, sizeof(binding));
    binding.struct_size = sizeof(binding);
    binding.abi_version = RSF_D3D11_HOST_ABI_VERSION;
    binding.swapchain = swapchain;
    if (rsf_d3d11_host_bind_presentation(host.user, &binding) != RSF_OK) return;
    if (!rsf_ac7_carrier_startup_ready()) return;
    if (rsf_observer_acquire_device(&bridge.device, &bridge.context) != RSF_OBSERVER_OK) return;
    bridge.refused = 1;
    if (!game_session || !game_prepared) {
        say("native AC7 renderer is not prepared; the game keeps its own rendering");
        return;
    }
    memset(&tap, 0, sizeof(tap));
    tap.struct_size = sizeof(tap);
    tap.abi_version = RSF_FRAME_TAP_ABI_VERSION;
    tap.on_geometry = on_geometry;
    tapped = rsf_frame_tap_install(bridge.context, &tap);
    if (tapped != RSF_FRAME_TAP_OK) {
        say("native AC7 renderer not started: frame tap refused, result %d", (int)tapped);
        return;
    }
    if (InterlockedCompareExchange(&motion_capture_on, 0, 0)) {
        rsf_frame_tap_set_constant_observer(0u, observe_constants, NULL);
    }
    if (rsf_plugin_session_start(game_session) != RSF_OK) {
        rsf_frame_tap_uninstall();
        say("native AC7 controller activation refused; the game keeps its own rendering");
        return;
    }
    bridge.started = 1;
    /* The native renderer supplies its own jitter, so the carrier's gate stays shut. */
    if (bridge.set_jitter) bridge.set_jitter(0);
    say("native AC7 renderer started: graph insertion, engine output pools and downstream rectangles "
        "report to the core D3D11 host");
}

void rsf_bridge_present(void* user, const rsf_observer_present_event* event)
{
    rsf_game_render_pass window = {0};
    int matched;
    (void)user;
    if (!event || (event->flags & DXGI_PRESENT_TEST)) return;
    window.struct_size = sizeof(window);
    matched = rsf_native_window_match(event->swapchain, &window);
    if (event->completed) {
        if (matched) rsf_input_dump_presented(&window, event->result);
        return;
    }
    {
        char motion_prefix[640];
        if (rsf_ac7_motion_capture_present(event->swapchain, motion_prefix, sizeof(motion_prefix))) {
            rsf_bridge_request_dump(motion_prefix);
        }
    }
    start_native(event->swapchain);
    if (bridge.route_dump_frames) {
        /* The frame as presented, last of the set: what the player sees. */
        void* buffer = rsf_swapchain_back_buffer(event->swapchain);
        if (buffer) {
            route_dump(buffer, "final");
            rsf_resource_release(buffer);
        }
        --bridge.route_dump_frames;
        ++bridge.route_dump_frame;
    }
    if (bridge.started) {
        /* The runtime flushes at Present and rewrites its vtable when it does, which takes the
           tap's hooks with it; this puts them back before the next frame's first draw. */
        rsf_frame_tap_refresh();
    }
    rsf_input_dump_present(event->swapchain, bridge.context, matched ? &window : NULL);
}

static void on_layout_created(void* user, void* layout, const rsf_observer_layout_element* elements,
                              uint32_t copied, uint32_t count)
{
    (void)user;
    rsf_ac7_motion_capture_layout(layout, elements, copied, count);
    rsf_ac7_velocity_layout_seen(layout, elements, copied, count);
}

static void on_shader_created(void* user, void* shader, uint32_t stage, const void* bytecode,
                              uint32_t bytes)
{
    (void)user;
    rsf_ac7_motion_capture_shader(shader, stage, bytecode, bytes);
}

rsf_observer_layout_fn rsf_bridge_layout_hook(void) { return on_layout_created; }
rsf_observer_shader_fn rsf_bridge_shader_hook(void) { return on_shader_created; }

void rsf_bridge_set_log(rsf_bridge_log_fn log, void* log_user)
{
    bridge.log = log;
    bridge.log_user = log_user;
    rsf_ac7_velocity_set_log(log, log_user);
}

void rsf_bridge_request_dump(const char* prefix)
{
    if (bridge.started) {
        /* The reconstruction's own inputs when it runs, then the route's pictures for
           consecutive frames: whether the layer or the reconstruction moves between them is
           what a shimmer complaint needs answered. */
        rsf_dlss_pipeline_request_dump(prefix);
        snprintf(bridge.route_dump_prefix, sizeof(bridge.route_dump_prefix), "%s_route", prefix);
        bridge.route_dump_frame = 0;
        bridge.route_dump_serial = 0;
        bridge.route_dump_frames = 8;
        rsf_native_composition_request(2);
    }
}

int rsf_bridge_running(void)
{
    return bridge.started;
}
