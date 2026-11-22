/* SPDX-License-Identifier: MIT */
/* Research carrier for standalone AC7. The runtime loads and prepares the game DLL before
   graphics activation. The game DLL supplies graph inputs, frame/view identity and output pools;
   the runtime evaluates SR on the queued RHI stream. A refused native activation runs without SR. */

#include <windows.h>
#include <dxgi.h>

#include <rescaleframe/ac7_carrier.h>
#include <rescaleframe/ac7_motion_capture.h>
#include <rescaleframe/ac7_velocity_pass.h>
#include <rescaleframe/d3d11_observer.h>
#include <rescaleframe/dlss_pipeline.h>
#include <rescaleframe/sr_session.h>
#include <rescaleframe/frame_tap.h>
#include <rescaleframe/native_fg.h>
#include <rescaleframe/input_dump.h>
#include <rescaleframe/fg_choice.h>
#include <rescaleframe/overlay_fg_stats.h>
#include "../../../runtime/orchestrator/src/sharpening_overlay.h"
#include <rescaleframe/overlay_api.h>
#include <rescaleframe/resource_ref.h>
#include <rescaleframe/texture_dump.h>
#include <rescaleframe/plugin_session.h>
#include <rescaleframe/native_sr.h>
#include <rescaleframe/native_translucency.h>
#include <rescaleframe/native_composition.h>
#include <rescaleframe/native_cpu.h>
#include <rescaleframe/native_window.h>

#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include "dlss_bridge.h"
#include "overlay_host.h"

/* Unreal's velocity encoding, from Common.ush: In * (0.499 * 0.5) + 32767/65535. Written here as
   the decode a backend needs, which is the reciprocal of that scale and the same bias. The sentinel
   is far outside any real screen space motion and still exact in half precision. */
#define RSF_UNREAL_MOTION_SCALE (1.0f / (0.499f * 0.5f))
#define RSF_UNREAL_MOTION_BIAS (32767.0f / 65535.0f)
#define RSF_MOTION_SENTINEL (-1000.0f)
static rsf_plugin_session* game_session;
static int game_prepared;
static uint32_t performance_hud_enabled = 1;
/* Panel layout word (rsf_overlay_stats::ui_preferences). Session only, like the FPS overlay
   switch above: this carrier has no settings file of its own for either yet. */
static uint32_t overlay_ui_preferences;
static SRWLOCK game_prepare_guard = SRWLOCK_INIT;
static volatile LONG shutdown_requested;

static volatile LONG native_owner;
int rsf_bridge_native_owned(void) { return InterlockedCompareExchange(&native_owner, 0, 0) != 0; }
static void on_native_pass(void* user, void* list, const rsf_game_render_pass* pass, uint32_t begin);
static void route_dump(void* context, void* texture, const char* name);
static void on_native_scope(void* user, void* list, const rsf_game_render_pass* pass, uint32_t begin)
{ (void)user; (void)list; rsf_ac7_motion_capture_native_pass(pass, begin); rsf_ac7_velocity_scope(pass); }
static void on_native_cpu(void* user, const rsf_game_cpu_event* event)
{ (void)user; (void)rsf_native_fg_dispatch_cpu(event); }

int rsf_bridge_prepare_game(const wchar_t* plugin_path, const rsf_game_probe* probe,
                            rsf_bridge_log_fn log, void* log_user)
{
    rsf_plugin_session_options options;
    rsf_result result;
    AcquireSRWLockExclusive(&game_prepare_guard);
    if (game_session) {
        const int ready = game_prepared;
        ReleaseSRWLockExclusive(&game_prepare_guard); return ready;
    }
    memset(&options, 0, sizeof(options)); options.struct_size = sizeof(options);
    options.abi_version = RSF_PLUGIN_SESSION_ABI_VERSION; options.plugin_path = plugin_path; options.probe = probe;
    options.services.struct_size = sizeof(options.services); options.services.abi_version = RSF_GAME_ABI_VERSION;
    options.services.session_id = ((uint64_t)GetCurrentProcessId() << 32) | GetTickCount();
    options.services.render_pass = on_native_pass;
    options.services.cpu_event = on_native_cpu;
    options.services.scope_state = on_native_scope;
    options.services.render_config = rsf_native_sr_render_config; options.services.log = log; options.services.user = log_user;
    result = rsf_plugin_session_prepare(&options, &game_session);
    game_prepared = result == RSF_OK;
    rsf_ac7_motion_capture_native_owner(game_prepared);
    ReleaseSRWLockExclusive(&game_prepare_guard);
    return result == RSF_OK;
}

static struct {
    int started;
    void* device;
    void* context;
    rsf_bridge_log_fn log;
    void* log_user;

    /* Counters, so a run that produced nothing can say which step it stopped at rather than just
       failing quietly. */
    unsigned long passes;
    unsigned long evaluated;
    unsigned long refused;
    long last_result;

    /* What the last report said, so a report on a timer stays quiet while nothing moves. */
    unsigned long reported_passes;
    unsigned long reported_evaluated;

    uint64_t application_presented_frames;
    uint32_t native_input_mask;
    int has_jitter;
    float jitter_pixels[2];
    int enabled_requested;
    const char* setting_error;

    /* A route dump: the pictures of the native passes for a few consecutive frames, written from
       the render thread as the frame passes them, with DLSS running. RenderDoc cannot do this here:
       Streamline crashes inside RenderDoc's device wrapper the moment it is handed the device. */
    char route_dump_prefix[520];
    unsigned long route_dump_frames;
    unsigned long route_dump_frame;
    unsigned long route_dump_serial;

    /* The presented size the backend outputs at. */
    unsigned long output_width;
    unsigned long output_height;

    /* The DLSS quality level in effect, for the panel, and whether the panel chose one before the
       backend started. */
    unsigned long quality;
    unsigned long dlss_preset;
    rsf_sharpening_options sharpening;
    int quality_chosen;

    /* What the overlay can ask the carrier for. See dlss_bridge.h. */
    rsf_bridge_actions actions;
} bridge;

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

static void publish_native_settings(void)
{ rsf_native_sr_set_enabled(bridge.started && bridge.enabled_requested); }

static void on_native_pass(void* user, void* list, const rsf_game_render_pass* pass, uint32_t begin)
{
    (void)user; (void)list;
    if (!pass || pass->struct_size != sizeof(*pass)) return;
    rsf_input_dump_pass(bridge.context, pass, begin);
    if (pass->role == RSF_GAME_RENDER_VELOCITY_FINALIZE && begin && bridge.started)
        rsf_ac7_velocity_commit(bridge.context, pass);
    if (pass->role == RSF_GAME_RENDER_FRAME && !begin) rsf_ac7_velocity_frame_end();
    if (!begin && bridge.route_dump_frames && pass->color_output &&
        (pass->flags & RSF_GAME_RENDER_PRIMARY)) {
        char name[64];
        snprintf(name, sizeof(name), "native_pass_%u_output", pass->role);
        route_dump(bridge.context, pass->color_output, name);
    }
    (void)rsf_native_fg_dispatch_pass(bridge.context, pass, begin);
    if ((pass->role == RSF_GAME_RENDER_TRANSLUCENCY || pass->role == RSF_GAME_RENDER_CLOUD_DEPTH ||
         pass->role == RSF_GAME_RENDER_MATERIALS) && bridge.started)
        rsf_native_translucency_pass(bridge.context, pass, begin);
    if (bridge.started && pass->role == RSF_GAME_RENDER_UI_COMPOSITE) {
        const int captured = rsf_native_composition_capture(bridge.context, pass, begin);
        if (captured && !begin && bridge.route_dump_frames) {
            rsf_native_composition_frame frame = {0}; frame.struct_size = sizeof(frame);
            if (rsf_native_composition_read(&frame) && frame.scope_id == pass->scope_id &&
                frame.session_id == pass->session_id && frame.native_frame == pass->native_frame) {
                route_dump(bridge.context, frame.scene, "native_ui_branch_input");
                route_dump(bridge.context, frame.ui_raster, "native_ui_raster");
                route_dump(bridge.context, frame.composed, "native_composed");
                {
                    rsf_native_cpu_frame cpu = {0}; cpu.struct_size = sizeof(cpu);
                    const int found = rsf_native_cpu_read(frame.session_id, frame.source_frame_id, &cpu);
                    char path[600];
                    snprintf(path, sizeof(path), "%s_f%lu_s%lu_native_identity.json", bridge.route_dump_prefix,
                        bridge.route_dump_frame, bridge.route_dump_serial++);
                    FILE* stream = fopen(path, "wb");
                    if (stream) {
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
            }
        }
    }
    if (!begin || pass->role != RSF_GAME_RENDER_SR || !bridge.started) return;
    ++bridge.passes;
    bridge.native_input_mask = (pass->color_input ? 1u : 0u) | (pass->depth ? 2u : 0u) |
        (pass->motion ? 4u : 0u) | (pass->exposure ? 8u : 0u);
    bridge.has_jitter = pass->camera_valid;
    memcpy(bridge.jitter_pixels, pass->camera.jitter_pixels, sizeof(bridge.jitter_pixels));
    bridge.last_result = rsf_native_sr_evaluate(bridge.context, pass);
    rsf_input_dump_sr_result(bridge.last_result);
    if (bridge.route_dump_frames) {
        route_dump(bridge.context, pass->color_input, "native_sr_input");
        route_dump(bridge.context, rsf_dlss_pipeline_output_texture(), "native_sr_backend_output");
        route_dump(bridge.context, pass->color_output, "native_sr_graph_output");
    }
    if (bridge.last_result == RSF_DLSS_PIPELINE_OK) ++bridge.evaluated;
    else ++bridge.refused;
    if (bridge.evaluated + bridge.refused <= 6) {
        say("native SR: frame %llu scope %llu, %ux%u -> %ux%u, result %ld; native spatial fallback queued",
            (unsigned long long)pass->native_frame, (unsigned long long)pass->scope_id,
            pass->camera.render_width, pass->camera.render_height, pass->camera.output_width,
            pass->camera.output_height, bridge.last_result);
    }
}

static char sdk_directories[4][1024];
void rsf_bridge_set_sdk_directories(const char* fsr2, const char* fsr3, const char* fsr4, const char* xess)
{
    const char* paths[4] = {fsr2, fsr3, fsr4, xess};
    unsigned int i;
    for (i = 0; i < 4; ++i) snprintf(sdk_directories[i], sizeof(sdk_directories[i]), "%s", paths[i] ? paths[i] : "");
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

/* Whether RSF_MOTION_CAPTURE is on, told by the proxy that reads the setting. The constant watch
   only feeds the motion capture, so it is armed only when a capture was asked for. */
static volatile LONG motion_capture_on;
void rsf_bridge_set_motion_capture(int on) { InterlockedExchange(&motion_capture_on, on != 0); }

/* Write one texture of the route for the frame being dumped. Through a staging copy, so the
   frame's state is left alone; a stall, which a capture can afford. */
static void route_dump(void* context, void* texture, const char* name)
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
    rsf_dump_texture_result raw_result = rsf_dump_texture_bytes(bridge.device, context, texture, &options);
    rsf_dump_texture_result preview_result = rsf_dump_texture(bridge.device, context, texture, &options, &report);
    if (raw_result == RSF_TEXTURE_OK || preview_result == RSF_TEXTURE_OK) {
        say("route dump: %s, %ux%u format %u", path, report.width, report.height, report.format);
    } else {
        say("route dump: %s could not be written, raw %d preview %d", path, raw_result, preview_result);
    }
}

/* Refresh rate of the display the game is on, for the panel's VRR cap. Read only while the
   panel is open, and at most once a second. */
static uint32_t display_refresh_mhz(void)
{
    static uint32_t cached;
    static ULONGLONG checked;
    const ULONGLONG now = GetTickCount64();
    if (rsf_overlay_host_visible() && (!checked || now - checked >= 1000)) {
        MONITORINFOEXW info;
        DEVMODEW mode;
        const HWND window = GetForegroundWindow();
        ZeroMemory(&info, sizeof(info)); info.cbSize = sizeof(info);
        ZeroMemory(&mode, sizeof(mode)); mode.dmSize = sizeof(mode);
        checked = now ? now : 1;
        if (window && GetMonitorInfoW(MonitorFromWindow(window, MONITOR_DEFAULTTOPRIMARY), (MONITORINFO*)&info) &&
            EnumDisplaySettingsW(info.szDevice, ENUM_CURRENT_SETTINGS, &mode) && mode.dmDisplayFrequency > 1) {
            cached = mode.dmDisplayFrequency * 1000u;
        }
    }
    return cached;
}

/* Everything the panel displays, gathered from where it actually lives, every frame the panel is
   open rather than kept as state. */
static void fill_overlay_stats(rsf_overlay_stats* stats)
{
    rsf_dlss_pipeline_status pipeline;

    memset(stats, 0, sizeof(*stats));
    stats->struct_size = sizeof(*stats);

    memset(&pipeline, 0, sizeof(pipeline));
    pipeline.struct_size = sizeof(pipeline);
    if (rsf_dlss_pipeline_get_status(&pipeline) == RSF_DLSS_PIPELINE_OK) {
        stats->backend_loaded = pipeline.running;
        stats->backend_supported = pipeline.dlss_supported;
        stats->render_width = pipeline.render_width;
        stats->render_height = pipeline.render_height;
        stats->output_width = pipeline.output_width;
        stats->output_height = pipeline.output_height;
        stats->frames_evaluated = (uint32_t)pipeline.frames_evaluated;
        stats->frames_refused = (uint32_t)pipeline.frames_refused;
        stats->last_result = (int32_t)pipeline.last_result;
    }
    /* Every super-resolution backend the pipeline can switch to, bit n for backend ID n. */
    stats->sr_backend_choices = (1u << RSF_SR_DLSS) | (1u << RSF_SR_FSR2) | (1u << RSF_SR_FSR3) |
                                (1u << RSF_SR_FSR4) | (1u << RSF_SR_XESS) | (1u << RSF_SR_SPATIAL);
    stats->backend = pipeline.backend;
    stats->requested_backend = pipeline.requested_backend;
    stats->last_switch_result = pipeline.last_switch_result;
    stats->backend_name = pipeline.backend == 2 ? "FSR2" : pipeline.backend == 3 ? "FSR3" :
        pipeline.backend == 4 ? "FSR4" : pipeline.backend == 5 ? "XeSS" : "DLSS";

    /* Why nothing is happening, in the order the pipeline actually fails. */
    if (!bridge.started) {
        stats->refusal_reason = rsf_ac7_carrier_startup_attempted() ?
            "DLSS could not start; see the log" : "Waiting for the renderer";
    } else if (!stats->backend_supported) {
        stats->refusal_reason = "the driver did not accept DLSS";
    } else if (pipeline.requested_backend == RSF_SR_FSR4 &&
               pipeline.last_switch_result == RSF_BACKEND_ERROR_NOT_SUPPORTED) {
        stats->refusal_reason = "FSR4 unavailable for this GPU/runtime; previous backend remains active";
    } else if (bridge.passes == 0) {
        stats->refusal_reason = "no pass has bound the reconstruction inputs yet";
    }

    stats->have_scene_color = (bridge.native_input_mask & 1u) != 0;
    stats->have_depth = (bridge.native_input_mask & 2u) != 0;
    stats->have_motion = (bridge.native_input_mask & 4u) != 0;
    stats->have_exposure = (bridge.native_input_mask & 8u) != 0;
    stats->motion_decoded = bridge.evaluated > 0;
    stats->jitter_active = (uint32_t)bridge.has_jitter;
    stats->jitter_pixels[0] = bridge.jitter_pixels[0];
    stats->jitter_pixels[1] = bridge.jitter_pixels[1];
    {
        rsf_native_fg_status fg = {0}; fg.struct_size = sizeof(fg);
        const int have_fg = rsf_native_fg_status_get(&fg);
        rsf_overlay_fill_fg_stats(stats, have_fg ? &fg : NULL);
        if (have_fg) stats->display_refresh_mhz = display_refresh_mhz();
    }
    {
        LARGE_INTEGER now, frequency;
        QueryPerformanceCounter(&now); QueryPerformanceFrequency(&frequency);
        stats->application_presented_frames = bridge.application_presented_frames;
        stats->sample_qpc = (uint64_t)now.QuadPart; stats->qpc_frequency = (uint64_t)frequency.QuadPart;
        stats->show_performance_hud = performance_hud_enabled;
    }
    stats->ui_preferences = overlay_ui_preferences;
    stats->enabled = (uint32_t)bridge.enabled_requested;
    stats->reinsert_on = (uint32_t)bridge.enabled_requested;
    stats->reinsert_available = (uint32_t)bridge.started;
    stats->quality = (rsf_overlay_quality)bridge.quality;
    stats->dlss_preset = (uint32_t)bridge.dlss_preset;
    stats->dlss_preset_available = 1;
    rsf_overlay_fill_sharpening_stats(stats, &bridge.sharpening);
    {
        rsf_game_render_config config = {0}; config.struct_size = sizeof(config);
        stats->render_scale_percent = rsf_native_sr_render_config(NULL, &config) && config.output_width ?
            (uint32_t)(((uint64_t)config.render_width * 100u + config.output_width - 1u) / config.output_width) : 0u;
    }
    stats->captures_written =
        bridge.actions.capture_count ? (uint32_t)bridge.actions.capture_count() : 0u;
    stats->jitter_on = (uint32_t)bridge.enabled_requested;
    stats->jitter_available =
        bridge.actions.jitter_available ? (uint32_t)bridge.actions.jitter_available() : 0u;
    if (bridge.setting_error) {
        stats->refusal_reason = bridge.setting_error;
    } else if (!bridge.enabled_requested) {
        stats->refusal_reason = NULL;
    }
}

int rsf_bridge_select_sharpening(const rsf_sharpening_options* options)
{
    if (rsf_dlss_pipeline_set_sharpening(options) != RSF_DLSS_PIPELINE_OK) return 0;
    bridge.sharpening = *options;
    if (bridge.actions.save_sharpening) bridge.actions.save_sharpening(options);
    return 1;
}

int rsf_bridge_select_dlss_preset(unsigned long preset)
{
    if (rsf_dlss_pipeline_set_preset((uint32_t)preset) != RSF_DLSS_PIPELINE_OK) return 0;
    bridge.dlss_preset = preset;
    if (bridge.actions.save_dlss_preset) bridge.actions.save_dlss_preset(preset);
    return 1;
}

int rsf_bridge_select_quality(unsigned long quality)
{
    uint32_t width = 0, height = 0;
    if (quality > RSF_DLSS_QUALITY_ULTRA_PERFORMANCE) {
        return 0;
    }
    if (bridge.started) {
        if (rsf_dlss_pipeline_resize_output((uint32_t)bridge.output_width,
            (uint32_t)bridge.output_height, NULL, NULL) != RSF_DLSS_PIPELINE_OK) {
            bridge.setting_error = "The backend refused the engine output extent"; return 0;
        }
        if (rsf_dlss_pipeline_set_quality((rsf_dlss_quality)quality, &width, &height) !=
            RSF_DLSS_PIPELINE_OK) {
            bridge.setting_error = "DLSS did not accept this preset";
            return 0;
        }
        say("preset %lu applied: requested %ux%u, output %lux%lu", quality, width, height,
            bridge.output_width, bridge.output_height);
    }
    bridge.quality = quality;
    if (bridge.started) rsf_native_sr_set_surface((uint32_t)bridge.output_width, (uint32_t)bridge.output_height);
    publish_native_settings();
    bridge.quality_chosen = 1;
    bridge.setting_error = NULL;
    if (bridge.actions.save_quality) {
        bridge.actions.save_quality(quality);
    }
    return 1;
}

void rsf_bridge_set_enabled(int enabled)
{
    if (!enabled) {
        bridge.enabled_requested = 0;
        if (bridge.actions.set_jitter) {
            bridge.actions.set_jitter(0);
        }
    } else {
        bridge.enabled_requested = 1;
        rsf_ac7_carrier_set_startup_attempted(0);
        if (bridge.started && !rsf_bridge_select_quality(bridge.quality)) {
            bridge.enabled_requested = 0;
            return;
        }
        if (bridge.started && bridge.actions.set_jitter) {
            bridge.actions.set_jitter(0);
        }
    }
    publish_native_settings();
    bridge.setting_error = NULL;
    if (bridge.actions.save_enabled) {
        bridge.actions.save_enabled(enabled != 0);
    }
}

/* The FG provider the panel asked for, saved once the switch has taken effect. UINT32_MAX is none. */
static uint32_t pending_fg_save = UINT32_MAX;
static uint32_t overlay_sr_change_id, overlay_sr_change_status;

/* Lay the panel out and draw it, and apply what was clicked here, on the render thread, at the
   point in the frame the overlay was drawn from. */
static void overlay_tick(void* swapchain)
{
    rsf_overlay_stats stats;
    rsf_overlay_intent intent;
    if (!bridge.log) {
        return;
    }
    /* The presenting chain selects the overlay's device. The observer's first allocation can
       belong to a helper device, including one created by capture support. */
    if (!rsf_overlay_host_start(swapchain, bridge.log, bridge.log_user)) {
        return;
    }

    fill_overlay_stats(&stats);
    stats.sr_quality_choices = RSF_OVERLAY_SR_QUALITY_STANDARD_MASK;
    stats.sr_change_id = overlay_sr_change_id;
    stats.sr_change_status = overlay_sr_change_status;
    rsf_overlay_host_option host_options[RSF_OVERLAY_SERVICE_OPTION_LIMIT];
    if (bridge.actions.read_host_options) {
        stats.host_option_count = bridge.actions.read_host_options(host_options, RSF_OVERLAY_SERVICE_OPTION_LIMIT);
        stats.host_options = host_options;
    }
    /* The switch runs at a later Present, so the choice is saved once it has taken effect and
       dropped when the switch reported a failure. */
    if (pending_fg_save != UINT32_MAX) {
        if (rsf_d3d11_present_switch_result() != 0) {
            pending_fg_save = UINT32_MAX;
        } else if (rsf_d3d11_present_backend() == pending_fg_save) {
            rsf_fg_choice_save(pending_fg_save);
            pending_fg_save = UINT32_MAX;
        }
    }
    memset(&intent, 0, sizeof(intent));
    intent.struct_size = sizeof(intent);
    if (!rsf_overlay_host_present(swapchain, &stats, &intent)) {
        return;
    }
    if (intent.host_option_changed && bridge.actions.save_host_option) {
        if (!bridge.actions.save_host_option(intent.host_option_index, intent.host_option_value)) {
            bridge.setting_error = "Could not save startup setting";
        } else {
            bridge.setting_error = NULL;
        }
    }
    if (intent.fg_backend_changed) {
        const rsf_backend_result result = rsf_d3d11_present_request(intent.fg_backend);
        pending_fg_save = result == RSF_BACKEND_OK ? intent.fg_backend : UINT32_MAX;
        say("FG provider requested at Present: backend=%u result=%d", intent.fg_backend, result);
    }
    if (intent.fg_changed || intent.reflex_changed || intent.frame_limit_changed) {
        rsf_native_fg_status current = {0}; current.struct_size = sizeof(current);
        if (rsf_native_fg_status_get(&current)) {
            if (intent.fg_changed) { current.requested.mode = intent.fg_mode; current.requested.generated_frames = intent.fg_generated; }
            if (intent.reflex_changed) current.requested.reflex_mode = intent.reflex_mode;
            if (intent.frame_limit_changed) {
                if (bridge.actions.save_frame_limit && !bridge.actions.save_frame_limit(intent.frame_limit_us)) {
                    say("settings: could not save the frame limit; keeping the active limit");
                } else {
                    current.requested.frame_limit_us = intent.frame_limit_us;
                }
            }
            rsf_native_fg_options_set(&current.requested);
            say("FG selection: requested mode=%u generated=%u Reflex=%u frame limit=%u us", current.requested.mode,
                current.requested.generated_frames, current.requested.reflex_mode, current.requested.frame_limit_us);
        }
    }
    if (intent.performance_hud_changed) performance_hud_enabled = intent.performance_hud != 0;
    if (intent.ui_preferences_changed) overlay_ui_preferences = intent.ui_preferences;
    if (intent.start_requested) {
        if (bridge.actions.start_backend) {
            bridge.actions.start_backend();
        } else {
            say("overlay: nothing is registered to start the backend");
        }
    }
    if (intent.scale_requested && intent.scale_percent > 0) {
        say("overlay: nothing is registered to set the render scale");
    }
    if (intent.dump_requested) {
        if (bridge.actions.trigger_dump) {
            bridge.actions.trigger_dump();
        } else {
            say("overlay: nothing is registered to write a dump");
        }
    }
    int sr_applied = 1;
    if (intent.enabled_changed) {
        rsf_bridge_set_enabled(intent.enabled != 0);
        sr_applied = ((bridge.enabled_requested != 0) == (intent.enabled != 0));
    }
    if (intent.jitter_changed) {
        if (bridge.actions.set_jitter) {
            bridge.actions.set_jitter(intent.jitter);
        } else {
            say("overlay: this build has no jitter control registered");
        }
    }
    if (intent.capture_requested) {
        if (bridge.actions.trigger_capture) {
            bridge.actions.trigger_capture();
        } else {
            say("overlay: no capture support was registered, so there is nothing to capture with");
        }
    }
    if (intent.backend_changed && bridge.started) {
        rsf_dlss_pipeline_status previous;
        memset(&previous, 0, sizeof(previous)); previous.struct_size = sizeof(previous);
        rsf_dlss_pipeline_get_status(&previous);
        if (rsf_dlss_pipeline_select_backend(intent.backend, NULL, NULL) != RSF_DLSS_PIPELINE_OK) {
            bridge.setting_error = "Requested backend is unavailable; keeping the active backend";
        } else if (!rsf_bridge_select_quality(bridge.quality)) {
            rsf_dlss_pipeline_select_backend(previous.backend, NULL, NULL);
            bridge.setting_error = "Game render resolution refused; previous backend restored";
        }
    }
    if (intent.sharpening_changed) {
        rsf_sharpening_options options = rsf_overlay_requested_sharpening(&intent);
        rsf_bridge_select_sharpening(&options);
    }
    if (intent.dlss_preset_changed) rsf_bridge_select_dlss_preset(intent.dlss_preset);
    if (intent.quality_changed) {
        if (!rsf_bridge_select_quality((unsigned long)intent.quality)) sr_applied = 0;
    }
    if (intent.sr_change_id && (intent.enabled_changed || intent.quality_changed)) {
        overlay_sr_change_id = intent.sr_change_id;
        overlay_sr_change_status = sr_applied ? RSF_OVERLAY_SR_CHANGE_APPLIED : RSF_OVERLAY_SR_CHANGE_REFUSED;
    }
}

int rsf_bridge_present_extent(uint32_t* width, uint32_t* height)
{
    rsf_observer_status status;
    memset(&status, 0, sizeof(status));
    status.struct_size = sizeof(status);
    if (rsf_observer_get_status(&status) != RSF_OBSERVER_OK || status.present_width == 0) {
        return 0;
    }
    *width = status.present_width;
    *height = status.present_height;
    return 1;
}

void rsf_bridge_request_shutdown(void)
{ InterlockedExchange(&shutdown_requested, 1); }
static int drain_shutdown(void)
{
    if (!InterlockedCompareExchange(&shutdown_requested, 0, 0)) return 0;
    bridge.enabled_requested = 0; rsf_native_sr_set_enabled(0);
    if (game_session) {
        const rsf_result quiet = rsf_plugin_session_quiesce(game_session);
        if (quiet != RSF_OK) return 1;
        const rsf_result stopped = rsf_plugin_session_stop(game_session);
        if (stopped != RSF_OK) return 1;
        game_session = NULL; game_prepared = 0;
    }
    // Queued native callbacks are finished before their backend, input refs or context disappear.
    const rsf_frame_tap_result untapped = rsf_frame_tap_uninstall();
    if (untapped != RSF_FRAME_TAP_OK && untapped != RSF_FRAME_TAP_ERROR_NOT_INSTALLED) {
        return 1;
    }
    rsf_ac7_velocity_release();
    rsf_dlss_pipeline_stop(); bridge.started = 0;
    rsf_input_dump_cancel(0);
    InterlockedExchange(&native_owner, 0); rsf_ac7_motion_capture_native_owner(0);
    rsf_ac7_carrier_set_startup_attempted(1);
    InterlockedExchange(&shutdown_requested, 0);
    say("renderer shutdown completed after native command and resource retirement");
    return 1;
}

/* Called before the game's own Present, from the observer. */
static void on_present(void* user, void* swapchain)
{
    (void)user;
    if (drain_shutdown()) return;
    if (rsf_bridge_native_owned()) {
        rsf_game_render_pass window = {0}; window.struct_size = sizeof(window);
        static unsigned matched = 0;
        if (rsf_native_window_match(swapchain, &window) && matched < 6) {
            ++matched;
            say("native window Present match: source %llu scope %llu, viewport %llx window %llx RHI %llx",
                (unsigned long long)window.source_frame_id, (unsigned long long)window.scope_id,
                (unsigned long long)window.viewport_key, (unsigned long long)window.window_key,
                (unsigned long long)window.rhi_viewport_key);
        }
    }

    {
        char motion_prefix[640];
        if (rsf_ac7_motion_capture_present(swapchain, motion_prefix, sizeof(motion_prefix))) {
            rsf_bridge_request_dump(motion_prefix);
        }
    }
    rsf_ac7_carrier_try_auto_start(bridge.enabled_requested, bridge.started,
        bridge.actions.startup_ready, rsf_bridge_present_extent, bridge.actions.start_backend_at_extent);
    if (bridge.started) {
        uint32_t width = 0, height = 0;
        if (rsf_bridge_present_extent(&width, &height) &&
            (width != bridge.output_width || height != bridge.output_height)) {
            const rsf_dlss_pipeline_result resized = rsf_dlss_pipeline_resize_output(width, height, NULL, NULL);
            if (resized == RSF_DLSS_PIPELINE_OK) {
                bridge.output_width = width; bridge.output_height = height;
                bridge.setting_error = NULL;
                rsf_native_sr_set_surface(width, height); publish_native_settings();
            } else bridge.setting_error = "The backend refused the new output resolution";
        }
    }

    if (bridge.route_dump_frames) {
        /* The frame as presented, last of the set: what the player sees. */
        void* buffer = rsf_swapchain_back_buffer(swapchain);
        if (buffer) {
            route_dump(bridge.context, buffer, "final");
            rsf_resource_release(buffer);
        }
        --bridge.route_dump_frames;
        ++bridge.route_dump_frame;
    }
    if (bridge.started) {
        /* The runtime flushes at Present and rewrites its vtable when it does, which takes the
           tap's hooks with it; this puts them back before the next frame's first draw. */
        rsf_frame_tap_refresh();
        rsf_frame_tap_end_frame();
        publish_native_settings();
    }
    {
        rsf_game_render_pass window = {0}; window.struct_size = sizeof(window);
        const int matched = rsf_native_window_match(swapchain, &window);
        rsf_input_dump_present(swapchain, bridge.context, matched ? &window : NULL);
    }

    /* Last, so the panel is drawn over the finished frame. It takes no part in the
       reconstruction and is never one of its inputs. */
    overlay_tick(swapchain);
}

static void on_present_event(void* user, const rsf_observer_present_event* event)
{
    (void)user;
    rsf_native_window_present(event);
    if (event && event->completed && !(event->flags & DXGI_PRESENT_TEST)) {
        rsf_game_render_pass window = {0}; window.struct_size = sizeof(window);
        if (rsf_native_window_match(event->swapchain, &window)) rsf_input_dump_presented(&window, event->result);
    }
    if (event && event->completed && event->result == S_OK && !(event->flags & DXGI_PRESENT_TEST))
        ++bridge.application_presented_frames;
}
rsf_observer_present_event_fn rsf_bridge_present_event_hook(void)
{ return on_present_event; }

rsf_observer_present_fn rsf_bridge_present_hook(void)
{
    return on_present;
}

void rsf_bridge_set_log(rsf_bridge_log_fn log, void* log_user)
{
    bridge.log = log;
    bridge.log_user = log_user;
    rsf_ac7_velocity_set_log(log, log_user);
}

rsf_observer_layout_fn rsf_bridge_layout_hook(void) { return on_layout_created; }
rsf_observer_shader_fn rsf_bridge_shader_hook(void) { return on_shader_created; }

void rsf_bridge_set_actions(const rsf_bridge_actions* actions)
{
    if (actions) {
        bridge.actions = *actions;
    } else {
        memset(&bridge.actions, 0, sizeof(bridge.actions));
    }
}

int rsf_bridge_start(const char* streamline_directory, unsigned long output_width,
                     unsigned long output_height, unsigned long quality, rsf_bridge_log_fn log,
                     void* log_user)
{
    rsf_dlss_pipeline_setup setup;
    rsf_frame_tap_options tap;
    rsf_dlss_pipeline_result started;
    rsf_frame_tap_result tapped;

    bridge.log = log;
    bridge.log_user = log_user;
    if (bridge.started) {
        return 1;
    }
    if (!streamline_directory || !streamline_directory[0]) {
        say("dlss bridge: RSF_STREAMLINE_BIN is not set, nothing to load");
        return 0;
    }
    if (!game_session || !game_prepared) {
        say("dlss bridge: the native AC7 renderer is not prepared, so SR stays off");
        return 0;
    }

    /* The game's own device, not one of ours. A second device could not share resources with the
       game's, which is the entire point. */
    if (rsf_observer_acquire_device(&bridge.device, &bridge.context) != RSF_OBSERVER_OK) {
        say("dlss bridge: no device yet, the game has not created one");
        return 0;
    }

    memset(&setup, 0, sizeof(setup));
    setup.struct_size = sizeof(setup);
    setup.abi_version = RSF_DLSS_PIPELINE_ABI_VERSION;
    setup.streamline_directory_utf8 = streamline_directory;
    setup.output_width = (uint32_t)output_width;
    setup.output_height = (uint32_t)output_height;
    /* A level chosen in the panel before the backend started wins over the settings file. */
    if (bridge.quality_chosen) {
        quality = bridge.quality;
    }
    bridge.quality = quality;
    setup.quality = (rsf_dlss_quality)quality;
    setup.motion.struct_size = sizeof(setup.motion);
    setup.motion.scale_x = RSF_UNREAL_MOTION_SCALE;
    setup.motion.scale_y = RSF_UNREAL_MOTION_SCALE;
    setup.motion.bias_x = RSF_UNREAL_MOTION_BIAS;
    setup.motion.bias_y = RSF_UNREAL_MOTION_BIAS;
    /* Left at one: decoded Unreal motion is already in the range a backend wants. Which sign and
       which axis direction it actually needs is unverified, and this is where a flip goes when a
       rendered result shows one is needed. */
    setup.motion.output_scale_x = 1.0f;
    setup.motion.output_scale_y = 1.0f;
    setup.motion.invalid_value = RSF_MOTION_SENTINEL;
    setup.motion.zero_means_unwritten = 1u;
    setup.log = log;
    setup.log_user = log_user;
    setup.engine = RSF_DLSS_ENGINE_UNREAL;
    setup.engine_version_utf8 = RSF_AC7_ENGINE_VERSION;
    setup.project_id_utf8 = RSF_AC7_PROJECT_ID;
    setup.fsr2_directory_utf8 = sdk_directories[0];
    setup.fsr3_directory_utf8 = sdk_directories[1];
    setup.fsr4_directory_utf8 = sdk_directories[2];
    setup.xess_directory_utf8 = sdk_directories[3];
    setup.view_space_to_meters = 0.01f;

    started = rsf_dlss_pipeline_start(bridge.device, &setup);
    if (started != RSF_DLSS_PIPELINE_OK) {
        say("dlss bridge: pipeline did not start, result %d", (int)started);
        return 0;
    }

    bridge.started = 1;
    if (rsf_plugin_session_start(game_session) != RSF_OK) {
        say("native AC7 controller activation refused; SR stays off");
        rsf_dlss_pipeline_stop(); bridge.started = 0; publish_native_settings();
        return 0;
    }
    InterlockedExchange(&native_owner, 1);
    say("native AC7 renderer owns SR: graph insertion, engine output pools and downstream rectangles");
    bridge.output_width = output_width;
    bridge.output_height = output_height;

    memset(&tap, 0, sizeof(tap));
    tap.struct_size = sizeof(tap);
    tap.abi_version = RSF_FRAME_TAP_ABI_VERSION;
    tap.on_geometry = on_geometry;
    tap.log = log;
    tap.log_user = log_user;
    tapped = rsf_frame_tap_install(bridge.context, &tap);
    if (tapped != RSF_FRAME_TAP_OK) {
        say("dlss bridge: frame tap not installed, result %d", (int)tapped);
        rsf_dlss_pipeline_stop(); bridge.started = 0; publish_native_settings();
        return 0;
    }
    if (InterlockedCompareExchange(&motion_capture_on, 0, 0)) {
        rsf_frame_tap_set_constant_observer(0u, observe_constants, NULL);
    }

    if (!rsf_bridge_select_quality(bridge.quality)) {
        rsf_frame_tap_uninstall();
        rsf_dlss_pipeline_stop();
        bridge.started = 0; publish_native_settings();
        return 0;
    }
    /* The native renderer supplies its own jitter, so the carrier's gate stays shut. */
    if (bridge.actions.set_jitter) {
        bridge.actions.set_jitter(0);
    }
    say("SR running through the native AC7 post-process graph");
    return 1;
}

void rsf_bridge_report(void)
{
    rsf_dlss_pipeline_status status;

    /* Nothing has moved since the last report, so there is nothing to say. This is what makes it
       safe to call on a timer. */
    if (bridge.passes == bridge.reported_passes && bridge.evaluated == bridge.reported_evaluated) {
        return;
    }
    bridge.reported_passes = bridge.passes;
    bridge.reported_evaluated = bridge.evaluated;
    say("bridge: %lu passes, %lu evaluated, %lu refused, last result %ld",
        bridge.passes, bridge.evaluated, bridge.refused, bridge.last_result);

    memset(&status, 0, sizeof(status));
    status.struct_size = sizeof(status);
    if (rsf_dlss_pipeline_get_status(&status) == RSF_DLSS_PIPELINE_OK) {
        say("pipeline: running %u, dlss supported %u, render %ux%u, output %ux%u, evaluated %llu, "
            "refused %llu",
            status.running, status.dlss_supported, status.render_width, status.render_height,
            status.output_width, status.output_height,
            (unsigned long long)status.frames_evaluated,
            (unsigned long long)status.frames_refused);
        /* Whether this is upscaling at all. A backend fed the presented size is doing antialiasing
           instead, which looks clean and sharp precisely because nothing was reconstructed from
           less, so it has to be said rather than seen. */
        if (status.render_width != 0 && status.output_width != 0 &&
            status.render_width >= status.output_width) {
            say("note: the game is rendering at the presented size, so this is antialiasing at "
                "native resolution and not upscaling");
        }
    }
}

void rsf_bridge_request_dump(const char* prefix)
{
    if (bridge.started) {
        /* A disabled-backend capture must not label a later enabled frame with this prefix. */
        if (bridge.enabled_requested) rsf_dlss_pipeline_request_dump(prefix);
        /* And the route's own pictures, for consecutive frames: whether the layer or the
           reconstruction moves between them is what a shimmer complaint needs answered. */
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
