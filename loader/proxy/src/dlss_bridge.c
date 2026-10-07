/* SPDX-License-Identifier: GPL-3.0-only */
/* Research carrier for standalone AC7. The runtime loads and prepares the game DLL before
   graphics activation. With native ownership, the game DLL supplies graph inputs, frame/view
   identity and output pools; the runtime evaluates SR on the queued RHI stream. The D3D binding
   matcher and promotion plans below remain a compatibility fallback for refused native hooks. */

#include <windows.h>
#include <dxgiformat.h>
#include <dxgi.h>

#include <rescaleframe/ac7_view.h>
#include <rescaleframe/ac7_motion_capture.h>
#include <rescaleframe/constant_twin.h>
#include <rescaleframe/ac7_scene_color.h>
#include <rescaleframe/constant_buffer_read.h>
#include <rescaleframe/d3d11_observer.h>
#include <rescaleframe/d3d11_state.h>
#include <rescaleframe/dlss_pipeline.h>
#include <rescaleframe/sr_session.h>
#include <rescaleframe/frame_tap.h>
#include <rescaleframe/depth_replay.h>
#include <rescaleframe/present_blit.h>
#include <rescaleframe/native_fg.h>
#include <rescaleframe/fg_choice.h>
#include <rescaleframe/ac7_ui_rules.h>
#include <rescaleframe/fullscreen_pass.h>
#include <rescaleframe/resource_ref.h>
#include <rescaleframe/scene_promote.h>
#include <rescaleframe/texture_dump.h>
#include <rescaleframe/ui_identify.h>
#include <rescaleframe/ui_layer.h>
#include <rescaleframe/plugin_session.h>
#include <rescaleframe/native_sr.h>
#include <rescaleframe/native_translucency.h>
#include <rescaleframe/native_composition.h>
#include <rescaleframe/native_cpu.h>
#include <rescaleframe/native_window.h>
#include <rescaleframe/native_scene.h>

#include <ctype.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <math.h>

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
static SRWLOCK game_prepare_guard = SRWLOCK_INIT;
static volatile LONG shutdown_requested;

static volatile LONG native_owner;
int rsf_bridge_native_owned(void) { return InterlockedCompareExchange(&native_owner, 0, 0) != 0; }
static void on_native_pass(void* user, void* list, const rsf_game_render_pass* pass, uint32_t begin);
static void route_dump(void* context, void* texture, const char* name);
static void on_native_scope(void* user, void* list, const rsf_game_render_pass* pass, uint32_t begin)
{ (void)user; (void)list; rsf_ac7_motion_capture_native_pass(pass, begin); }
static void on_native_cpu(void* user, const rsf_game_cpu_event* event)
{ (void)user; if (rsf_native_cpu_event(event)) rsf_native_fg_cpu(event); }

/* Serialize executable probing and plugin preparation. Host callbacks/user remain valid
   until quiesce/stop drains queued native work; repeated calls reuse the existing session result. */
int rsf_bridge_prepare_game(const wchar_t* plugin_path, const char* executable_sha256,
                            rsf_bridge_log_fn log, void* log_user)
{
    rsf_plugin_session_options options;
    rsf_game_probe probe;
    rsf_result result;
    AcquireSRWLockExclusive(&game_prepare_guard);
    if (game_session) {
        const int ready = game_prepared;
        ReleaseSRWLockExclusive(&game_prepare_guard); return ready;
    }
    memset(&probe, 0, sizeof(probe)); probe.struct_size = sizeof(probe);
    probe.pe_machine = 0x8664; probe.executable_name_utf8 = "Ace7Game.exe";
    probe.sha256_hex = executable_sha256;
    memset(&options, 0, sizeof(options)); options.struct_size = sizeof(options);
    options.abi_version = RSF_PLUGIN_SESSION_ABI_VERSION; options.plugin_path = plugin_path; options.probe = &probe;
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

/* Compatibility tail observation budgets in Present intervals. Stop retaining swapchain
   backbuffers promptly because outstanding references can block ResizeBuffers; engine pool
   targets are retained until their plan/watch ownership ends. */
#define RSF_TAIL_ARM_FRAMES 6ul
#define RSF_TAIL_STOP_FRAMES 32ul
/* Reserve two watch slots for backbuffer/composite discovery; remaining slots confirm
   same-size post-tonemap intermediates from observed producer bindings. */
#define RSF_CHAIN_CANDIDATES (RSF_FRAME_TAP_WATCH_SLOTS - 2u)
#define RSF_CHAIN_CONFIRM_DRAWS 4u
/* Periodically revisit the composite for chains that appear after initial discovery. */
#define RSF_CHAIN_RESCAN_FRAMES 300ul
#define RSF_CHAIN_RESCAN_DRAWS 16u
/* Bound the diagnostic lines emitted by each chain scan. */
#define RSF_CHAIN_SAID 24u
/* Age unused retained UI targets out of the plan after 120 Present intervals. */
#define RSF_UI_TARGET_STALE_FRAMES 120ul
/* Bound the backbuffer/composite draw reports independently of Present budgets. */
#define RSF_TAIL_BACK_BUFFER_DRAWS 8u
#define RSF_TAIL_COMPOSITE_DRAWS 64u


/* DXGI format families used by post-tonemap targets, including typeless storage. View
   format is tracked separately when replacements need a typed view. */
static int is_eight_bit_colour(unsigned long format)
{
    switch (format) {
    case 27ul: /* R8G8B8A8_TYPELESS */
    case 28ul: /* R8G8B8A8_UNORM */
    case 29ul: /* R8G8B8A8_UNORM_SRGB */
    case 87ul: /* B8G8R8A8_UNORM */
    case 88ul: /* B8G8R8X8_UNORM */
    case 90ul: /* B8G8R8A8_TYPELESS */
    case 91ul: /* B8G8R8A8_UNORM_SRGB */
        return 1;
    default:
        return 0;
    }
}


static struct {
    int started;
    void* device;
    void* context;
    rsf_bridge_log_fn log;
    void* log_user;

    /* Stage-specific diagnostic counters. Evaluation success does not establish image quality. */
    unsigned long passes;
    unsigned long view_read_failures;
    unsigned long not_main_view;
    unsigned long no_jitter;
    unsigned long evaluated;
    unsigned long refused;
    long last_result;

    /* What the last report said, so a report on a timer stays quiet while nothing moves. */
    unsigned long reported_calls;
    unsigned long reported_passes;
    unsigned long reported_evaluated;

    /* Optional debug blit of ungraded reconstruction; excludes the game UI. */
    rsf_present_blit* blit;
    int show;
    unsigned long frames_shown;
    uint64_t application_presented_frames;

    /* Bounded compatibility-pass tracing; multiple qualifying passes can occur per Present. */
    unsigned long pass_in_frame;
    unsigned long frames_described;


    /* Owned replay of layer geometry over scene depth, rebuilt when the layer extent changes. */
    rsf_depth_replay* layer_replay;
    unsigned long layer_replay_width;
    unsigned long layer_replay_height;
    /* The view the layer was drawn with, from its upload: the scene's camera at the layer's size,
       jitter in the layer's pixels. Valid for the frame it was uploaded in. */
    rsf_pipeline_camera_frame layer_camera;
    int layer_camera_valid;
    int layer_unjitter;
    int ui_unjitter;
    int enabled_requested;
    int startup_attempted;
    unsigned long enable_retry_after;
    unsigned long enable_after_evaluations;
    const char* setting_error;
    /* The integrated layer: the pipeline's output for it, and the view the recombine reads it
       through. The view belongs to the bridge; the texture to the pipeline. */
    void* layer_output;
    void* layer_output_view;
    unsigned long layer_integrations;
    unsigned long layer_integrations_refused;
    unsigned long layer_depth_missing;
    unsigned long depth_evaluations;
    unsigned long depth_candidates;
    unsigned long depth_candidate_width;
    unsigned long depth_candidate_height;
    unsigned long depth_candidate_samples;
    unsigned long geometry_draws;
    unsigned long geometry_traced;
    /* Indices drawn into the separate translucency layer, this frame and the last completed one.
       The second is kept so the report and the panel have something to show: the live one is zero
       for most of a frame and would read as "nothing there" whenever it was asked. */
    unsigned long translucent_indices;
    unsigned long translucent_indices_last;
    unsigned long translucent_draws;
    unsigned long translucent_draws_last;
    unsigned long depth_handover_traced;
    /* Owned layer reference retained beyond the borrowed geometry callback. Capture/integration
       require a current-frame producer; order_trace bounds post-layer composition tracing. */
    void* translucency_layer;
    unsigned long translucency_layer_seen_at;
    unsigned long order_trace;
    unsigned long depth_replayed;
    rsf_ac7_scene_color color_selection;
    unsigned long composed_evaluations;
    void* held_color;
    uint32_t native_input_mask;
    void* held_depth;
    void* held_motion;
    void* held_exposure;
    rsf_pipeline_camera_frame held_camera;
    unsigned long held_width;
    unsigned long held_height;
    /* The recombine route evaluates before this frame's qualifying pass has happened, so it works
       from the textures the last pass named, which are the same pooled targets every frame, and
       from this frame's camera, read from the main view's uniform buffer as the game uploads it. */
    void* last_depth;
    void* last_motion;
    void* last_exposure;
    rsf_pipeline_camera_frame upload_camera;
    unsigned long upload_width;
    unsigned long upload_height;
    int upload_camera_valid;
    /* Which route the plan takes. The recombine exists on screens with separate translucency, the
       briefing among them, and not on every menu; a plan waiting for a recombine that never comes
       never evaluates. So the route follows the frame: the recombine route while its gate opens,
       the composite route once it has stayed shut, and back when the recombine is seen again. */
    int recombine_gate_this_frame;
    unsigned long recombine_missing;
    int recombine_off;
    unsigned long finishes;
    unsigned long layer_draws_twinned;
    /* Previous successful tail and retry state. On rebuild refusal restore its plan when
       possible and delay another rebuild to avoid repeated frame disruption. */
    rsf_promote_frame_tail last_tail;
    int last_tail_valid;
    unsigned long plan_retry_after;
    unsigned long plan_restores;
    /* Bounded per-Present gate trace after plan installation or capture requests. */
    unsigned long gate_trace_left;
    /* Bounded raw/preview snapshots at render-thread route gates. Staging readback may stall. */
    char route_dump_prefix[520];
    unsigned long route_dump_frames;
    unsigned long route_dump_frame;
    unsigned long route_dump_serial;
    char briefing_capture_prefix[520];
    unsigned long briefing_capture_wait;
    unsigned long briefing_capture_draws;
    unsigned long briefing_shader_bytes;
    /* Compatibility main-view heuristic: compare the current recombine camera with the camera
       observed before the composite tonemap gate. Shared pooled targets do not identify a view. */
    rsf_pipeline_camera_frame main_camera_ref;
    int main_camera_ref_valid;
    unsigned long gates_declined_camera;
    unsigned long gate_decisions_logged;
    unsigned long layer_draws_untwinned;
    /* While the recombined target is promoted, rewrite matching render-size constants in
       uploaded postprocess buffers to the output extent so pixel-to-UV math follows the viewport. */
    void* size_patch_target;
    unsigned long size_uploads_patched;
    unsigned long sizes_patched;
    /* Individually published dimensions for the carrier jitter gate. Each word is atomic;
       the pair is not a synchronized settings snapshot. */
    volatile LONG view_width;
    volatile LONG view_height;
    int have_held;

    /* Compatibility discovery retains bounded watch targets: a final draw identifies a
       composite candidate, then draws into it identify the tonemap/chain. Bindings are possible
       reads, not shader access proof. The retained target addresses remain valid for plan matching. */
    void* back_buffer;
    void* composite;
    /* Kept separately from the pointer, which is dropped when the watch ends. What was learned
       outlives the reference that was needed to learn it. */
    int composite_found;
    unsigned long tail_frames;
    /* Gate/stall counters detect pool-role changes: retaining a target preserves its address
       but does not guarantee the engine continues using it as the composite. */
    unsigned long redirects_seen;
    unsigned long redirect_stall;
    /* Whether the installed plan's composite has opened a gate since it was installed, and the
       composite it was installed for, which a rebuild for a new layer keeps. */
    int plan_proven;
    void* plan_composite;
    unsigned long tail_restakes;
    int tail_restaking;
    /* The interface layers: the targets the classifier has seen widget quads drawn into, each
       retained because the plan names it by address, with the present it was last seen in so a
       layer the pool has retired ages out of the plan instead of holding a slot forever. */
    void* ui_targets[RSF_PROMOTE_MAX_UI_TARGETS];
    unsigned long ui_target_seen[RSF_PROMOTE_MAX_UI_TARGETS];
    uint32_t ui_target_count;
    int ui_targets_overflowed;
    /* Chain candidates, named by a composite draw's inputs and each watched for the draw that
       writes it, indexed by the watch slot above the tail's two; and the confirmed chain, which is
       what the plan promotes. All retained. */
    void* chain_candidates[RSF_CHAIN_CANDIDATES];
    void* chain_targets[RSF_PROMOTE_MAX_CHAIN_TARGETS];
    uint32_t chain_target_count;
    unsigned long chain_said;
    unsigned long chain_rescan_countdown;
    /* The render target view formats the game binds each promoted surface with, from the tap's
       reports, because the surfaces are typeless and the replacements' views need a format. Zero
       until seen, which promotion resolves to the plain UNORM member of the family. */
    uint32_t composite_view_format;
    uint32_t ui_target_view_format;
    uint32_t chain_view_format;
    void* hud_producer_shaders[16];
    uint32_t hud_producer_count;
    /* Set when either set changes under an installed plan, so the present hook rebuilds it. */
    int plan_stale;
    unsigned long presents;
    /* The presented size, kept here so a per-draw callback can judge against it without asking the
       pipeline for its status on every draw. */
    unsigned long output_width;
    unsigned long output_height;
    unsigned long tail_draws;

    /* The scene colour, taken from the frame and held. The plan names it by address and the tap
       never dereferences it, so a reference of our own is what keeps that address meaning what it
       meant when it was learned. */
    void* scene_color;

    /* Owned compatibility promotion resources and whether their substitution plan is armed. */
    rsf_promote* promote;
    int reinsert_on;
    /* The DLSS quality level in effect, for the panel, and whether the panel chose one before the
       backend started. */
    unsigned long quality;
    int quality_chosen;
    unsigned long reinsert_frames;
    unsigned long gate_evaluates;

    /* What the overlay can ask the carrier for. See dlss_bridge.h. */
    rsf_bridge_actions actions;

    /* Owned pipeline-object registry and observational class counters; extraction is explicit. */
    rsf_ui_registry* ui;
    int ui_classify;
    unsigned long ui_class_counts[7];
    unsigned long ui_candidate_draws;
    /* Record widget raster and destination extents independently; their difference controls
       promotion detail and does not identify a screen. */
    unsigned long ui_widget_extent[2];
    unsigned long ui_layer_extent[2];
    unsigned long ui_reported_counts[7];
    unsigned long ui_traced;


    /* Borrowed address identity of the current swapchain target, refreshed each Present.
     * Compared only; never dereferenced or retained here. Tail observation owns a separate
     * temporary backbuffer reference. This field alone cannot establish resize synchronization. */
    void* present_target;

    rsf_ui_layer* layer;
    rsf_fullscreen_pass* composite_pass;
    int ui_extract;
    int ui_extract_failed;
    unsigned long ui_composites;
} bridge;


static void say(const char* format, ...);

/* Publish the registry's sets to the tap, so its prefilter has something to match. Called after any
   change, which is rare: pipeline objects are created in bursts at load and then not at all. */
static void publish_ui_candidates(void)
{
    void* merged_layouts[128];
    uint32_t slate_count = 0;
    uint32_t canvas_count = 0;
    void* const* slate = rsf_ui_registry_view(bridge.ui, RSF_UI_SET_SLATE_LAYOUT, &slate_count);
    void* const* canvas = rsf_ui_registry_view(bridge.ui, RSF_UI_SET_CANVAS_LAYOUT, &canvas_count);
    uint32_t merged = 0;
    for (uint32_t index = 0; index < slate_count && merged < 128; ++index) {
        merged_layouts[merged++] = slate[index];
    }
    for (uint32_t index = 0; index < canvas_count && merged < 128; ++index) {
        merged_layouts[merged++] = canvas[index];
    }

    uint32_t widget_count = 0;
    void* const* widgets = rsf_ui_registry_view(bridge.ui, RSF_UI_SET_WIDGET_TARGET, &widget_count);
    uint32_t forced_count = 0;
    void* const* forced = rsf_ui_registry_view(bridge.ui, RSF_UI_SET_FORCE_SHADER, &forced_count);
    void* shaders[64];
    uint32_t shader_count = 0;
    for (uint32_t index = 0; index < bridge.hud_producer_count && shader_count < 64; ++index) {
        shaders[shader_count++] = bridge.hud_producer_shaders[index];
    }
    for (uint32_t index = 0; index < forced_count && shader_count < 64; ++index) {
        shaders[shader_count++] = forced[index];
    }

    rsf_frame_tap_candidates candidates;
    memset(&candidates, 0, sizeof(candidates));
    candidates.struct_size = sizeof(candidates);
    candidates.layouts = merged_layouts;
    candidates.layout_count = merged;
    candidates.widget_targets = widgets;
    candidates.widget_target_count = widget_count;
    candidates.shaders = shaders;
    candidates.shader_count = shader_count;
    rsf_frame_tap_set_candidates(&candidates);
}

/* Creation callback: forget reused addresses before classifying the complete copied layout.
   Partial/truncated declarations remain unclassified. */
static void on_layout_created(void* user, void* layout, const rsf_observer_layout_element* elements,
                              uint32_t copied, uint32_t count)
{
    rsf_ac7_layout_element facts[RSF_AC7_UI_MAX_LAYOUT_ELEMENTS];
    uint32_t index;
    (void)user;
    if (!bridge.ui || !layout) {
        return;
    }
    rsf_ui_registry_forget(bridge.ui, layout);
    if (copied != count || count > RSF_AC7_UI_MAX_LAYOUT_ELEMENTS) {
        return;
    }
    for (index = 0; index < copied; ++index) {
        facts[index].semantic_index = elements[index].semantic_index;
        facts[index].format = elements[index].format;
        facts[index].input_slot = elements[index].input_slot;
        facts[index].byte_offset = elements[index].byte_offset;
        facts[index].per_instance = elements[index].per_instance;
    }
    switch (rsf_ac7_ui_classify_layout(facts, copied)) {
    case RSF_AC7_LAYOUT_SLATE:
    case RSF_AC7_LAYOUT_SLATE_INSTANCED:
        rsf_ui_registry_add(bridge.ui, RSF_UI_SET_SLATE_LAYOUT, layout);
        publish_ui_candidates();
        break;
    case RSF_AC7_LAYOUT_CANVAS:
        rsf_ui_registry_add(bridge.ui, RSF_UI_SET_CANVAS_LAYOUT, layout);
        publish_ui_candidates();
        break;
    default:
        break;
    }
}

/* Bounded force/skip hash lists resolved to live object addresses by creation callbacks. */
#define RSF_UI_MAX_NAMED 16u
static unsigned long ui_forced_hashes[RSF_UI_MAX_NAMED];
static unsigned long ui_skipped_hashes[RSF_UI_MAX_NAMED];
static unsigned int ui_forced_count;
static unsigned int ui_skipped_count;
static unsigned long ui_hashes_seen;

/* Bounded pointer-to-hash diagnostic cache. Probe eight slots, then overwrite the initial
 * slot; collisions affect trace names rather than draw classification. */
#define RSF_UI_HASH_SLOTS 4096u
static struct {
    void* shader;
    unsigned long hash;
} ui_hash_table[RSF_UI_HASH_SLOTS];

static unsigned int ui_hash_slot(const void* shader)
{
    /* Fibonacci hashing on the pointer. Addresses are aligned, so the low bits are zeros and using
       them directly would pile every shader into a fraction of the table. */
    unsigned long long key = (unsigned long long)(size_t)shader;
    key *= 0x9E3779B97F4A7C15ull;
    return (unsigned int)((key >> 52) & (RSF_UI_HASH_SLOTS - 1u));
}

static void ui_remember_hash(void* shader, unsigned long hash)
{
    unsigned int slot = ui_hash_slot(shader);
    unsigned int probe;
    for (probe = 0; probe < 8u; ++probe) {
        const unsigned int index = (slot + probe) & (RSF_UI_HASH_SLOTS - 1u);
        if (ui_hash_table[index].shader == NULL || ui_hash_table[index].shader == shader) {
            ui_hash_table[index].shader = shader;
            ui_hash_table[index].hash = hash;
            return;
        }
    }
    ui_hash_table[slot].shader = shader;
    ui_hash_table[slot].hash = hash;
}

static unsigned long ui_hash_of(const void* shader)
{
    unsigned int slot;
    unsigned int probe;
    if (!shader) {
        return 0;
    }
    slot = ui_hash_slot(shader);
    for (probe = 0; probe < 8u; ++probe) {
        const unsigned int index = (slot + probe) & (RSF_UI_HASH_SLOTS - 1u);
        if (ui_hash_table[index].shader == shader) {
            return ui_hash_table[index].hash;
        }
        if (ui_hash_table[index].shader == NULL) {
            break;
        }
    }
    return 0;
}

static void ui_forget_hash(void* shader)
{
    unsigned int slot = ui_hash_slot(shader);
    unsigned int probe;
    for (probe = 0; probe < 8u; ++probe) {
        const unsigned int index = (slot + probe) & (RSF_UI_HASH_SLOTS - 1u);
        if (ui_hash_table[index].shader == shader) {
            ui_hash_table[index].shader = NULL;
            ui_hash_table[index].hash = 0;
            return;
        }
    }
}

/* Creation callback consumes borrowed bytecode synchronously, records bounded research
 * output, and resolves force/skip hashes. Forget prior registry/hash identities at reused
 * addresses. Forced membership wins when a hash appears in both lists. */
static void on_shader_created(void* user, void* shader, uint32_t stage, const void* bytecode,
                              uint32_t bytes)
{
    unsigned long hash;
    unsigned int index;
    (void)user;
    rsf_ac7_motion_capture_shader(shader, stage, bytecode, bytes);
    if (stage > RSF_OBSERVER_STAGE_PIXEL) {
        return;
    }
    if (!bridge.ui || !shader) {
        return;
    }
    rsf_ui_registry_forget(bridge.ui, shader);
    for (index = 0; index < bridge.hud_producer_count;) {
        if (bridge.hud_producer_shaders[index] == shader) {
            bridge.hud_producer_shaders[index] = bridge.hud_producer_shaders[--bridge.hud_producer_count];
        } else ++index;
    }
    if (stage == RSF_OBSERVER_STAGE_PIXEL && rsf_ac7_ui_is_hud_producer(bytecode, bytes) &&
        bridge.hud_producer_count < 16) {
        bridge.hud_producer_shaders[bridge.hud_producer_count++] = shader;
        say("ui: captured flight HUD fullscreen producer registered");
    }
    publish_ui_candidates();
    ui_forget_hash(shader);
    hash = (unsigned long)rsf_ui_shader_hash(bytecode, bytes);
    if (bridge.briefing_capture_prefix[0] && bytes <= 1024u * 1024u &&
        bridge.briefing_shader_bytes + bytes <= 64ul * 1024ul * 1024ul) {
        char path[600];
        FILE* file;
        snprintf(path, sizeof(path), "%s_shader_%u_%08lx.dxbc", bridge.briefing_capture_prefix,
                 (unsigned)stage, hash);
        file = fopen(path, "rb");
        if (file) {
            fclose(file);
        } else if ((file = fopen(path, "wb")) != NULL) {
            fwrite(bytecode, 1, bytes, file);
            fclose(file);
            bridge.briefing_shader_bytes += bytes;
        }
    }
    ui_remember_hash(shader, hash);
    ++ui_hashes_seen;
    for (index = 0; index < ui_forced_count; ++index) {
        if (ui_forced_hashes[index] == hash) {
            rsf_ui_registry_add(bridge.ui, RSF_UI_SET_FORCE_SHADER, shader);
            publish_ui_candidates();
            say("ui: shader 0x%08lx named by the settings as interface", hash);
            return;
        }
    }
    for (index = 0; index < ui_skipped_count; ++index) {
        if (ui_skipped_hashes[index] == hash) {
            rsf_ui_registry_add(bridge.ui, RSF_UI_SET_SKIP_SHADER, shader);
            say("ui: shader 0x%08lx named by the settings as not interface", hash);
            return;
        }
    }
}

/* Forget registry membership at reused texture addresses. Converter identity is learned
 * from Slate producer draws rather than texture dimensions alone. */
static void on_texture_created(void* user, void* texture, uint32_t width, uint32_t height,
                               uint32_t format, uint32_t mip_levels, uint32_t array_size,
                               uint32_t sample_count, uint32_t bind_flags, uint32_t misc_flags)
{
    (void)user;
    (void)width;
    (void)height;
    (void)format;
    (void)mip_levels;
    (void)array_size;
    (void)sample_count;
    (void)bind_flags;
    (void)misc_flags;
    if (!bridge.ui || !texture) {
        return;
    }
    rsf_ui_registry_forget(bridge.ui, texture);
}

/* Release every retained per-frame input reference and clear its availability flag. */
static void release_held(void)
{
    rsf_resource_release(bridge.held_color);
    rsf_resource_release(bridge.held_depth);
    rsf_resource_release(bridge.held_motion);
    rsf_resource_release(bridge.held_exposure);
    bridge.held_color = NULL;
    bridge.held_depth = NULL;
    bridge.held_motion = NULL;
    bridge.held_exposure = NULL;
    bridge.have_held = 0;
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

/* Route borrowed execution-stream passes by SDK role. Native modules copy identities or
   retain resources within the documented leases; SR runs inline before its native spatial fallback.
   CPU source identity and final window/Present ownership are checked by separate native modules. */
static void on_native_pass(void* user, void* list, const rsf_game_render_pass* pass, uint32_t begin)
{
    (void)user; (void)list;
    if (!pass || pass->struct_size != sizeof(*pass)) return;
    if (!begin && bridge.route_dump_frames && pass->color_output &&
        (pass->flags & RSF_GAME_RENDER_PRIMARY)) {
        char name[64];
        snprintf(name, sizeof(name), "native_pass_%u_output", pass->role);
        route_dump(bridge.context, pass->color_output, name);
    }
    if (pass->role == RSF_GAME_RENDER_TEXTURE_BINDING && !begin) rsf_native_scene_texture_binding(pass);
    if (pass->role == RSF_GAME_RENDER_FINAL_SCENE) {
        rsf_native_scene_pass(pass, begin);
        if (!begin) rsf_native_fg_final(bridge.context, pass);
    }
    if (pass->role == RSF_GAME_RENDER_WINDOW) {
        if (!begin) rsf_native_fg_window_end(pass);
        rsf_native_window_scope(pass, begin);
    }
    if ((pass->role == RSF_GAME_RENDER_TRANSLUCENCY || pass->role == RSF_GAME_RENDER_CLOUD_DEPTH ||
         pass->role == RSF_GAME_RENDER_MATERIALS) && bridge.started)
        rsf_native_translucency_pass(bridge.context, pass, begin);
    if (pass->role == RSF_GAME_RENDER_SUBMISSION) rsf_native_fg_submission(pass, begin);
    if (pass->role == RSF_GAME_RENDER_FRAME) rsf_native_fg_frame(pass, begin);
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
    InterlockedExchange(&bridge.view_width, (LONG)pass->camera.render_width);
    InterlockedExchange(&bridge.view_height, (LONG)pass->camera.render_height);
    bridge.held_camera.has_jitter = pass->camera_valid;
    memcpy(bridge.held_camera.jitter_pixels, pass->jitter_pixels, 8);
    bridge.held_width = pass->camera.render_width; bridge.held_height = pass->camera.render_height;
    bridge.last_result = rsf_native_sr_evaluate(bridge.context, pass);
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

/* Copied SDK directories for alternate SR providers; no DLL loading occurs in this setter. */
static char sdk_directories[4][1024];
void rsf_bridge_set_sdk_directories(const char* fsr2, const char* fsr3, const char* fsr4, const char* xess)
{
    const char* paths[4] = {fsr2, fsr3, fsr4, xess};
    unsigned int i;
    for (i = 0; i < 4; ++i) snprintf(sdk_directories[i], sizeof(sdk_directories[i]), "%s", paths[i] ? paths[i] : "");
}

static void fill_camera(const rsf_ac7_view* view, rsf_pipeline_camera_frame* camera)
{
    memset(camera, 0, sizeof(*camera));
    camera->struct_size = sizeof(*camera);
    camera->abi_version = RSF_FRAME_ASSEMBLY_ABI_VERSION;

    /* Backends receive matrices with temporal jitter removed; the engine upload itself is jittered. */
    memcpy(camera->view_to_clip, view->view_to_clip_no_jitter, sizeof(camera->view_to_clip));
    memcpy(camera->clip_to_view, view->clip_to_view_no_jitter, sizeof(camera->clip_to_view));
    memcpy(camera->clip_to_prev_clip, view->clip_to_prev_clip, sizeof(camera->clip_to_prev_clip));
    memcpy(camera->prev_clip_to_clip, view->prev_clip_to_clip, sizeof(camera->prev_clip_to_clip));

    memcpy(camera->camera_position, view->camera_position, sizeof(camera->camera_position));
    memcpy(camera->camera_forward, view->camera_forward, sizeof(camera->camera_forward));
    memcpy(camera->camera_up, view->camera_up, sizeof(camera->camera_up));
    memcpy(camera->camera_right, view->camera_right, sizeof(camera->camera_right));

    camera->near_plane = view->near_plane;
    /* Zero means infinite, which is what a reversed Z projection has and what the assembly turns
       into a number a backend can use. */
    camera->far_plane = 0.0f;
    camera->vertical_fov = view->vertical_fov;
    camera->aspect_ratio = view->aspect_ratio;

    camera->jitter_pixels[0] = view->jitter_pixels[0];
    camera->jitter_pixels[1] = view->jitter_pixels[1];
    camera->has_jitter = view->has_jitter;

    /* AC7 projection is reversed Z. Sparse engine motion does not provide camera velocity
       for all pixels; the pipeline reconstructs missing camera motion. */
    camera->depth_inverted = 1u;
    camera->camera_motion_included = 0u;
}

/* Compatibility input discovery on the render thread. Copy camera metadata and retain
   the selected textures; later draws still add sky/translucency. Evaluation occurs at the
   matching reinsertion gate or Present, after scene colour is complete. */
static void on_pass(void* user, const rsf_frame_tap_pass* pass)
{
    unsigned char view_bytes[RSF_AC7_VIEW_BUFFER_BYTES];
    rsf_ac7_view view;
    rsf_pipeline_camera_frame camera;
    rsf_dlss_pipeline_frame frame;
    rsf_dlss_pipeline_result result;

    (void)user;
    if (InterlockedCompareExchange(&native_owner, 0, 0) || !bridge.started || !pass || !pass->view_constants) {
        return;
    }
    ++bridge.passes;

    if (rsf_read_constant_buffer(bridge.device, pass->context, pass->view_constants, view_bytes,
                                 sizeof(view_bytes)) != RSF_CONSTANT_BUFFER_OK) {
        ++bridge.view_read_failures;
        return;
    }

    memset(&view, 0, sizeof(view));
    view.struct_size = sizeof(view);
    if (rsf_ac7_view_read(view_bytes, (uint32_t)sizeof(view_bytes), RSF_AC7_VIEW_ABI_VERSION,
                          &view) != RSF_AC7_VIEW_OK) {
        ++bridge.view_read_failures;
        return;
    }

    /* Reject secondary-view camera metadata even when pooled render targets are shared. */
    if (!view.is_main_view) {
        ++bridge.not_main_view;
        return;
    }
    /* Publish the render extent before checking jitter so a changed size can reopen its gate. */
    InterlockedExchange(&bridge.view_width, (LONG)pass->render_width);
    InterlockedExchange(&bridge.view_height, (LONG)pass->render_height);
    if (!view.has_jitter) {
        /* Track missing jitter separately from missing resources or a failed view read. */
        ++bridge.no_jitter;
        return;
    }

    ++bridge.pass_in_frame;
    if (bridge.frames_described < 4) {
        /* Trace each qualifying pass while discovery is active; shared bindings can carry partial colour. */
        say("  pass %lu of this frame: colour %p format %lu at %ux%u, %s exposure, view %ux%u in "
            "%ux%u, camera at %.0f %.0f %.0f looking %.2f %.2f %.2f, jitter %.2f %.2f px",
            bridge.pass_in_frame, pass->scene_color, (unsigned long)pass->scene_color_format,
            pass->render_width, pass->render_height, pass->exposure ? "with" : "no",
            view.view_width, view.view_height, view.buffer_width, view.buffer_height,
            view.camera_position[0], view.camera_position[1], view.camera_position[2],
            view.camera_forward[0], view.camera_forward[1], view.camera_forward[2],
            view.jitter_pixels[0], view.jitter_pixels[1]);
    }

    fill_camera(&view, &camera);

    /* Held rather than evaluated. The camera is a plain structure and is copied; the textures are
       borrowed for this callback only, so keeping them past it means taking a reference. One set
       per frame: if a second pass somehow qualifies, the first is dropped rather than leaked. */
    if (bridge.have_held) {
        release_held();
    }
    bridge.held_color = pass->scene_color;
    bridge.held_depth = pass->depth;
    bridge.held_motion = pass->motion;
    bridge.held_exposure = pass->exposure;
    rsf_resource_retain(bridge.held_color);
    rsf_resource_retain(bridge.held_depth);
    rsf_resource_retain(bridge.held_motion);
    rsf_resource_retain(bridge.held_exposure);
    bridge.held_camera = camera;
    if (bridge.last_depth != pass->depth) {
        rsf_resource_release(bridge.last_depth);
        bridge.last_depth = pass->depth;
        rsf_resource_retain(bridge.last_depth);
    }
    if (bridge.last_motion != pass->motion) {
        rsf_resource_release(bridge.last_motion);
        bridge.last_motion = pass->motion;
        rsf_resource_retain(bridge.last_motion);
    }
    if (bridge.last_exposure != pass->exposure) {
        rsf_resource_release(bridge.last_exposure);
        bridge.last_exposure = pass->exposure;
        rsf_resource_retain(bridge.last_exposure);
    }
    bridge.held_width = pass->render_width;
    bridge.held_height = pass->render_height;
    bridge.have_held = 1;
    if (rsf_ac7_scene_color_source(&bridge.color_selection, pass->scene_color, pass->context,
                                   pass->render_width, pass->render_height)) {
        rsf_frame_tap_watch_input(bridge.color_selection.source);
    }

    /* Retain scene identity for substitution matching and replace it when the pool/extent changes. */
    if (bridge.scene_color != pass->scene_color) {
        rsf_resource_release(bridge.scene_color);
        bridge.scene_color = pass->scene_color;
        rsf_resource_retain(bridge.scene_color);
    }
    (void)frame;
    (void)result;
}

/* Bound verbose depth-replay candidate tracing; summary counters continue after the budget. */
#define RSF_GEOMETRY_TRACE_DRAWS 400u

/* Observe compatibility translucency candidates, retain the current layer, count geometry,
   and replay depth only for the optional jittered integration route. */
static void on_geometry(void* user, const rsf_frame_tap_geometry* draw)
{
    if (InterlockedCompareExchange(&native_owner, 0, 0)) return;
    unsigned int rejects[2];
    (void)user;
    if (!bridge.started) {
        return;
    }
    /* Count hook traffic separately from accepted depth-replay candidates. */
    ++bridge.geometry_draws;
    if (!rsf_ac7_scene_depth_candidate(draw)) {
        if (bridge.geometry_traced < RSF_GEOMETRY_TRACE_DRAWS) {
            ++bridge.geometry_traced;
            say("geometry %lu: NOT a candidate. target %p %lux%lu format %lu samples %lu, depth "
                "view %p, kind %lu topology %lu, %lu elements, %lu instances",
                bridge.geometry_draws, draw->target, (unsigned long)draw->width,
                (unsigned long)draw->height, (unsigned long)draw->format,
                (unsigned long)draw->samples, draw->depth_view, (unsigned long)draw->kind,
                (unsigned long)draw->topology, (unsigned long)draw->count,
                (unsigned long)draw->instances);
        }
        return;
    }
    ++bridge.depth_candidates;
    if (draw->target != bridge.translucency_layer) {
        rsf_resource_retain(draw->target);
        rsf_resource_release(bridge.translucency_layer);
        bridge.translucency_layer = draw->target;
        /* A new layer means a new tail: the recombine's reads of it are what the plan names. */
        bridge.plan_stale = 1;
        say("translucency layer %p %lux%lu: %s before the recombine",
            draw->target, (unsigned long)draw->width, (unsigned long)draw->height,
            bridge.layer_unjitter ? "unjittered, no temporal integration" : "integrated at one to one");
    }
    bridge.translucency_layer_seen_at = bridge.presents;
    bridge.depth_candidate_width = draw->width;
    bridge.depth_candidate_height = draw->height;
    bridge.depth_candidate_samples = draw->samples;
    /* Count instanced geometry volume as well as draws for the carrier layer-scale policy. */
    ++bridge.translucent_draws;
    bridge.translucent_indices +=
        (unsigned long)draw->count * (draw->instances ? (unsigned long)draw->instances : 1ul);

    rejects[0] = 0;
    rejects[1] = 0;
    if (bridge.layer_replay && !bridge.layer_unjitter) {
        const uint32_t replayed = rsf_depth_replay_draw(bridge.layer_replay, draw);
        bridge.depth_replayed += replayed;
        rejects[0] = replayed ? 0u : rsf_depth_replay_last_reject(bridge.layer_replay);
    }

    if (bridge.geometry_traced < RSF_GEOMETRY_TRACE_DRAWS) {
        ++bridge.geometry_traced;
        say("geometry %lu: candidate. target %p %lux%lu format %lu samples %lu, depth view %p, "
            "kind %lu topology %lu, %lu elements, %lu instances, vs %p. Replay targets %lux%lu "
            "and %lux%lu refused %lu and %lu",
            bridge.geometry_draws, draw->target, (unsigned long)draw->width,
            (unsigned long)draw->height, (unsigned long)draw->format,
            (unsigned long)draw->samples, draw->depth_view, (unsigned long)draw->kind,
            (unsigned long)draw->topology, (unsigned long)draw->count,
            (unsigned long)draw->instances, draw->vertex_shader,
            bridge.layer_replay_width, bridge.layer_replay_height, 0ul, 0ul,
            (unsigned long)rejects[0], (unsigned long)rejects[1]);
    }
}

static void describe_inputs(const rsf_frame_tap_target_draw* draw);

static void on_input_draw(void* user, const rsf_frame_tap_target_draw* draw)
{
    if (InterlockedCompareExchange(&native_owner, 0, 0)) return;
    (void)user;
    rsf_ac7_scene_color_draw(&bridge.color_selection, draw);
}

/* Trace bound 2D resources with original slot indices and known route identities.
   Non-texture gaps are preserved; a binding alone does not prove the shader reads it. */
static void describe_inputs(const rsf_frame_tap_target_draw* draw)
{
    uint32_t index;
    for (index = 0; index < draw->input_count; ++index) {
        const rsf_frame_tap_input* input = &draw->inputs[index];
        say("    slot %lu: %p %lux%lu format %lu%s", (unsigned long)input->slot, input->texture,
            (unsigned long)input->width, (unsigned long)input->height,
            (unsigned long)input->format,
            (input->texture && input->texture == bridge.held_color) ? "  <- scene colour"
            : (input->texture && input->texture == bridge.translucency_layer)
                ? "  <- translucency layer"
            : (input->texture && input->texture == bridge.color_selection.composed)
                ? "  <- recombined colour"
                : "");
    }
}

/* Retained compatibility UI/chain identities come from producer observations, not
   descriptor shape alone. Drawing hooks mark plans stale; Present rebuilds resources. */

static void forget_chain_candidate(void* target);

static int is_ui_target(const void* texture)
{
    uint32_t index;
    for (index = 0; index < bridge.ui_target_count; ++index) {
        if (bridge.ui_targets[index] == texture) {
            return 1;
        }
    }
    return 0;
}

static int is_chain_target(const void* texture)
{
    uint32_t index;
    for (index = 0; index < bridge.chain_target_count; ++index) {
        if (bridge.chain_targets[index] == texture) {
            return 1;
        }
    }
    return 0;
}

/* A widget quad was drawn into this target. Called on the render thread from inside a draw hook,
   so it compares, retains and sets a flag; the present hook creates the replacement. */
static void note_ui_target(void* target)
{
    uint32_t index;
    if (!target || target == bridge.present_target) {
        return;
    }
    for (index = 0; index < bridge.ui_target_count; ++index) {
        if (bridge.ui_targets[index] == target) {
            bridge.ui_target_seen[index] = bridge.presents;
            return;
        }
    }
    if (bridge.ui_target_count >= RSF_PROMOTE_MAX_UI_TARGETS) {
        if (!bridge.ui_targets_overflowed) {
            bridge.ui_targets_overflowed = 1;
            say("interface layer: %p is one more than the %u there is room for, so it stays at "
                "render resolution",
                target, (unsigned)RSF_PROMOTE_MAX_UI_TARGETS);
        }
        return;
    }
    rsf_resource_retain(target);
    bridge.ui_targets[bridge.ui_target_count] = target;
    bridge.ui_target_seen[bridge.ui_target_count] = bridge.presents;
    ++bridge.ui_target_count;
    bridge.plan_stale = 1;
    say("interface layer: widget quads draw into %p, now one of %u to promote", target,
        (unsigned)bridge.ui_target_count);
    /* A layer is written by the quads and not from the composite, so it is not a chain target
       whatever the composite watch made of it before the classifier saw a quad. */
    forget_chain_candidate(target);
}

/* Release retired UI identities and compact the set after its inactivity budget.
   Removing an entry marks the promotion plan stale. */
static void age_ui_targets(void)
{
    uint32_t index = 0;
    while (index < bridge.ui_target_count) {
        if (bridge.presents - bridge.ui_target_seen[index] <= RSF_UI_TARGET_STALE_FRAMES) {
            ++index;
            continue;
        }
        say("interface layer: %p has not been drawn into for %lu presents, no longer promoted",
            bridge.ui_targets[index], RSF_UI_TARGET_STALE_FRAMES);
        rsf_resource_release(bridge.ui_targets[index]);
        --bridge.ui_target_count;
        bridge.ui_targets[index] = bridge.ui_targets[bridge.ui_target_count];
        bridge.ui_target_seen[index] = bridge.ui_target_seen[bridge.ui_target_count];
        bridge.ui_targets[bridge.ui_target_count] = NULL;
        bridge.ui_targets_overflowed = 0;
        bridge.plan_stale = 1;
    }
}

static void forget_chain_candidate(void* target)
{
    uint32_t index;
    for (index = 0; index < RSF_CHAIN_CANDIDATES; ++index) {
        if (bridge.chain_candidates[index] == target) {
            rsf_frame_tap_watch_target(2u + index, NULL, 0);
            rsf_resource_release(target);
            bridge.chain_candidates[index] = NULL;
        }
    }
    index = 0;
    while (index < bridge.chain_target_count) {
        if (bridge.chain_targets[index] != target) {
            ++index;
            continue;
        }
        say("chain target: %p turned out to be an interface layer, no longer promoted as chain",
            target);
        rsf_resource_release(target);
        --bridge.chain_target_count;
        bridge.chain_targets[index] = bridge.chain_targets[bridge.chain_target_count];
        bridge.chain_targets[bridge.chain_target_count] = NULL;
        bridge.plan_stale = 1;
    }
}

/* Stop watching and let go of everything the chain holds. The chain belongs to the composite and
   goes stale with it. */
static void clear_chain(void)
{
    uint32_t index;
    for (index = 0; index < RSF_CHAIN_CANDIDATES; ++index) {
        if (bridge.chain_candidates[index]) {
            rsf_frame_tap_watch_target(2u + index, NULL, 0);
            rsf_resource_release(bridge.chain_candidates[index]);
            bridge.chain_candidates[index] = NULL;
        }
    }
    for (index = 0; index < bridge.chain_target_count; ++index) {
        rsf_resource_release(bridge.chain_targets[index]);
        bridge.chain_targets[index] = NULL;
    }
    bridge.chain_target_count = 0;
    bridge.chain_said = 0;
}

/* A composite draw's inputs name the candidates: eight bit, the composite's own size, and none of
   the surfaces already accounted for. Each one gets a watch of its own, so the draw that writes it
   can say whether it is produced from the composite. */
static void note_chain_candidates(const rsf_frame_tap_target_draw* draw)
{
    uint32_t index;
    for (index = 0; index < draw->input_count; ++index) {
        const rsf_frame_tap_input* input = &draw->inputs[index];
        uint32_t slot;
        if (!input->texture || !is_eight_bit_colour(input->format) ||
            input->width != draw->target_width || input->height != draw->target_height ||
            input->texture == bridge.composite || input->texture == bridge.scene_color ||
            is_ui_target(input->texture) || is_chain_target(input->texture)) {
            continue;
        }
        for (slot = 0; slot < RSF_CHAIN_CANDIDATES; ++slot) {
            if (bridge.chain_candidates[slot] == input->texture) {
                break;
            }
        }
        if (slot < RSF_CHAIN_CANDIDATES) {
            continue; /* already being watched */
        }
        for (slot = 0; slot < RSF_CHAIN_CANDIDATES; ++slot) {
            if (!bridge.chain_candidates[slot]) {
                break;
            }
        }
        if (slot == RSF_CHAIN_CANDIDATES) {
            if (bridge.chain_said < RSF_CHAIN_SAID) {
                ++bridge.chain_said;
                say("chain candidate: %p is read by a composite draw and every confirmation watch "
                    "is busy, so it waits for the next look",
                    input->texture);
            }
            continue;
        }
        rsf_resource_retain(input->texture);
        bridge.chain_candidates[slot] = input->texture;
        rsf_frame_tap_watch_target(2u + slot, input->texture, RSF_CHAIN_CONFIRM_DRAWS);
        if (bridge.chain_said < RSF_CHAIN_SAID) {
            ++bridge.chain_said;
            say("chain candidate: %p, %lux%lu format %lu in slot %lu of a composite draw. Watching "
                "what writes it",
                input->texture, (unsigned long)input->width, (unsigned long)input->height,
                (unsigned long)input->format, (unsigned long)input->slot);
        }
    }
}

/* The draw that writes a candidate. Reading the composite, or a target already in the chain, is
   what makes it chain; anything else is some other surface that happened to be eight bit and the
   size of the frame, and promoting it would move a pass whose role is unknown. Bindings establish
   possible reads rather than reads, so the inputs are described for a reader to judge. */
static void confirm_chain_candidate(const rsf_frame_tap_target_draw* draw)
{
    const uint32_t slot = draw->watch_index - 2u;
    void* candidate;
    uint32_t index;
    int reads_chain = 0;

    if (slot >= RSF_CHAIN_CANDIDATES || !bridge.chain_candidates[slot]) {
        return;
    }
    candidate = bridge.chain_candidates[slot];
    for (index = 0; index < draw->input_count; ++index) {
        const void* texture = draw->inputs[index].texture;
        if (texture && (texture == bridge.composite || is_chain_target(texture))) {
            reads_chain = 1;
            break;
        }
    }
    rsf_frame_tap_watch_target(2u + slot, NULL, 0);
    bridge.chain_candidates[slot] = NULL;
    if (!reads_chain) {
        if (bridge.chain_said < RSF_CHAIN_SAID) {
            ++bridge.chain_said;
            say("chain candidate: %p is written by a draw that reads no part of the chain, %s %lu "
                "with %lu inputs, so it is not promoted",
                candidate, draw->indexed ? "indices" : "vertices",
                (unsigned long)draw->element_count, (unsigned long)draw->input_count);
            describe_inputs(draw);
        }
        rsf_resource_release(candidate);
        return;
    }
    if (bridge.chain_target_count >= RSF_PROMOTE_MAX_CHAIN_TARGETS) {
        say("chain target: %p is produced from the composite and there is no room for a %uth, so "
            "it stays at render resolution",
            candidate, (unsigned)(RSF_PROMOTE_MAX_CHAIN_TARGETS + 1u));
        rsf_resource_release(candidate);
        return;
    }
    /* Keeps the candidate's retain. */
    bridge.chain_targets[bridge.chain_target_count++] = candidate;
    bridge.chain_view_format = draw->target_view_format;
    bridge.plan_stale = 1;
    say("chain target: %p is produced from the composite by a draw with %lu inputs, now one of %u "
        "to promote",
        candidate, (unsigned long)draw->input_count, (unsigned)bridge.chain_target_count);
    describe_inputs(draw);
}

/* Release unconfirmed chain watches at the end of each scan. No observed draw means
   the writer was not seen; copying or earlier production remain possible explanations. */
static void settle_chain_candidates(void)
{
    uint32_t slot;
    for (slot = 0; slot < RSF_CHAIN_CANDIDATES; ++slot) {
        if (!bridge.chain_candidates[slot]) {
            continue;
        }
        if (bridge.chain_said < RSF_CHAIN_SAID) {
            ++bridge.chain_said;
            say("chain candidate: %p was never drawn into while watched, so it is filled by a copy "
                "or before the tail, and is not promoted",
                bridge.chain_candidates[slot]);
        }
        rsf_frame_tap_watch_target(2u + slot, NULL, 0);
        rsf_resource_release(bridge.chain_candidates[slot]);
        bridge.chain_candidates[slot] = NULL;
    }
}

/* Watch the composite again, briefly and periodically, once the tail walk is over. */
static void chain_rescan_tick(void)
{
    if (!bridge.composite || bridge.tail_frames <= RSF_TAIL_STOP_FRAMES) {
        return;
    }
    if (bridge.chain_rescan_countdown > 0) {
        --bridge.chain_rescan_countdown;
        return;
    }
    bridge.chain_rescan_countdown = RSF_CHAIN_RESCAN_FRAMES;
    settle_chain_candidates();
    rsf_frame_tap_watch_target(1, bridge.composite, RSF_CHAIN_RESCAN_DRAWS);
}


/* Build borrowed AC7 draw facts from the registry and tap report, then classify once
 * through the same rules used by counting and diversion. Update recent converter/UI targets
 * from observed producers. Unknown bindings remain conservative. */
static rsf_ac7_draw_class classify_candidate(const rsf_frame_tap_target_draw* draw)
{
    rsf_ac7_draw_facts facts;
    rsf_ac7_ui_registry rules;
    rsf_ac7_draw_input inputs[RSF_AC7_UI_MAX_INPUTS];
    uint32_t slate_count = 0;
    uint32_t canvas_count = 0;
    uint32_t widget_count = 0;
    uint32_t forced_count = 0;
    uint32_t skip_count = 0;
    uint32_t index;
    uint32_t used = 0;
    rsf_ac7_draw_class verdict;
    if (!draw || !bridge.ui) {
        return RSF_AC7_DRAW_SCENE;
    }

    memset(&rules, 0, sizeof(rules));
    rules.struct_size = sizeof(rules);
    rules.slate_layouts = rsf_ui_registry_view(bridge.ui, RSF_UI_SET_SLATE_LAYOUT, &slate_count);
    rules.slate_layout_count = slate_count;
    rules.canvas_layouts = rsf_ui_registry_view(bridge.ui, RSF_UI_SET_CANVAS_LAYOUT, &canvas_count);
    rules.canvas_layout_count = canvas_count;
    rules.widget_targets = rsf_ui_registry_view(bridge.ui, RSF_UI_SET_WIDGET_TARGET, &widget_count);
    rules.widget_target_count = widget_count;
    rules.force_shaders = rsf_ui_registry_view(bridge.ui, RSF_UI_SET_FORCE_SHADER, &forced_count);
    rules.force_shader_count = forced_count;
    rules.skip_shaders = rsf_ui_registry_view(bridge.ui, RSF_UI_SET_SKIP_SHADER, &skip_count);
    rules.skip_shader_count = skip_count;
    /* Classify Slate against the final backbuffer identity, not the reduced composite. */
    rules.back_buffer = bridge.present_target;

    memset(&facts, 0, sizeof(facts));
    facts.struct_size = sizeof(facts);
    facts.input_layout = draw->input_layout;
    facts.vertex_shader = draw->vertex_shader;
    facts.pixel_shader = draw->pixel_shader;
    facts.render_target = draw->render_target;
    facts.target_width = draw->target_width;
    facts.target_height = draw->target_height;
    facts.target_count = draw->target_count;
    facts.depth_bound = draw->depth_bound;
    facts.indexed = draw->indexed;
    facts.element_count = draw->element_count;
    facts.vertex_stride = draw->vertex_stride;
    /* The blend state is shadowed by pointer and its factors are not readable without a device
       call, so the rule sees the over blend AC7's interface uses. Reading the description belongs
       with the divert, which needs it anyway to patch the alpha operations. */
    facts.blend_enabled = 1;
    facts.src_blend = RSF_AC7_BLEND_SRC_ALPHA;
    facts.dest_blend = RSF_AC7_BLEND_INV_SRC_ALPHA;
    for (index = 0; index < draw->input_count && used < RSF_AC7_UI_MAX_INPUTS; ++index) {
        inputs[used].slot = draw->inputs[index].slot;
        inputs[used].texture = draw->inputs[index].texture;
        inputs[used].width = draw->inputs[index].width;
        inputs[used].height = draw->inputs[index].height;
        ++used;
    }
    facts.input_count = used;
    facts.inputs = inputs;

    verdict = rsf_ac7_ui_classify(&rules, &facts);

    /* Flight uses a fullscreen HUD producer instead of the briefing's widget quads.
       Follow the producer's output on every draw, including when widget content is cached. */
    for (index = 0; index < bridge.hud_producer_count; ++index) {
        if (draw->pixel_shader == bridge.hud_producer_shaders[index] &&
            draw->element_count == 3 && !draw->depth_bound && draw->target_count == 1 &&
            draw->target_format == 27 && draw->render_target != rules.back_buffer) {
            uint32_t input_index;
            for (input_index = 0; input_index < used; ++input_index) {
                if (inputs[input_index].slot == 1 && inputs[input_index].texture &&
                    rsf_ui_registry_contains(bridge.ui, RSF_UI_SET_WIDGET_TARGET, inputs[input_index].texture)) {
                    note_ui_target(draw->render_target);
                    bridge.ui_target_view_format = draw->target_view_format;
                    break;
                }
            }
        }
    }

    /* A Slate producer draw confirms converter membership before its output is sampled.
     * Descriptor shape is insufficient because many unrelated pooled textures share it. */
    /* Refresh recent membership on every producer draw so retired menu targets can be evicted. */
    if (draw->render_target && draw->render_target != rules.back_buffer &&
        rsf_ui_registry_contains(bridge.ui, RSF_UI_SET_SLATE_LAYOUT, draw->input_layout)) {
        if (rsf_ui_registry_note_recent(bridge.ui, RSF_UI_SET_WIDGET_TARGET,
                                        draw->render_target)) {
            publish_ui_candidates();
        }
    }

    if (verdict == RSF_AC7_DRAW_UI_WIDGET_QUAD) {
        /* Record both source raster and destination layer extents. */
        for (index = 0; index < used; ++index) {
            if (inputs[index].width != 0) {
                bridge.ui_widget_extent[0] = inputs[index].width;
                bridge.ui_widget_extent[1] = inputs[index].height;
                break;
            }
        }
        bridge.ui_layer_extent[0] = draw->target_width;
        bridge.ui_layer_extent[1] = draw->target_height;
        /* And the target itself: it is one of AC7's interface layers, and promotion names it. */
        note_ui_target(draw->render_target);
        bridge.ui_target_view_format = draw->target_view_format;
    }
    return verdict;
}

/* Count what the classifier saw. Counting only: what moves is decided in `ui_verdict`, from the
   same call, so the report and the picture cannot disagree. */
static void on_candidate_draw(void* user, const rsf_frame_tap_target_draw* draw)
{
    if (InterlockedCompareExchange(&native_owner, 0, 0)) return;
    rsf_ac7_draw_class verdict;
    uint32_t index;
    (void)user;

    if (!draw || !bridge.ui || !bridge.ui_classify) {
        return;
    }
    ++bridge.ui_candidate_draws;
    verdict = classify_candidate(draw);
    if (verdict < 7) {
        ++bridge.ui_class_counts[verdict];
    }

    if (bridge.ui_traced < 24u && verdict != RSF_AC7_DRAW_SCENE) {
        ++bridge.ui_traced;
        /* Use stable bytecode hashes for diagnostic overrides. */
        /* Texture storage can be typeless; typed views determine this draw's colour conversion. */
        say("  ui draw: class %u, vs 0x%08lx, ps 0x%08lx, layout %p, %s %lu, stride %lu, "
            "target %p %lux%lu texture format %lu view format %lu, depth %lu, targets %lu, "
            "inputs %lu",
            (unsigned)verdict, ui_hash_of(draw->vertex_shader), ui_hash_of(draw->pixel_shader),
            draw->input_layout, draw->indexed ? "indices" : "vertices",
            (unsigned long)draw->element_count, (unsigned long)draw->vertex_stride,
            draw->render_target, (unsigned long)draw->target_width,
            (unsigned long)draw->target_height, (unsigned long)draw->target_format,
            (unsigned long)draw->target_view_format, (unsigned long)draw->depth_bound,
            (unsigned long)draw->target_count, (unsigned long)draw->input_count);
        /* Trace widget/blur/glow inputs to distinguish effects carried by the classified quad
           from effects produced by other passes. */
        for (index = 0; index < draw->input_count && index < 8; ++index) {
            say("    input slot %lu: %p %lux%lu format %lu",
                (unsigned long)draw->inputs[index].slot, draw->inputs[index].texture,
                (unsigned long)draw->inputs[index].width, (unsigned long)draw->inputs[index].height,
                (unsigned long)draw->inputs[index].format);
        }
    }
}

/* Dispatch bounded watch reports to chain confirmation or composite discovery. Resource
   identity is retained when learned; no backend evaluation occurs during these reports. */
static void on_target_draw(void* user, const rsf_frame_tap_target_draw* draw)
{
    if (InterlockedCompareExchange(&native_owner, 0, 0)) return;
    (void)user;
    if (!draw) {
        return;
    }
    /* The slots above the tail's two are chain confirmation watches, one draw each. */
    if (draw->watch_index >= 2) {
        confirm_chain_candidate(draw);
        return;
    }
    ++bridge.tail_draws;

    /* Bound verbose tail logging across periodic rescans and repeated pool rediscovery. */
    if (bridge.tail_frames <= RSF_TAIL_STOP_FRAMES && bridge.tail_restakes <= 5) {
        say("%s draw %lu: target %p %lux%lu format %lu, viewport %lux%lu, %s %lu, %lu inputs",
            draw->watch_index == 0 ? "back buffer" : "composite", (unsigned long)draw->draw_index,
            draw->render_target, (unsigned long)draw->target_width,
            (unsigned long)draw->target_height, (unsigned long)draw->target_format,
            (unsigned long)draw->viewport_width, (unsigned long)draw->viewport_height,
            draw->indexed ? "indices" : "vertices", (unsigned long)draw->element_count,
            (unsigned long)draw->input_count);
        describe_inputs(draw);
    }

    /* A draw into the composite. Its eight bit inputs at the composite's own size are the chain
       candidates: the intermediates between the tonemap and the interface composite that would
       otherwise downsample the promoted composite back to render resolution. */
    if (draw->watch_index == 1) {
        if (!bridge.composite_view_format) {
            bridge.composite_view_format = draw->target_view_format;
        }
        note_chain_candidates(draw);
    }

    /* Discover a composite only from watched backbuffer draws; an existing composite is retained. */
    if (draw->watch_index != 0 || bridge.composite) {
        return;
    }
    /* Compatibility heuristic: select the first large eight-bit bound texture, excluding
       backend output. Other slots may be stale because D3D11 retains unused SRVs. This is
       binding/descriptor evidence rather than proof of shader read ownership. */
    const rsf_frame_tap_input* composite = NULL;
    {
        uint32_t index;
        for (index = 0; index < draw->input_count; ++index) {
            const rsf_frame_tap_input* input = &draw->inputs[index];
            if (!input->texture || !is_eight_bit_colour(input->format)) {
                continue;
            }
            /* Half the target's height is a wide margin: the composite is either the size of the
               back buffer or exactly half it at a reduced render scale, and every mip and bar in
               this draw is far smaller. */
            if (input->height * 2u < draw->target_height) {
                continue;
            }
            composite = input;
            break;
        }
    }
    if (!composite) {
        return;
    }
    /* Our own debug blit also draws over the back buffer, from inside the Present hook, and nothing
       about its shape distinguishes it from the game's last draw. Taking it would point the
       reinsertion at the reconstruction's own output. */
    if (composite->texture == rsf_dlss_pipeline_output_texture()) {
        return;
    }
    bridge.composite = composite->texture;
    bridge.composite_found = 1;
    rsf_resource_retain(bridge.composite);
    rsf_frame_tap_watch_target(1, bridge.composite, RSF_TAIL_COMPOSITE_DRAWS);
    say("  slot %lu, %lux%lu format %lu, is the composite the scene has to be replaced in. "
        "Watching it: the draw into it that reads scene colour is where the reconstruction goes "
        "back",
        (unsigned long)composite->slot, (unsigned long)composite->width,
        (unsigned long)composite->height, (unsigned long)composite->format);
}


/* Present-interval stall budgets: allow longer pauses for a plan with proven gate
   activity and rediscover unproven plans sooner. */
#define RSF_REINSERT_STALL_FRAMES 240ul
#define RSF_REINSERT_UNPROVEN_STALL_FRAMES 30ul


static int install_reinsert_plan(void);
static void stop_reinsert(void);


/* Frames without a recombine before the plan leaves the recombine route. Under the unproven stall
   patience, so a menu reached from the briefing switches routes rather than restaking. */
#define RSF_RECOMBINE_ABSENT_FRAMES 20u

/* Switch compatibility insertion between recombine and tonemap as current-frame producers
   appear/disappear. Mark the plan stale; replacement resources are rebuilt later at Present. */
static void follow_recombine(void)
{
    rsf_promote_status status;
    const int seen = bridge.recombine_gate_this_frame;

    bridge.recombine_gate_this_frame = 0;
    if (!bridge.promote || bridge.plan_stale) {
        return;
    }
    memset(&status, 0, sizeof(status));
    status.struct_size = sizeof(status);
    if (rsf_promote_get_status(bridge.promote, &status) != RSF_PROMOTE_OK || !status.ready) {
        return;
    }
    if (status.at_recombine) {
        if (seen) {
            bridge.recombine_missing = 0;
            return;
        }
        if (++bridge.recombine_missing < RSF_RECOMBINE_ABSENT_FRAMES) {
            return;
        }
        bridge.recombine_missing = 0;
        bridge.recombine_off = 1;
        bridge.plan_stale = 1;
        say("reinsert: no recombine for %u frames, so the reconstruction goes in at the tonemap",
            RSF_RECOMBINE_ABSENT_FRAMES);
        return;
    }
    /* The composite route. The recombine rule matches here because nothing it reads is promoted. */
    if (bridge.color_selection.composed_this_frame && bridge.color_selection.composed) {
        bridge.recombine_off = 0;
        bridge.recombine_missing = 0;
        bridge.plan_stale = 1;
        say("reinsert: the recombine is back, so the reconstruction goes in there and separate "
            "translucency stays out of it");
    }
}

/* Detect absent composite gate activity, disarm substitutions, release stale composite/chain
   ownership, and restart bounded tail discovery while retaining recent UI identities. */
static void watch_for_stalled_plan(void)
{
    rsf_frame_tap_status tap;

    memset(&tap, 0, sizeof(tap));
    tap.struct_size = sizeof(tap);
    if (rsf_frame_tap_get_status(&tap) != RSF_FRAME_TAP_OK) {
        return;
    }
    /* Only composite gate activity proves this plan's composite is still in use; unrelated
       UI redirects must not keep a stale composite alive. */
    if (tap.gates_opened != bridge.redirects_seen) {
        /* The install recorded the count, so any movement is a gate under this plan. */
        bridge.plan_proven = 1;
        bridge.redirects_seen = tap.gates_opened;
        bridge.redirect_stall = 0;
        return;
    }
    {
        const unsigned long patience =
            bridge.plan_proven ? RSF_REINSERT_STALL_FRAMES : RSF_REINSERT_UNPROVEN_STALL_FRAMES;
        if (bridge.tail_restaking || ++bridge.redirect_stall < patience) {
            return;
        }
        ++bridge.tail_restakes;
        bridge.redirect_stall = 0;
        bridge.tail_restaking = 1;
        /* Limit repeated rediscovery logs on loading screens. */
        if (bridge.tail_restakes <= 5 || bridge.tail_restakes % 20 == 0) {
            say("reinsert: the composite has not been bound for %lu frames, so the plan no longer "
                "names the frame's composite. Looking for the tail again, restake %lu",
                patience, bridge.tail_restakes);
        }
    }
    /* Let go of what the plan named before looking, so a stale composite cannot be re-found by
       being the thing already held. The tail walk re-identifies both from the frame itself. */
    rsf_frame_tap_set_plan(NULL);
    bridge.size_patch_target = NULL;
    rsf_resource_release(bridge.composite);
    bridge.composite = NULL;
    bridge.composite_view_format = 0;
    /* Clear the dependent chain together with its stale composite; UI targets age separately. */
    clear_chain();
    bridge.tail_frames = 0;
    bridge.tail_draws = 0;
}

/* Advance bounded backbuffer/composite watches. Acquire each flip-buffer identity afresh
   and release backbuffer ownership after the short discovery budget. */
static void watch_tail(void* swapchain)
{
    void* buffer;

    ++bridge.tail_frames;
    if (bridge.tail_frames > RSF_TAIL_STOP_FRAMES) {
        return;
    }
    if (bridge.tail_frames == RSF_TAIL_STOP_FRAMES) {
        /* The watch stops; the references do not. The composite and the chain are what the plan
           names, and an engine pooled target is not made harder to reuse by one more reference the
           way a swap chain buffer is. */
        rsf_frame_tap_watch_target(1, NULL, 0);
        settle_chain_candidates();
        say("frame tail: done looking, %lu draws described, composite %s, %u chain target%s, %u "
            "interface layer%s",
            bridge.tail_draws, bridge.composite ? "found" : "not found",
            (unsigned)bridge.chain_target_count, bridge.chain_target_count == 1 ? "" : "s",
            (unsigned)bridge.ui_target_count, bridge.ui_target_count == 1 ? "" : "s");
        bridge.chain_rescan_countdown = RSF_CHAIN_RESCAN_FRAMES;
        /* Finish rediscovery with the currently observed tail. Later UI/chain arrivals mark
           it stale and rebuild the plan, so incomplete loading/video tails need not loop here. */
        if (bridge.tail_restaking) {
            bridge.tail_restaking = 0;
            install_reinsert_plan();
        }
        return;
    }
    if (bridge.tail_frames >= RSF_TAIL_ARM_FRAMES) {
        if (bridge.back_buffer) {
            rsf_frame_tap_watch_target(0, NULL, 0);
            rsf_resource_release(bridge.back_buffer);
            bridge.back_buffer = NULL;
        }
        return;
    }

    /* Re-read rather than kept. A flip model swap chain hands out a different texture per frame,
       and a watch left on the previous one would match nothing while looking exactly like a tail
       that has no draws in it. */
    buffer = rsf_swapchain_back_buffer(swapchain);
    if (!buffer) {
        return;
    }
    if (buffer == bridge.back_buffer) {
        rsf_resource_release(buffer);
        return;
    }
    rsf_resource_release(bridge.back_buffer);
    bridge.back_buffer = buffer;
    rsf_frame_tap_watch_target(0, buffer, RSF_TAIL_BACK_BUFFER_DRAWS);
}

/* Run the frame that was held, now that the game has finished drawing it. */
static void evaluate_held(void* context)
{
    rsf_dlss_pipeline_frame frame;
    rsf_dlss_pipeline_result result;

    if (!bridge.have_held) {
        return;
    }

    /* The scene colour and depth the qualifying pass bound. Separate translucency is not the
       reconstruction's business any more: it is rendered at full size and composited over the
       reconstruction by the game's own recombine, so it neither goes through DLSS nor needs a
       depth of its own there. */
    memset(&frame, 0, sizeof(frame));
    frame.struct_size = sizeof(frame);
    frame.abi_version = RSF_DLSS_PIPELINE_ABI_VERSION;
    frame.scene_color = bridge.held_color;
    frame.depth = bridge.held_depth;
    frame.game_motion = bridge.held_motion;
    frame.exposure = bridge.held_exposure;
    frame.render_width = (uint32_t)bridge.held_width;
    frame.render_height = (uint32_t)bridge.held_height;
    frame.camera = &bridge.held_camera;

    result = rsf_dlss_pipeline_on_frame(context, &frame);
    bridge.last_result = (long)result;
    if (result == RSF_DLSS_PIPELINE_OK) {
        ++bridge.evaluated;
    } else {
        ++bridge.refused;
    }
    release_held();
}


/* Evaluate at the recombine: scene colour is finished and translucency is not in it yet.

   The qualifying pass comes later in the frame, so this takes the textures the last pass named and
   this frame's camera from the main view's upload. Returns whether an evaluation happened. */
static int evaluate_at_recombine(void* context)
{
    rsf_dlss_pipeline_frame frame;
    rsf_dlss_pipeline_result result;
    if (!bridge.upload_camera_valid || !bridge.scene_color || !bridge.last_depth ||
        !bridge.last_motion || bridge.upload_width == 0 || bridge.upload_height == 0) {
        ++bridge.refused;
        return 0;
    }
    memset(&frame, 0, sizeof(frame));
    frame.struct_size = sizeof(frame);
    frame.abi_version = RSF_DLSS_PIPELINE_ABI_VERSION;
    frame.scene_color = bridge.scene_color;
    frame.depth = bridge.last_depth;
    frame.game_motion = bridge.last_motion;
    frame.exposure = bridge.last_exposure;
    frame.render_width = (uint32_t)bridge.upload_width;
    frame.render_height = (uint32_t)bridge.upload_height;
    frame.camera = &bridge.upload_camera;
    result = rsf_dlss_pipeline_on_frame(context, &frame);
    bridge.last_result = (long)result;
    if (result != RSF_DLSS_PIPELINE_OK) {
        ++bridge.refused;
        return 0;
    }
    ++bridge.evaluated;
    return 1;
}

/* Compatibility continuity heuristic: FOV difference < 0.02 radians, forward-vector
   dot > cos(5 degrees), and position difference < 2000 engine units. This does not establish
   source-frame identity or handle arbitrary cuts. */
static int camera_continues(const rsf_pipeline_camera_frame* candidate, const rsf_pipeline_camera_frame* reference)
{
    float dot = 0.0f;
    float distance = 0.0f;
    int axis;
    for (axis = 0; axis < 3; ++axis) {
        const float delta = candidate->camera_position[axis] - reference->camera_position[axis];
        dot += candidate->camera_forward[axis] * reference->camera_forward[axis];
        distance += delta * delta;
    }
    return fabsf(candidate->vertical_fov - reference->vertical_fov) < 0.02f &&
           dot > 0.9962f /* cos 5 degrees */ && distance < 2000.0f * 2000.0f;
}

static void gate_trace(const char* what, void* texture)
{
    if (bridge.gate_trace_left == 0) {
        return;
    }
    say("gate trace: %s %p, present %lu", what, texture, bridge.presents);
}

static void log_gate_decision(const char* verdict)
{
    if (bridge.gate_decisions_logged >= 12) {
        return;
    }
    ++bridge.gate_decisions_logged;
    say("recombine gate %s: camera at %.0f %.0f %.0f looking %.2f %.2f %.2f fov %.3f; the main "
        "view's %s at %.0f %.0f %.0f looking %.2f %.2f %.2f fov %.3f",
        verdict, bridge.upload_camera.camera_position[0], bridge.upload_camera.camera_position[1],
        bridge.upload_camera.camera_position[2], bridge.upload_camera.camera_forward[0],
        bridge.upload_camera.camera_forward[1], bridge.upload_camera.camera_forward[2],
        bridge.upload_camera.vertical_fov, bridge.main_camera_ref_valid ? "was" : "is unknown, so",
        bridge.main_camera_ref.camera_position[0], bridge.main_camera_ref.camera_position[1],
        bridge.main_camera_ref.camera_position[2], bridge.main_camera_ref.camera_forward[0],
        bridge.main_camera_ref.camera_forward[1], bridge.main_camera_ref.camera_forward[2],
        bridge.main_camera_ref.vertical_fov);
}

/* CopyResource through the game's context, for two textures of one size and format. */
static void rsf_d3d11_copy_texture(void* context, void* destination, void* source)
{
    if (context && destination && source) {
        rsf_d3d11_copy_resource(context, destination, source);
    }
}

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

/* Integrate the layer for this frame: the second DLSS feature, one to one, fed the layer, the
   replayed depth for it and the layer's own view. On any refusal the raw layer is copied into the
   output instead, so the recombine never reads a stale integration. */
static void integrate_layer(void* context)
{
    rsf_dlss_pipeline_layer layer;
    rsf_depth_replay_detail detail;
    void* depth = NULL;
    int ok = 0;
    if (bridge.layer_unjitter || !bridge.layer_output || !bridge.translucency_layer) {
        return;
    }
    if (bridge.translucency_layer_seen_at != bridge.presents) {
        return;
    }
    if (bridge.layer_replay) {
        memset(&detail, 0, sizeof(detail));
        rsf_depth_replay_get_detail(bridge.layer_replay, &detail);
        if (detail.source) {
            void* selected = rsf_depth_replay_selected(bridge.layer_replay, context, detail.source,
                                                       bridge.translucency_layer);
            depth = selected != detail.source ? selected : NULL;
        }
    }
    if (depth && bridge.layer_camera_valid) {
        memset(&layer, 0, sizeof(layer));
        layer.struct_size = sizeof(layer);
        layer.abi_version = RSF_DLSS_PIPELINE_ABI_VERSION;
        layer.color = bridge.translucency_layer;
        layer.depth = depth;
        layer.width = (uint32_t)bridge.layer_replay_width;
        layer.height = (uint32_t)bridge.layer_replay_height;
        layer.camera = &bridge.layer_camera;
        ok = rsf_dlss_pipeline_on_layer(context, &layer) == RSF_DLSS_PIPELINE_OK;
    } else if (!depth) {
        ++bridge.layer_depth_missing;
    }
    if (ok) {
        ++bridge.layer_integrations;
    } else {
        ++bridge.layer_integrations_refused;
        rsf_d3d11_copy_texture(context, bridge.layer_output, bridge.translucency_layer);
    }
}

/* Run compatibility reconstruction before downstream consumers and restore the immediate
   context state. Return zero only to decline a secondary recombine camera and retry the gate;
   evaluation refusal seeds the replacement with current game colour rather than stale output. */
static int on_gate(void* user, void* context, void* texture)
{
    rsf_d3d11_state state;
    rsf_promote_status status;

    (void)user;
    (void)texture;
    if (!context) {
        return 1;
    }
    memset(&status, 0, sizeof(status));
    status.struct_size = sizeof(status);
    if (bridge.promote && rsf_promote_get_status(bridge.promote, &status) == RSF_PROMOTE_OK &&
        status.at_recombine) {
        gate_trace(texture && texture == bridge.composite ? "composite bound" : "recombined bound",
                   texture);
        if (texture && texture == bridge.composite) {
            /* The tonemap's gate: temporal AA wrote the scratch target, and scene colour gets the
               recombined result instead. The pass just before this is the main view's temporal
               pass, so its camera is next frame's reference. */
            if (!rsf_d3d11_state_save(context, &state)) {
                return 1;
            }
            rsf_promote_finish(bridge.promote, context);
            if (bridge.route_dump_frames) {
                void* scene_stand_in = NULL;
                void* composed_stand_in = NULL;
                rsf_promote_get_stand_ins(bridge.promote, &scene_stand_in, &composed_stand_in);
                route_dump(context, composed_stand_in, "recombined");
                say("route dump: frame %lu camera jitter %.3f %.3f px", bridge.route_dump_frame,
                    bridge.upload_camera.jitter_pixels[0], bridge.upload_camera.jitter_pixels[1]);
            }
            rsf_d3d11_state_restore(context, &state);
            ++bridge.finishes;
            if (bridge.have_held) {
                bridge.main_camera_ref = bridge.held_camera;
                bridge.main_camera_ref_valid = 1;
            }
            return 1;
        }
        /* The recombine's gate. Only the main view's, judged by its camera continuing the
           reference; any other render's is declined and the gate asked again at the next. Without
           a reference, the first is taken and the composite gate establishes one. */
        if (bridge.main_camera_ref_valid && bridge.upload_camera_valid &&
            !camera_continues(&bridge.upload_camera, &bridge.main_camera_ref)) {
            ++bridge.gates_declined_camera;
            log_gate_decision("declined");
            gate_trace("recombine declined", texture);
            return 0;
        }
        log_gate_decision("taken");
        gate_trace("recombine taken", texture);
        if (!rsf_d3d11_state_save(context, &state)) {
            return 1;
        }
        bridge.recombine_gate_this_frame = 1;
        /* Seeded every time: with the reconstruction, or with the game's own scene colour when
           the evaluation was refused, so the stand-in never shows a stale frame. */
        rsf_promote_seed(bridge.promote, context, evaluate_at_recombine(context) ? 1u : 0u);
        integrate_layer(context);
        if (bridge.route_dump_frames) {
            void* scene_stand_in = NULL;
            void* composed_stand_in = NULL;
            rsf_promote_get_stand_ins(bridge.promote, &scene_stand_in, &composed_stand_in);
            route_dump(context, bridge.scene_color, "scene");
            route_dump(context, scene_stand_in, "seed");
            if (bridge.translucency_layer_seen_at == bridge.presents) {
                route_dump(context, bridge.translucency_layer, "layer");
            } else {
                say("route dump: separate translucency not produced this frame; cached layer skipped");
            }
            route_dump(context, bridge.layer_output, "layer_integrated");
        }
        rsf_d3d11_state_restore(context, &state);
        ++bridge.gate_evaluates;
        return 1;
    }
    if (!bridge.have_held) {
        return 1;
    }
    if (!rsf_d3d11_state_save(context, &state)) {
        return 1;
    }
    evaluate_held(context);
    rsf_d3d11_state_restore(context, &state);
    ++bridge.gate_evaluates;
    return 1;
}

/* UI extraction transport: the layer is linear UNORM and the backbuffer is display
 * encoded. Select no transform, sRGB or gamma 2.2 when compositing; API format alone does
 * not establish the game's intended display curve. */
static rsf_fullscreen_mode ui_composite_mode = RSF_FULLSCREEN_PREMULTIPLIED_SRGB;

/* Modules that log take a sink and a user pointer; this bridge's log is a single global. */
static void bridge_layer_log(void* user, const char* message)
{
    (void)user;
    say("%s", message);
}

/* Cached owned RTV for UI extraction, recreated when GetBuffer returns a new identity.
 * The RTV retains its resource and therefore can also block ResizeBuffers; releasing the
 * temporary GetBuffer reference does not remove that dependency. */
static void* back_buffer_view(void* swapchain)
{
    static void* cached_for = NULL;
    static void* cached = NULL;
    static int complained = 0;
    void* buffer;

    if (!swapchain || !bridge.device) {
        return NULL;
    }
    /* Acquire the current swapchain buffer rather than the bounded tail-watch pointer.
     * Release its temporary reference after constructing the cached view. */
    buffer = rsf_swapchain_back_buffer(swapchain);
    if (!buffer) {
        if (!complained) {
            complained = 1;
            say("ui extract: the swap chain gave no back buffer, so nothing can be composited");
        }
        return NULL;
    }
    if (buffer != cached_for) {
        rsf_resource_release(cached);
        cached = rsf_create_render_target_view(bridge.device, buffer);
        cached_for = buffer;
        if (!cached && !complained) {
            complained = 1;
            say("ui extract: no view onto the back buffer, so nothing can be composited onto it");
        }
    }
    rsf_resource_release(buffer);
    return cached;
}


/* Compatibility uniform-buffer twins remove jitter from every verified AC7 view field
   before UI/layer draws. A mapped WRITE_DISCARD upload fills the twin before draw overrides
   use it; pooled buffers refilled with non-view data lose their twin. */
static rsf_constant_twins* view_twins;
static unsigned long twins_written, twins_bound;
static unsigned char twin_scratch[RSF_AC7_VIEW_BUFFER_BYTES];

/* Rewrite (W, H, 1/W, 1/H) at the render size into the same at the output size, in an upload for a
   draw the plan runs at output size. Unreal packs every post process input's size that way
   (RenderingCompositionGraph.cpp:973), and the pixel-to-UV factor a shader derives from it is what
   has to follow the viewport. */
static void patch_input_sizes(void* contents, uint32_t bytes)
{
    float* values = (float*)contents;
    const float width = (float)bridge.held_width;
    const float height = (float)bridge.held_height;
    const float output_width = (float)bridge.output_width;
    const float output_height = (float)bridge.output_height;
    uint32_t index;
    unsigned long patched = 0;
    if (width <= 0.0f || height <= 0.0f || output_width <= 0.0f || output_height <= 0.0f) {
        return;
    }
    for (index = 0; index + 4 <= bytes / 4; index += 4) {
        if (values[index] == width && values[index + 1] == height &&
            fabsf(values[index + 2] * width - 1.0f) < 1e-3f &&
            fabsf(values[index + 3] * height - 1.0f) < 1e-3f) {
            values[index] = output_width;
            values[index + 1] = output_height;
            values[index + 2] = 1.0f / output_width;
            values[index + 3] = 1.0f / output_height;
            ++patched;
        }
    }
    if (patched) {
        ++bridge.size_uploads_patched;
        bridge.sizes_patched += patched;
    }
}

static void on_view_constants(void* user, void* buffer, const void* contents, uint32_t bytes);

/* Every constant buffer the game fills. The view's buffer is read for the camera and twinned
   without its jitter; an upload for a draw into the recombined target has its sizes promoted. */
static void on_constants(void* user, void* buffer, void* contents, uint32_t bytes)
{
    if (InterlockedCompareExchange(&native_owner, 0, 0)) {
        rsf_ac7_motion_capture_upload(buffer, contents, bytes); return;
    }
    if (bridge.size_patch_target && rsf_frame_tap_bound_target() == bridge.size_patch_target) {
        patch_input_sizes(contents, bytes);
    }
    if (bytes == RSF_AC7_VIEW_BUFFER_BYTES) {
        on_view_constants(user, buffer, contents, bytes);
    } else if (view_twins) {
        rsf_constant_twins_forget(view_twins, buffer);
    }
    rsf_ac7_motion_capture_upload(buffer, contents, bytes);
}

static void on_view_constants(void* user, void* buffer, const void* contents, uint32_t bytes)
{
    if (InterlockedCompareExchange(&native_owner, 0, 0)) return;
    (void)user;
    if (bytes != RSF_AC7_VIEW_BUFFER_BYTES) {
        return;
    }
    {
        rsf_ac7_view main_view;
        memset(&main_view, 0, sizeof(main_view));
        main_view.struct_size = sizeof(main_view);
        if (rsf_ac7_view_read(contents, bytes, RSF_AC7_VIEW_ABI_VERSION, &main_view) ==
                RSF_AC7_VIEW_OK &&
            main_view.is_main_view && main_view.has_jitter &&
            bridge.depth_candidate_width != 0 &&
            main_view.view_width == bridge.depth_candidate_width &&
            main_view.view_height == bridge.depth_candidate_height) {
            /* The separate translucency view: the scene's camera at the layer's size, which is
               what the layer's integration needs, jitter in the layer's own pixels. */
            fill_camera(&main_view, &bridge.layer_camera);
            bridge.layer_camera_valid = 1;
        }
        if (rsf_ac7_view_read(contents, bytes, RSF_AC7_VIEW_ABI_VERSION, &main_view) ==
                RSF_AC7_VIEW_OK &&
            main_view.is_main_view && main_view.has_jitter &&
            /* The render size the qualifying pass reported. UE builds the separate translucency
               view from the main one at the layer's size, 1600x904 in a 1600x900 briefing, and
               that upload is main and jittered too; handed to DLSS as the render size it made
               every evaluation fail. */
            /* The pass reports the pooled motion target's size, which the pool rounds up by a
               few rows, so the view has to fit inside it rather than equal it. */
            (bridge.held_width == 0 ||
             (main_view.view_width <= bridge.held_width &&
              main_view.view_height <= bridge.held_height))) {
            fill_camera(&main_view, &bridge.upload_camera);
            bridge.upload_width = main_view.view_width;
            bridge.upload_height = main_view.view_height;
            bridge.upload_camera_valid = 1;
        }
    }
    if (!view_twins) {
        return;
    }
    /* Only a real view buffer with a jitter gets a twin: the reader's checks refuse anything else
       the pool puts in a 4096-byte buffer, and a pooled buffer refilled with something else loses
       its twin here rather than keeping a stale one. */
    {
        rsf_ac7_view view;
        memset(&view, 0, sizeof(view));
        view.struct_size = sizeof(view);
        if (rsf_ac7_view_read(contents, bytes, RSF_AC7_VIEW_ABI_VERSION, &view) !=
                RSF_AC7_VIEW_OK ||
            !rsf_ac7_view_remove_jitter(contents, twin_scratch, bytes)) {
            rsf_constant_twins_forget(view_twins, buffer);
            return;
        }
    }
    if (rsf_constant_twins_write(view_twins, bridge.context, buffer, twin_scratch)) {
        ++twins_written;
    }
}

/* Read the buffers the actual draw binds, not an unrelated upload of the same size. Bounded to
   two captured frames and 128 draws, explicitly opted in through the diagnostic prefix. */
static void capture_layer_constants(const rsf_frame_tap_target_draw* draw)
{
    unsigned char data[RSF_AC7_VIEW_BUFFER_BYTES];
    uint32_t stage, slot;
    const unsigned long ordinal = bridge.briefing_capture_draws++;
    say("briefing capture: frame %lu draw %lu target %p %ux%u elements %u topology %u VS %08lx PS %08lx",
        bridge.route_dump_frame, ordinal, draw->render_target, draw->target_width, draw->target_height,
        draw->element_count, draw->topology, ui_hash_of(draw->vertex_shader), ui_hash_of(draw->pixel_shader));
    const char* names[] = {"vs", "ps", "gs", "hs", "ds"};
    void* const* bound[] = {draw->vertex_constants, draw->pixel_constants,
                           draw->geometry_constants, draw->hull_constants, draw->domain_constants};
    for (stage = 0; stage < 5; ++stage) {
        for (slot = 0; slot < 14; ++slot) {
            void* original = bound[stage][slot];
            void* twin = rsf_constant_twins_find(view_twins, original);
            unsigned variant;
            if (!original) {
                continue;
            }
            for (variant = 0; variant < 2; ++variant) {
                void* buffer = variant ? twin : original;
                if (buffer && rsf_read_constant_buffer(bridge.device, bridge.context, buffer,
                                                       data, sizeof(data)) == RSF_CONSTANT_BUFFER_OK) {
                    char path[640];
                    FILE* file;
                    rsf_ac7_view view;
                    rsf_ac7_view_result result;
                    memset(&view, 0, sizeof(view));
                    view.struct_size = sizeof(view);
                    result = rsf_ac7_view_read(data, sizeof(data), RSF_AC7_VIEW_ABI_VERSION, &view);
                    snprintf(path, sizeof(path), "%s_f%lu_d%lu_%s_b%u_%s.bin",
                             bridge.briefing_capture_prefix, bridge.route_dump_frame, ordinal,
                             names[stage], (unsigned)slot, variant ? "twin" : "original");
                    file = fopen(path, "wb");
                    if (file) {
                        fwrite(data, 1, sizeof(data), file);
                        fclose(file);
                    }
                    say("briefing buffer: draw %lu %s b%u %s %p parse %d view %ux%u jitter %.6f %.6f",
                        ordinal, names[stage], (unsigned)slot, variant ? "twin" : "original",
                        buffer, (int)result, view.view_width, view.view_height,
                        view.jitter_pixels[0], view.jitter_pixels[1]);
                }
            }
        }
    }
}

/* Return flattened graphics-stage/slot overrides for twinned view buffers. UI uses VS;
   unjittered translucency also replaces PS/GS/HS/DS slots. Returned buffers belong to view_twins. */
static int ui_constant_override(void* user, const rsf_frame_tap_target_draw* draw, uint32_t* slots,
                                void** buffers)
{
    uint32_t index;
    int count = 0;
    int layer_draw;
    (void)user;
    if (!draw || !view_twins) {
        return 0;
    }
    layer_draw = bridge.layer_unjitter && bridge.reinsert_on && !bridge.recombine_off &&
        bridge.color_selection.composed && rsf_ac7_separate_translucency_draw(draw);
    if (!layer_draw && (!bridge.ui_unjitter ||
                       classify_candidate(draw) != RSF_AC7_DRAW_UI_WIDGET_QUAD)) {
        return 0;
    }
    if (layer_draw && bridge.briefing_capture_prefix[0] && bridge.route_dump_frames &&
        bridge.briefing_capture_draws < 128) {
        capture_layer_constants(draw);
    }
    /* Every slot with a twin, not the first. D3D11 keeps earlier draws' buffers bound in the slots
       a shader does not use, so the first twinned buffer found may be a stale one while the view
       this shader reads sits in a later slot, jitter and all. */
    for (index = 0; index < 14; ++index) {
        void* twin = rsf_constant_twins_find(view_twins, draw->vertex_constants[index]);
        if (twin) {
            slots[count] = index;
            buffers[count] = twin;
            ++count;
        }
    }
    /* Terrain is a 12-control-point patch: its domain shader performs the final projection.
       Every graphics stage must receive the same unjittered view. UI retains its VS-only path. */
    if (layer_draw) {
        void* const* bound[] = {draw->pixel_constants, draw->geometry_constants,
                               draw->hull_constants, draw->domain_constants};
        uint32_t stage;
        for (stage = 0; stage < 4; ++stage) {
            for (index = 0; index < 14; ++index) {
                void* twin = rsf_constant_twins_find(view_twins, bound[stage][index]);
                if (twin) {
                    slots[count] = 14 * (stage + 1) + index;
                    buffers[count] = twin;
                    ++count;
                }
            }
        }
    }
    if (count) {
        ++twins_bound;
        if (layer_draw) {
            ++bridge.layer_draws_twinned;
        }
    } else if (layer_draw) {
        ++bridge.layer_draws_untwinned;
    }
    return count;
}

/* Divert classified widget quads only and request corrected alpha accumulation. Scene,
   converter, final Slate, unknown and modulate draws retain their existing targets. */
static rsf_frame_tap_verdict ui_verdict(void* user, const rsf_frame_tap_target_draw* draw)
{
    rsf_ac7_draw_class verdict;
    (void)user;
    if (!draw) {
        return RSF_FRAME_TAP_LEAVE;
    }
    verdict = classify_candidate(draw);
    if (verdict != RSF_AC7_DRAW_UI_WIDGET_QUAD) {
        return RSF_FRAME_TAP_LEAVE;
    }
    /* Mark the layer as potentially written before requesting diversion; tap status reports
       actual diversion/refusal counts separately. */
    rsf_ui_layer_mark_written(bridge.layer);
    /* The quads are drawn with the base pass translucent blend, whose alpha factors leave a
       transparent layer at zero coverage however much colour lands on it. Measured under DXVK in
       tests/ui_layer.cpp rather than taken from the engine source. */
    return RSF_FRAME_TAP_DIVERT_PATCH_ALPHA;
}

/* Bring up the layer and the compositor, once the device and the presented size are known. */
static int start_extraction(unsigned long width, unsigned long height)
{
    rsf_ui_layer_setup layer;
    rsf_fullscreen_setup pass;

    if (bridge.layer || bridge.ui_extract_failed) {
        return bridge.layer != NULL;
    }
    memset(&layer, 0, sizeof(layer));
    layer.struct_size = sizeof(layer);
    layer.abi_version = RSF_UI_LAYER_ABI_VERSION;
    layer.width = (uint32_t)width;
    layer.height = (uint32_t)height;
    /* Shareable from the start: frame generation opens this on a D3D12 device later and the flag
       cannot be added without recreating the texture. */
    layer.shareable = 1;
    /* Use linear UNORM to match widget-quad destination views; display encoding belongs
     * to the final composite. Evidence is in the captured typed view formats. */
    layer.srgb = 0;
    layer.log = bridge_layer_log;
    if (rsf_ui_layer_create(bridge.device, &layer, &bridge.layer) != RSF_UI_LAYER_OK) {
        say("ui extract: the layer could not be created; the interface stays in the scene");
        bridge.ui_extract_failed = 1;
        return 0;
    }

    memset(&pass, 0, sizeof(pass));
    pass.struct_size = sizeof(pass);
    pass.abi_version = RSF_FULLSCREEN_PASS_ABI_VERSION;
    pass.log = bridge_layer_log;
    if (rsf_fullscreen_pass_create(bridge.device, &pass, &bridge.composite_pass) !=
        RSF_FULLSCREEN_OK) {
        /* Roll back layer creation if composition resources fail, so diversion cannot hide the UI. */
        say("ui extract: the compositor could not be created; the interface stays in the scene");
        rsf_ui_layer_destroy(bridge.layer);
        bridge.layer = NULL;
        bridge.ui_extract_failed = 1;
        return 0;
    }
    say("ui extract: layer and compositor ready at %lux%lu", width, height);
    return 1;
}

/* Put the interface back over the finished frame.
 *
 * Runs inside the present hook, after the game has finished drawing and before anything is shown,
 * which is the one moment the frame exists complete and unseen. Skipped when nothing was diverted,
 * so a frame with no interface on it costs nothing. */
static void composite_ui(void* swapchain)
{
    rsf_ui_layer_status status;
    rsf_fullscreen_draw parameters;
    void* source;
    void* target_view;

    if (!bridge.layer || !bridge.composite_pass) {
        return;
    }
    memset(&status, 0, sizeof(status));
    status.struct_size = sizeof(status);
    if (rsf_ui_layer_get_status(bridge.layer, &status) != RSF_UI_LAYER_OK) {
        return;
    }
    if (status.written_this_frame) {
        source = rsf_ui_layer_source(bridge.layer);
        target_view = back_buffer_view(swapchain);
        if (source && target_view) {
            memset(&parameters, 0, sizeof(parameters));
            parameters.struct_size = sizeof(parameters);
            parameters.mode = ui_composite_mode;
            if (rsf_fullscreen_pass_draw(bridge.composite_pass, bridge.context, target_view, source,
                                         &parameters) == RSF_FULLSCREEN_OK) {
                ++bridge.ui_composites;
            }
        }
    }
    /* The next frame's slot, cleared here rather than after the composite, so a frame that presents
       twice keeps a stale layer rather than losing the interface entirely. */
    rsf_ui_layer_begin_frame(bridge.layer, bridge.context);

    /* And point the divert at it. The layer alternates slots so that a vendor holding the previous
       one is not reading the one being drawn, which means the target the tap writes into changes
       every frame and arming it once would send every frame after the first into the slot being
       composited from. */
    if (bridge.ui_extract) {
        rsf_frame_tap_divert_setup divert;
        memset(&divert, 0, sizeof(divert));
        divert.struct_size = sizeof(divert);
        divert.layer_target = rsf_ui_layer_target(bridge.layer);
        divert.layer_width = status.width;
        divert.layer_height = status.height;
        divert.verdict = ui_verdict;
        rsf_frame_tap_set_divert(&divert);
    }
}

/* Diagnostic blit of linear/ungraded reconstruction over the completed frame. It omits
   tonemapping and UI, so it is useful for temporal inspection rather than final-image comparison. */
static void show_result(void* swapchain)
{
    void* output;

    if (!bridge.started || !bridge.show || !bridge.blit) {
        return;
    }
    output = rsf_dlss_pipeline_output_texture();
    if (!output || bridge.evaluated == 0) {
        return;
    }
    if (rsf_present_blit_draw(bridge.blit, bridge.context, swapchain, output, 1u) ==
        RSF_PRESENT_BLIT_OK) {
        ++bridge.frames_shown;
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

/* Build one UI snapshot from current pipeline, tap, native FG and carrier state. Report
   requested/effective provider state separately; SDK/DXGI totals do not measure scanout or latency. */
static void fill_overlay_stats(rsf_overlay_stats* stats)
{
    rsf_dlss_pipeline_status pipeline;
    rsf_frame_tap_status tap;

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
    stats->backend = pipeline.backend;
    stats->requested_backend = pipeline.requested_backend;
    stats->last_switch_result = pipeline.last_switch_result;
    stats->backend_name = pipeline.backend == 2 ? "FSR2" : pipeline.backend == 3 ? "FSR3" :
        pipeline.backend == 4 ? "FSR4" : pipeline.backend == 5 ? "XeSS" : "DLSS";

    /* Choose a user-facing refusal from the earliest unmet startup/evaluation stage. */
    if (!bridge.started) {
        stats->refusal_reason = bridge.startup_attempted ? "DLSS could not start; see the log" : "Waiting for the renderer";
    } else if (!stats->backend_supported) {
        stats->refusal_reason = "the driver did not accept DLSS";
    } else if (pipeline.requested_backend == RSF_SR_FSR4 &&
               pipeline.last_switch_result == RSF_BACKEND_ERROR_NOT_SUPPORTED) {
        stats->refusal_reason = "FSR4 unavailable for this GPU/runtime; previous backend remains active";
    } else if (bridge.passes == 0) {
        stats->refusal_reason = "no pass has bound the reconstruction inputs yet";
    } else if (bridge.no_jitter > 0 && bridge.evaluated == 0) {
        stats->refusal_reason = "Waiting for valid temporal inputs";
    } else if (bridge.not_main_view > 0 && bridge.evaluated == 0) {
        stats->refusal_reason = "the view read was not the main view";
    }

    memset(&tap, 0, sizeof(tap));
    tap.struct_size = sizeof(tap);
    if (rsf_frame_tap_get_status(&tap) == RSF_FRAME_TAP_OK) {
        stats->have_motion = tap.motion_seen > 0;
        stats->have_depth = tap.depth_seen > 0;
        stats->have_exposure = tap.exposure_seen > 0;
        if (stats->render_width == 0) {
            stats->render_width = tap.render_width;
            stats->render_height = tap.render_height;
        }
    }
    stats->have_scene_color = bridge.scene_color != NULL || bridge.held_color != NULL;
    if (rsf_bridge_native_owned()) {
        stats->have_scene_color = (bridge.native_input_mask & 1u) != 0;
        stats->have_depth = (bridge.native_input_mask & 2u) != 0;
        stats->have_motion = (bridge.native_input_mask & 4u) != 0;
        stats->have_exposure = (bridge.native_input_mask & 8u) != 0;
    }
    stats->motion_decoded = bridge.evaluated > 0;
    stats->jitter_active = bridge.held_camera.has_jitter;
    stats->jitter_pixels[0] = bridge.held_camera.jitter_pixels[0];
    stats->jitter_pixels[1] = bridge.held_camera.jitter_pixels[1];
    stats->frames_presented = bridge.frames_shown;
    {
        const rsf_fg_choice choice = rsf_fg_choice_get();
        stats->fg_backend = rsf_d3d11_present_has_owner() ? rsf_d3d11_present_backend() : 0;
        stats->fg_requested_backend = choice.backend; stats->fg_backend_choices = choice.choices | RSF_OVERLAY_FG_RUNTIME_SWITCH;
        stats->fg_selection_result = choice.last_result;
        if (!stats->fg_selection_result) stats->fg_selection_result = rsf_d3d11_present_switch_result();
    }
    {
        rsf_native_fg_status fg = {0}; fg.struct_size = sizeof(fg);
        if (rsf_native_fg_status_get(&fg)) {
            stats->fg_available = fg.available;
            stats->fg_requested_mode = fg.requested.mode;
            stats->fg_requested_generated = fg.requested.generated_frames;
            stats->fg_effective_mode = fg.vendor.effective_mode;
            stats->fg_effective_generated = fg.vendor.effective_generated_frames;
            stats->fg_active = fg.vendor.active;
            stats->fg_max_generated = fg.vendor.max_generated_frames;
            stats->reflex_available = fg.vendor.low_latency_available;
            stats->reflex_requested_mode = fg.requested.reflex_mode;
            stats->reflex_effective_mode = fg.vendor.effective_reflex;
            stats->fg_reason = fg.reason; stats->fg_last_result = fg.last_result;
            stats->fg_total_presented = fg.vendor.total_presented;
            stats->fg_present_count_valid = fg.available && (fg.vendor.valid_statistics & RSF_FG_STAT_TOTAL_PRESENTED) != 0;
            stats->frame_limit_us = fg.requested.frame_limit_us;
            stats->display_refresh_mhz = display_refresh_mhz();
        }
    }
    {
        LARGE_INTEGER now, frequency;
        QueryPerformanceCounter(&now); QueryPerformanceFrequency(&frequency);
        stats->application_presented_frames = bridge.application_presented_frames;
        stats->sample_qpc = (uint64_t)now.QuadPart; stats->qpc_frequency = (uint64_t)frequency.QuadPart;
        stats->show_performance_hud = performance_hud_enabled;
    }
    stats->enabled = InterlockedCompareExchange(&native_owner, 0, 0) ? (uint32_t)bridge.enabled_requested : (uint32_t)bridge.reinsert_on;
    stats->debug_view_on = (uint32_t)bridge.show;
    stats->reinsert_on = InterlockedCompareExchange(&native_owner, 0, 0) ? (uint32_t)bridge.enabled_requested : (uint32_t)bridge.reinsert_on;
    /* What rsf_bridge_toggle_reinsert would refuse on, asked before the click rather than after.
       The composite has to have been identified and a reconstruction has to exist. */
    stats->reinsert_available = InterlockedCompareExchange(&native_owner, 0, 0) ? (uint32_t)bridge.started :
        (uint32_t)(bridge.started && bridge.composite_found && bridge.scene_color != NULL &&
                   bridge.evaluated > 0);
    /* Log reinsertion prerequisites only when their availability changes. */
    {
        static unsigned last_reason = 0xffu;
        const unsigned reason = InterlockedCompareExchange(&native_owner, 0, 0) ? 0u : bridge.started ? 0u : 1u | (bridge.composite_found ? 0u : 2u) |
                                (bridge.scene_color != NULL ? 0u : 4u) |
                                (bridge.evaluated > 0 ? 0u : 8u);
        if (reason != last_reason) {
            last_reason = reason;
            if (reason == 0) {
                say("reinsert: available, the panel switch and F6 will take it");
            } else {
                say("reinsert: not available yet: %s%s%s%s",
                    (reason & 1u) ? "backend not started; " : "",
                    (reason & 2u) ? "composite not identified; " : "",
                    (reason & 4u) ? "no scene colour retained; " : "",
                    (reason & 8u) ? "nothing evaluated yet; " : "");
            }
        }
    }
    stats->quality = (rsf_overlay_quality)bridge.quality;
    if (rsf_bridge_native_owned()) {
        rsf_game_render_config config = {0}; config.struct_size = sizeof(config);
        stats->render_scale_percent = rsf_native_sr_render_config(NULL, &config) && config.output_width ?
            (uint32_t)(((uint64_t)config.render_width * 100u + config.output_width - 1u) / config.output_width) : 0u;
    } else {
        stats->render_scale_percent = bridge.actions.render_scale_percent ? (uint32_t)bridge.actions.render_scale_percent() : 0u;
    }
    stats->captures_written =
        bridge.actions.capture_count ? (uint32_t)bridge.actions.capture_count() : 0u;
    stats->jitter_on = rsf_bridge_native_owned() ? (uint32_t)bridge.enabled_requested :
        bridge.actions.jitter_open ? (uint32_t)bridge.actions.jitter_open() : 0u;
    stats->jitter_available =
        bridge.actions.jitter_available ? (uint32_t)bridge.actions.jitter_available() : 0u;
    if (bridge.setting_error) {
        stats->refusal_reason = bridge.setting_error;
    } else if (!bridge.enabled_requested) {
        stats->refusal_reason = NULL;
    } else if (bridge.started && !stats->reinsert_on && !stats->refusal_reason) {
        stats->refusal_reason = "Waiting for the scene's upscale path";
    }
}

/* Apply a preset on the graphics owner thread. Query backend dimensions, apply the engine
   render scale, roll back the preset on refusal, then wait for fresh evaluation before rebuilding
   compatibility reinsertion. Accepted choices are persisted through carrier callbacks. */
int rsf_bridge_select_quality(unsigned long quality)
{
    uint32_t width = 0, height = 0;
    if (quality > RSF_DLSS_QUALITY_ULTRA_PERFORMANCE) {
        return 0;
    }
    if (bridge.started) {
        if (rsf_bridge_native_owned() && rsf_dlss_pipeline_resize_output((uint32_t)bridge.output_width,
            (uint32_t)bridge.output_height, NULL, NULL) != RSF_DLSS_PIPELINE_OK) {
            bridge.setting_error = "The backend refused the engine output extent"; return 0;
        }
        const unsigned long previous = bridge.quality;
        if (rsf_dlss_pipeline_set_quality((rsf_dlss_quality)quality, &width, &height) !=
            RSF_DLSS_PIPELINE_OK) {
            bridge.setting_error = "DLSS did not accept this preset";
            return 0;
        }
        const unsigned long x = (width * 100ul + bridge.output_width - 1) / bridge.output_width;
        const unsigned long y = (height * 100ul + bridge.output_height - 1) / bridge.output_height;
        const unsigned long percent = x > y ? x : y;
        if (!rsf_bridge_native_owned() && bridge.enabled_requested && (!bridge.actions.set_render_scale ||
                                        !bridge.actions.set_render_scale(percent))) {
            rsf_dlss_pipeline_set_quality((rsf_dlss_quality)previous, NULL, NULL);
            bridge.setting_error = "The game did not accept the preset's render resolution";
            say("preset %lu refused: screen percentage did not change; keeping %lu", quality, previous);
            return 0;
        }
        if (bridge.reinsert_on) {
            stop_reinsert();
        }
        bridge.enable_after_evaluations = bridge.evaluated;
        bridge.enable_retry_after = bridge.presents + 2;
        say("preset %lu applied: requested %ux%u, output %lux%lu, screen percentage %lu",
            quality, width, height, bridge.output_width, bridge.output_height, percent);
    }
    bridge.quality = quality;
    if (bridge.started && rsf_bridge_native_owned()) rsf_native_sr_set_surface((uint32_t)bridge.output_width, (uint32_t)bridge.output_height);
    publish_native_settings();
    bridge.quality_chosen = 1;
    bridge.setting_error = NULL;
    if (bridge.actions.save_quality) {
        bridge.actions.save_quality(quality);
    }
    return 1;
}

/* Apply requested enablement on the render thread. Compatibility disable must restore
   native screen percentage before disarming; refusal preserves the working session. Backend
   objects remain available for later reenable/preset selection. */
void rsf_bridge_set_enabled(int enabled)
{
    if (!enabled) {
        /* Restore the game's resolution before disarming. A refused write leaves the working
           session intact and reports why the request could not be applied. */
        if (!rsf_bridge_native_owned() && bridge.started && bridge.actions.set_render_scale &&
            !bridge.actions.set_render_scale(100)) {
            bridge.setting_error = "Native resolution could not be restored";
            return;
        }
        stop_reinsert();
        bridge.show = 0;
        bridge.enabled_requested = 0;
        if (bridge.actions.set_jitter) {
            bridge.actions.set_jitter(0);
        }
    } else {
        bridge.enabled_requested = 1;
        bridge.startup_attempted = 0;
        if (bridge.started && !rsf_bridge_select_quality(bridge.quality)) {
            bridge.enabled_requested = 0;
            return;
        }
        if (bridge.started && bridge.actions.set_jitter) {
            bridge.actions.set_jitter(InterlockedCompareExchange(&native_owner, 0, 0) ? 0 : 1);
        }
    }
    publish_native_settings();
    bridge.setting_error = NULL;
    if (bridge.actions.save_enabled) {
        bridge.actions.save_enabled(enabled != 0);
    }
}

/* Draw the shared panel and apply its intents on the graphics owner thread. Provider
   switching requests drain through the presentation host; refused SR/scale changes restore the
   prior backend when possible and expose a status reason. */
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
    memset(&intent, 0, sizeof(intent));
    intent.struct_size = sizeof(intent);
    if (!rsf_overlay_host_present(swapchain, &stats, &intent)) {
        return;
    }
    if (intent.fg_backend_changed) {
        const rsf_backend_result result = rsf_d3d11_present_request(intent.fg_backend);
        if (result == RSF_BACKEND_OK) rsf_fg_choice_save(intent.fg_backend);
        say("FG provider requested at Present: backend=%u result=%d", intent.fg_backend, result);
    }
    if (intent.fg_changed || intent.reflex_changed || intent.frame_limit_changed) {
        rsf_native_fg_status current = {0}; current.struct_size = sizeof(current);
        if (rsf_native_fg_status_get(&current)) {
            if (intent.fg_changed) { current.requested.mode = intent.fg_mode; current.requested.generated_frames = intent.fg_generated; }
            if (intent.reflex_changed) current.requested.reflex_mode = intent.reflex_mode;
            if (intent.frame_limit_changed) current.requested.frame_limit_us = intent.frame_limit_us;
            rsf_native_fg_options_set(&current.requested);
            say("FG selection: requested mode=%u generated=%u Reflex=%u frame limit=%u us", current.requested.mode,
                current.requested.generated_frames, current.requested.reflex_mode, current.requested.frame_limit_us);
        }
    }
    if (intent.performance_hud_changed) performance_hud_enabled = intent.performance_hud != 0;
    /* Apply panel intents on the render thread after drawing the overlay; window callbacks
       only collect input and must not create resources or call the immediate context. */
    if (intent.start_requested) {
        if (bridge.actions.start_backend) {
            bridge.actions.start_backend();
        } else {
            say("overlay: nothing is registered to start the backend");
        }
    }
    if (intent.scale_requested && intent.scale_percent > 0) {
        if (!rsf_bridge_native_owned() && bridge.actions.set_render_scale) {
            bridge.actions.set_render_scale(intent.scale_percent);
        } else {
            say("overlay: nothing is registered to set the render scale");
        }
    }
    if (intent.debug_view_changed && (intent.debug_view != 0) != (bridge.show != 0)) {
        rsf_bridge_toggle_display();
    }
    if (intent.reinsert_changed && (intent.reinsert != 0) != (stats.reinsert_on != 0)) {
        rsf_bridge_toggle_reinsert();
    }
    if (intent.dump_requested) {
        if (bridge.actions.trigger_dump) {
            bridge.actions.trigger_dump();
        } else {
            say("overlay: nothing is registered to write a dump");
        }
    }
    if (intent.enabled_changed) {
        rsf_bridge_set_enabled(intent.enabled != 0);
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
    if (intent.quality_changed) {
        rsf_bridge_select_quality((unsigned long)intent.quality);
    }
}

/* Rebuild optional jittered layer replay/integration between frames from the observed
   layer extent, including engine pool padding. Old integrated views invalidate saved tails. */
static void follow_layer_size(void)
{
    const unsigned long width = bridge.depth_candidate_width;
    const unsigned long height = bridge.depth_candidate_height;
    rsf_depth_replay* rebuilt;
    void* output = NULL;
    if (bridge.layer_unjitter || !bridge.started || !bridge.device || width == 0 || height == 0 ||
        bridge.depth_candidate_samples != 1) {
        return;
    }
    if (bridge.layer_replay_width == width && bridge.layer_replay_height == height) {
        return;
    }
    rebuilt = rsf_depth_replay_create(bridge.device, (uint32_t)width, (uint32_t)height);
    if (!rebuilt) {
        say("layer: no depth replay could be made at %lux%lu", width, height);
        bridge.depth_candidate_width = 0;
        return;
    }
    rsf_depth_replay_destroy(bridge.layer_replay);
    bridge.layer_replay = rebuilt;
    bridge.layer_replay_width = width;
    bridge.layer_replay_height = height;
    if (bridge.layer_output_view) {
        rsf_resource_release(bridge.layer_output_view);
        bridge.layer_output_view = NULL;
        bridge.last_tail_valid = 0;
    }
    bridge.layer_output = NULL;
    if (rsf_dlss_pipeline_prepare_layer((uint32_t)width, (uint32_t)height, &output) ==
            RSF_DLSS_PIPELINE_OK &&
        output) {
        bridge.layer_output = output;
        bridge.layer_output_view = rsf_d3d11_create_shader_view(bridge.device, output);
    }
    say("layer: %lux%lu, depth replay built, integration %s", width, height,
        bridge.layer_output_view ? "ready" : "not available");
    /* The plan names the layer's integrated view, so it is rebuilt. */
    bridge.plan_stale = 1;
}


/* Requests from the hotkey worker, run on this thread at the next present. */
static volatile LONG pending_requests;

void rsf_bridge_request(unsigned long requests)
{
    InterlockedOr(&pending_requests, (LONG)requests);
}

/* The swap chain's current size, for arming extraction without the worker's view of the observer. */
static int swapchain_extent(void* swapchain, unsigned long* width, unsigned long* height)
{
    rsf_observer_status status;
    (void)swapchain;
    memset(&status, 0, sizeof(status));
    status.struct_size = sizeof(status);
    if (rsf_observer_get_status(&status) != RSF_OBSERVER_OK || status.present_width == 0) {
        return 0;
    }
    *width = status.present_width;
    *height = status.present_height;
    return 1;
}

static void run_pending_requests(void* swapchain)
{
    const LONG requests = InterlockedExchange(&pending_requests, 0);
    if (requests == 0) {
        return;
    }
    if (requests & RSF_BRIDGE_REQUEST_START) {
        if (bridge.actions.start_backend) {
            bridge.actions.start_backend();
        }
    }
    if (requests & RSF_BRIDGE_REQUEST_DISPLAY) {
        rsf_bridge_toggle_display();
    }
    if (requests & RSF_BRIDGE_REQUEST_REINSERT) {
        rsf_bridge_toggle_reinsert();
    }
    if (requests & RSF_BRIDGE_REQUEST_EXTRACT) {
        unsigned long width = 0;
        unsigned long height = 0;
        if (swapchain_extent(swapchain, &width, &height)) {
            rsf_bridge_extract_ui(width, height);
        } else {
            say("ui extract: the presented size is not known yet");
        }
    }
}

void rsf_bridge_request_shutdown(void)
{ InterlockedExchange(&shutdown_requested, 1); }
/* Retry native quiesce/stop at Present until queued callbacks drain. Keep backend and
   callback storage alive on refusal; only then disarm the tap and stop the pipeline. */
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
    stop_reinsert(); release_held(); rsf_frame_tap_uninstall();
    rsf_dlss_pipeline_stop(); bridge.started = 0;
    InterlockedExchange(&native_owner, 0); rsf_ac7_motion_capture_native_owner(0);
    bridge.startup_attempted = 1; bridge.show = 0;
    InterlockedExchange(&shutdown_requested, 0);
    say("renderer shutdown completed after native command and resource retirement");
    return 1;
}

/* Graphics-owner frame boundary: drain shutdown, apply queued requests, start/resize SR,
   maintain compatibility plans, perform bounded capture, finish frame bookkeeping, and draw UI.
   Native ownership skips binding heuristics; native graph callbacks already evaluated the scene. */
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

    /* First, so a key pressed since the last frame acts on this one, on this thread. */
    run_pending_requests(swapchain);
    {
        char motion_prefix[640];
        if (rsf_ac7_motion_capture_present(swapchain, motion_prefix, sizeof(motion_prefix))) {
            rsf_bridge_request_dump(motion_prefix);
        }
    }
    if (bridge.enabled_requested && !bridge.started && !bridge.startup_attempted &&
        bridge.actions.startup_ready && bridge.actions.startup_ready() &&
        bridge.actions.start_backend) {
        unsigned long width = 0, height = 0;
        if (swapchain_extent(swapchain, &width, &height)) {
            bridge.startup_attempted = 1;
            bridge.actions.start_backend();
        }
    }
    if (bridge.started && rsf_bridge_native_owned()) {
        unsigned long width = 0, height = 0;
        if (swapchain_extent(swapchain, &width, &height) &&
            (width != bridge.output_width || height != bridge.output_height)) {
            const rsf_dlss_pipeline_result resized = rsf_dlss_pipeline_resize_output((uint32_t)width, (uint32_t)height, NULL, NULL);
            if (resized == RSF_DLSS_PIPELINE_OK) {
                bridge.output_width = width; bridge.output_height = height;
                bridge.setting_error = NULL;
                rsf_native_sr_set_surface((uint32_t)width, (uint32_t)height); publish_native_settings();
            } else bridge.setting_error = "The backend refused the new output resolution";
        }
    }
    if (bridge.started && bridge.actions.maintain_renderer) {
        bridge.actions.maintain_renderer();
    }

    /* Refresh final-target identity for the following frame's classification callbacks. */
    {
        void* buffer = rsf_swapchain_back_buffer(swapchain);
        if (buffer) {
            bridge.present_target = buffer;
            /* The pointer outlives the reference deliberately: the swap chain owns the surface and
               this is only ever compared against a bound target, never dereferenced. Holding the
               reference is what makes ResizeBuffers fail. */
            rsf_resource_release(buffer);
        }
    }

    if (bridge.frames_described < 4 && bridge.pass_in_frame > 0) {
        say("frame ended after %lu qualifying passes", bridge.pass_in_frame);
        ++bridge.frames_described;
    }
    bridge.pass_in_frame = 0;

    /* Composite extracted UI independently of SR enablement before snapshots/debug output. */
    if (bridge.ui_extract) {
        composite_ui(swapchain);
    }

    bridge.upload_camera_valid = 0;
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
    if (bridge.gate_trace_left) {
        say("gate trace: present %lu ends, %lu passes this frame", bridge.presents,
            bridge.pass_in_frame);
        --bridge.gate_trace_left;
    }
    if (bridge.started) {
        ++bridge.presents;
        /* The runtime flushes at Present and rewrites its vtable when it does, which takes the
           tap's hooks with it; this puts them back before the next frame's first draw. */
        rsf_frame_tap_refresh();
        if (!InterlockedCompareExchange(&native_owner, 0, 0)) {
        watch_tail(swapchain);
        age_ui_targets();
        chain_rescan_tick();
        if (bridge.reinsert_on) {
            /* The evaluate already happened, at the gate, where it has to happen for the result to
               reach the game's own tonemap. All that is left is to let go of the frame's inputs and
               to close the gates so the next frame opens them again. */
            ++bridge.reinsert_frames;
            follow_recombine();
            watch_for_stalled_plan();
            /* A layer or a chain target came or went since the plan was built. Rebuilt here, on
               the thread that drives the frames, rather than inside the draw hook that noticed. */
            if (bridge.plan_stale && !bridge.tail_restaking &&
                bridge.presents >= bridge.plan_retry_after) {
                if (!install_reinsert_plan()) {
                    bridge.plan_retry_after = bridge.presents + 60;
                }
            }
            release_held();
            rsf_frame_tap_end_frame();
        } else if (bridge.enabled_requested) {
            evaluate_held(bridge.context);
        } else {
            release_held();
        }
        if (bridge.enabled_requested && !bridge.reinsert_on && bridge.composite_found &&
            bridge.scene_color && bridge.evaluated > bridge.enable_after_evaluations &&
            bridge.held_width && bridge.held_height &&
            bridge.presents >= bridge.enable_retry_after) {
            rsf_bridge_toggle_reinsert();
            bridge.enable_retry_after = bridge.presents + 60;
        }
        } else { release_held(); rsf_frame_tap_end_frame(); }
        publish_native_settings();
    }

    /* Publish completed geometry counts before clearing per-frame state. Compatibility
       observation starts with the frame tap; earlier engine draws are not represented. */
    bridge.translucent_indices_last = bridge.translucent_indices;
    bridge.translucent_draws_last = bridge.translucent_draws;
    if (bridge.briefing_capture_prefix[0] && bridge.reinsert_on && !bridge.recombine_off &&
        bridge.translucent_indices > 100000 && bridge.briefing_capture_wait < 120 &&
        ++bridge.briefing_capture_wait == 120) {
        rsf_bridge_request_dump(bridge.briefing_capture_prefix);
        rsf_bridge_report();
        say("briefing capture: collecting two frames automatically");
    }
    if (!InterlockedCompareExchange(&native_owner, 0, 0) && bridge.actions.translucent_geometry) {
        bridge.actions.translucent_geometry(bridge.translucent_indices);
    }
    bridge.translucent_indices = 0;
    bridge.translucent_draws = 0;

    rsf_ac7_scene_color_end_frame(&bridge.color_selection);
    rsf_depth_replay_end_frame(bridge.layer_replay);
    if (!InterlockedCompareExchange(&native_owner, 0, 0)) follow_layer_size();
    bridge.layer_camera_valid = 0;
    show_result(swapchain);

    /* Draw/apply overlay last, after game composition and optional debug output. */
    overlay_tick(swapchain);
}

void rsf_bridge_toggle_display(void)
{
    if (!bridge.started) {
        say("dlss bridge: nothing to show, the backend is not running");
        return;
    }
    if (!bridge.blit) {
        rsf_present_blit_setup setup;
        memset(&setup, 0, sizeof(setup));
        setup.struct_size = sizeof(setup);
        setup.abi_version = RSF_PRESENT_BLIT_ABI_VERSION;
        setup.log = bridge.log;
        setup.log_user = bridge.log_user;
        if (rsf_present_blit_create(bridge.device, &setup, &bridge.blit) != RSF_PRESENT_BLIT_OK) {
            say("dlss bridge: cannot show the result, the blit would not build");
            return;
        }
    }
    bridge.show = !bridge.show;
    say("dlss bridge: showing the reconstruction is now %s. It is scene colour from partway "
        "through the frame, so it is ungraded and has no interface on it",
        bridge.show ? "on" : "off");
}

/* Forward actual Present completion and count successful application presents. Test
   presents are excluded; generated-frame totals come from the vendor presentation status. */
static void on_present_event(void* user, const rsf_observer_present_event* event)
{
    (void)user;
    rsf_native_window_present(event);
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
}

int rsf_bridge_extract_ui(unsigned long width, unsigned long height)
{
    rsf_frame_tap_divert_setup divert;

    if (!bridge.ui || !bridge.device) {
        say("ui extract: nothing is being classified, so there is nothing to extract");
        return 0;
    }
    if (!start_extraction(width, height)) {
        return 0;
    }
    memset(&divert, 0, sizeof(divert));
    divert.struct_size = sizeof(divert);
    divert.layer_target = rsf_ui_layer_target(bridge.layer);
    divert.layer_width = (uint32_t)width;
    divert.layer_height = (uint32_t)height;
    divert.verdict = ui_verdict;
    if (!divert.layer_target) {
        /* The first frame has not begun, so there is no current slot yet. Begin one here: the layer
           is armed before the frame it belongs to rather than after. */
        rsf_ui_layer_begin_frame(bridge.layer, bridge.context);
        divert.layer_target = rsf_ui_layer_target(bridge.layer);
    }
    if (rsf_frame_tap_set_divert(&divert) != RSF_FRAME_TAP_OK) {
        say("ui extract: the frame tap refused the divert");
        return 0;
    }
    bridge.ui_extract = 1;
    say("ui extract: on. Interface draws now go to a %lux%lu layer and are composited at present",
        width, height);
    return 1;
}

/* Parse at most capacity hexadecimal values, skipping non-hex separators. Optional
 * 0x prefixes follow strtoul semantics; this permissive diagnostic parser is not a strict
 * token validator. rejected counts failed conversions rather than all stray text. */
static unsigned int parse_hash_list(const char* text, unsigned long* out, unsigned int capacity,
                                    unsigned int* rejected)
{
    unsigned int count = 0;
    const char* cursor = text;

    if (!text) {
        return 0;
    }
    while (*cursor && count < capacity) {
        char* end = NULL;
        unsigned long value;
        while (*cursor && !isxdigit((unsigned char)*cursor)) {
            ++cursor;
        }
        if (!*cursor) {
            break;
        }
        value = strtoul(cursor, &end, 16);
        if (end == cursor) {
            ++cursor;
            if (rejected) {
                ++*rejected;
            }
            continue;
        }
        out[count++] = value;
        cursor = end;
    }
    return count;
}

void rsf_bridge_set_ui_encoding(int encoding)
{
    switch (encoding) {
    case 0:
        ui_composite_mode = RSF_FULLSCREEN_PREMULTIPLIED;
        break;
    case 2:
        ui_composite_mode = RSF_FULLSCREEN_PREMULTIPLIED_GAMMA22;
        break;
    default:
        ui_composite_mode = RSF_FULLSCREEN_PREMULTIPLIED_SRGB;
        break;
    }
}

void rsf_bridge_name_shaders(const char* forced, const char* skipped)
{
    unsigned int rejected = 0;
    ui_forced_count = parse_hash_list(forced, ui_forced_hashes, RSF_UI_MAX_NAMED, &rejected);
    ui_skipped_count = parse_hash_list(skipped, ui_skipped_hashes, RSF_UI_MAX_NAMED, &rejected);
    if (ui_forced_count || ui_skipped_count || rejected) {
        say("ui: %u shaders named as interface, %u as not, %u entries unreadable",
            ui_forced_count, ui_skipped_count, rejected);
    }
}

int rsf_bridge_identify_ui(void)
{
    if (!bridge.ui) {
        bridge.ui = rsf_ui_registry_create(RSF_UI_IDENTIFY_ABI_VERSION);
        if (!bridge.ui) {
            return 0;
        }
    }
    bridge.ui_classify = 1;
    return 1;
}

rsf_observer_layout_fn rsf_bridge_layout_hook(void) { return on_layout_created; }
rsf_observer_shader_fn rsf_bridge_shader_hook(void) { return on_shader_created; }
rsf_observer_texture_fn rsf_bridge_texture_hook(void) { return on_texture_created; }

void rsf_bridge_set_actions(const rsf_bridge_actions* actions)
{
    if (actions) {
        bridge.actions = *actions;
    } else {
        memset(&bridge.actions, 0, sizeof(bridge.actions));
    }
}

/* Stop substituting and put the frame back the way the game draws it. */
static void stop_reinsert(void)
{
    rsf_frame_tap_set_plan(NULL);
    bridge.size_patch_target = NULL;
    bridge.last_tail_valid = 0;
    rsf_frame_tap_end_frame();
    bridge.reinsert_on = 0;
    say("reinsert: off, the game draws its own frame again");
}

void rsf_bridge_toggle_reinsert(void)
{
    if (InterlockedCompareExchange(&native_owner, 0, 0)) {
        rsf_bridge_set_enabled(!bridge.enabled_requested); return;
    }
    rsf_promote_setup setup;
    rsf_dlss_pipeline_status pipeline;
    void* reconstruction;

    if (bridge.reinsert_on) {
        stop_reinsert();
        return;
    }
    if (!bridge.started) {
        say("reinsert: nothing to reinsert, the backend is not running");
        return;
    }

    /* Require discovered inputs and a live reconstruction before creating replacements. */
    if (!bridge.composite) {
        say("reinsert: the composite has not been identified yet, so there is nowhere to put the "
            "result. It is learned from the draw into the back buffer in the first few frames");
        return;
    }
    if (!bridge.scene_color) {
        say("reinsert: no scene colour has been seen yet, so nothing has been reconstructed");
        return;
    }
    if (bridge.held_width == 0 || bridge.held_height == 0) {
        say("reinsert: the render resolution is not known yet");
        return;
    }

    memset(&pipeline, 0, sizeof(pipeline));
    pipeline.struct_size = sizeof(pipeline);
    if (rsf_dlss_pipeline_get_status(&pipeline) != RSF_DLSS_PIPELINE_OK || !pipeline.running) {
        say("reinsert: the pipeline is not running");
        return;
    }
    reconstruction = rsf_dlss_pipeline_output_texture();
    if (!reconstruction) {
        say("reinsert: there is no reconstruction to put back");
        return;
    }

    if (!bridge.promote) {
        memset(&setup, 0, sizeof(setup));
        setup.struct_size = sizeof(setup);
        setup.abi_version = RSF_PROMOTE_ABI_VERSION;
        setup.device = bridge.device;
        setup.output_width = pipeline.output_width;
        setup.output_height = pipeline.output_height;
        setup.log = bridge.log;
        setup.log_user = bridge.log_user;
        if (rsf_promote_create(&setup, &bridge.promote) != RSF_PROMOTE_OK) {
            say("reinsert: promotion could not be created");
            return;
        }
    }

    if (!install_reinsert_plan()) {
        return;
    }
    bridge.reinsert_on = 1;
    /* Installation success establishes armed substitutions; image correctness needs game evidence. */
    say("reinsert: on. The scene is reconstructed before the game's tonemap, and the grade and the "
        "interface are the game's own");
}


/* Prepare the replacements for `tail` and hand the tap its plan. On any failure the tap is left
   with no plan, which is what the caller then repairs. */
static int apply_tail(const rsf_promote_frame_tail* tail)
{
    rsf_frame_tap_plan plan;
    rsf_promote_result prepared;

    /* Preparing releases the previous replacements, and the tap must not be left holding views
       onto textures that are gone. Cleared first; the new plan follows within this call. */
    rsf_frame_tap_set_plan(NULL);
    bridge.size_patch_target = NULL;
    prepared = rsf_promote_prepare(bridge.promote, tail);
    if (prepared == RSF_PROMOTE_ERROR_NOT_SCALED) {
        say("reinsert: press F9 to put the render scale back first, there is nothing to upscale");
        return 0;
    }
    if (prepared != RSF_PROMOTE_OK) {
        say("reinsert: the replacements could not be prepared, result %d", (int)prepared);
        return 0;
    }

    memset(&plan, 0, sizeof(plan));
    plan.struct_size = sizeof(plan);
    if (rsf_promote_fill_plan(bridge.promote, &plan) != RSF_PROMOTE_OK) {
        say("reinsert: the plan could not be built");
        return 0;
    }
    plan.on_gate = on_gate;
    /* Promoted colour may differ from the game's depth extent. Use the carrier mismatch
       policy; none of these diagnostic policies supplies a new matching depth resource. */
    plan.depth_policy = bridge.actions.reinsert_depth_policy
                            ? (uint32_t)bridge.actions.reinsert_depth_policy()
                            : RSF_FRAME_TAP_DEPTH_DROP;

    if (rsf_frame_tap_set_plan(&plan) != RSF_FRAME_TAP_OK) {
        say("reinsert: the frame tap refused the plan");
        return 0;
    }
    /* The recombine's size constants follow the plan: promoted while its target is bound. */
    bridge.size_patch_target = tail->composed;
    return 1;
}

/* Build a tail from retained observed identities, install guarded substitutions, then reset
   discovery/stall counters. Preserve the last working tail when rebuilding can restore it. */
static int install_reinsert_plan(void)
{
    rsf_promote_frame_tail tail;
    uint32_t index;
    void* reconstruction = rsf_dlss_pipeline_output_texture();

    if (!bridge.promote || !bridge.composite || !bridge.scene_color || !reconstruction ||
        bridge.held_width == 0 || bridge.held_height == 0) {
        say("reinsert: the plan cannot be built, composite %s, scene colour %s, reconstruction %s, "
            "render size %lux%lu",
            bridge.composite ? "found" : "missing", bridge.scene_color ? "found" : "missing",
            reconstruction ? "found" : "missing", bridge.held_width, bridge.held_height);
        return 0;
    }

    memset(&tail, 0, sizeof(tail));
    tail.struct_size = sizeof(tail);
    tail.composite = bridge.composite;
    /* The interface layers the classifier has seen quads drawn into, and the chain the composite
       watch confirmed. Both are observations of this frame rather than rules about it. */
    for (index = 0; index < bridge.ui_target_count; ++index) {
        tail.ui_targets[tail.ui_target_count++] = bridge.ui_targets[index];
    }
    for (index = 0; index < bridge.chain_target_count; ++index) {
        tail.chain_targets[tail.chain_target_count++] = bridge.chain_targets[index];
    }
    tail.scene_color = bridge.scene_color;
    tail.reconstruction = reconstruction;
    tail.render_width = (uint32_t)bridge.held_width;
    tail.render_height = (uint32_t)bridge.held_height;
    tail.composite_view_format = bridge.composite_view_format;
    /* The recombine, when the game's recombine has been seen: the reconstruction goes in there and
       the game composites its full-size translucency over it. */
    tail.composed = bridge.recombine_off ? NULL : bridge.color_selection.composed;
    /* The layer, read integrated from the recombine onward, once its output exists. */
    if (!bridge.layer_unjitter && tail.composed && bridge.translucency_layer && bridge.layer_output_view) {
        tail.layer = bridge.translucency_layer;
        tail.layer_view = bridge.layer_output_view;
    }
    tail.ui_target_view_format = bridge.ui_target_view_format;
    tail.chain_view_format = bridge.chain_view_format;
    if (!apply_tail(&tail)) {
        /* Attempt to restore the last successful plan after preparation refusal; retry later. */
        if (bridge.last_tail_valid && apply_tail(&bridge.last_tail)) {
            ++bridge.plan_restores;
            say("reinsert: the previous plan is back until the new tail can be built");
        }
        return 0;
    }
    bridge.last_tail = tail;
    bridge.last_tail_valid = 1;
    /* A fresh plan describes its first frames' passes and gate decisions, so a log of a screen
       says what that screen renders. */
    bridge.frames_described = 0;
    bridge.gate_decisions_logged = 0;
    bridge.main_camera_ref_valid = 0;
    bridge.gate_trace_left = 6;
    /* The stall detector measures from here, so a fresh plan is never mistaken for a stalled one
       just because the previous plan's redirects are still the last thing counted. */
    bridge.redirect_stall = 0;
    bridge.plan_stale = 0;
    /* Unproven until a gate opens under this plan, which the stall detector measures from the
       count at this moment. A rebuild for a new layer keeps a composite that is already proven. */
    {
        rsf_frame_tap_status tap;
        memset(&tap, 0, sizeof(tap));
        tap.struct_size = sizeof(tap);
        if (rsf_frame_tap_get_status(&tap) == RSF_FRAME_TAP_OK) {
            if (bridge.plan_composite != bridge.composite) {
                bridge.plan_proven = 0;
            }
            bridge.redirects_seen = tap.gates_opened;
        }
        bridge.plan_composite = bridge.composite;
    }
    return 1;
}

/* Start SR against the observed game device, activate a prepared native plugin if accepted,
   then install the frame tap and preset. Failures stop the pipeline/tap as handled here; native
   ownership selects graph inputs instead of compatibility binding callbacks. */
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

    /* Acquire retained game device/context references through the observer. */
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
    /* Compatibility decode preserves axis scales here; frame assembly handles the backend
       convention. Native input supplies its independently decoded motion_to_uv contract. */
    setup.motion.output_scale_x = 1.0f;
    setup.motion.output_scale_y = 1.0f;
    setup.motion.invalid_value = RSF_MOTION_SENTINEL;
    setup.motion.zero_means_unwritten = 1u;
    setup.log = log;
    setup.log_user = log_user;
    setup.engine = RSF_DLSS_ENGINE_UNREAL;
    setup.engine_version_utf8 = "4.18";
    setup.project_id_utf8 = "a3ed1f08-3542-4698-b85c-e1a9908e861a";
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
    if (game_session && game_prepared) {
        if (rsf_plugin_session_start(game_session) == RSF_OK) {
            InterlockedExchange(&native_owner, 1);
            say("native AC7 renderer owns SR: graph insertion, engine output pools and downstream rectangles");
        } else say("native AC7 controller activation refused; compatibility renderer remains active");
    }
    memset(&tap, 0, sizeof(tap));
    tap.struct_size = sizeof(tap);
    tap.abi_version = RSF_FRAME_TAP_ABI_VERSION;
    tap.on_pass = rsf_bridge_native_owned() ? NULL : on_pass;
    tap.on_target_draw = rsf_bridge_native_owned() ? NULL : on_target_draw;
    tap.on_input_draw = rsf_bridge_native_owned() ? NULL : on_input_draw;
    tap.on_geometry = rsf_bridge_native_owned() ? NULL : on_geometry;
    bridge.output_width = output_width;
    bridge.output_height = output_height;
    tap.on_candidate_draw = rsf_bridge_native_owned() ? NULL : on_candidate_draw;
    tap.log = log;
    tap.log_user = log_user;
    tap.view_constant_bytes = RSF_AC7_VIEW_BUFFER_BYTES;
    /* What the tap judges sizes against. The presented size, not a size derived from the bound
       set: deriving it there made the render size an exact requirement and nothing ever matched. */
    tap.output_width = (uint32_t)output_width;
    tap.output_height = (uint32_t)output_height;

    /* No translucent depth replay: the translucency layer renders at output size against the
       reconstruction's promoted scene, so it never needs a depth of another size. */
    tapped = rsf_frame_tap_install(bridge.context, &tap);

    if (tapped != RSF_FRAME_TAP_OK) {
        say("dlss bridge: frame tap not installed, result %d", (int)tapped);
        rsf_dlss_pipeline_stop(); bridge.started = 0; publish_native_settings();
        return 0;
    }

    if (!rsf_bridge_select_quality(bridge.quality)) {
        rsf_frame_tap_uninstall();
        rsf_dlss_pipeline_stop();
        bridge.started = 0; publish_native_settings();
        return 0;
    }
    /* After the tap is installed, not before: the jitter is only worth having once there is
       something reading the frames it belongs to. */
    if (bridge.actions.set_jitter) {
        bridge.actions.set_jitter(InterlockedCompareExchange(&native_owner, 0, 0) ? 0 : 1);
    }
    say(rsf_bridge_native_owned() ? "SR running through the native AC7 post-process graph" :
        "SR compatibility renderer: watching D3D reconstruction bindings");
    return 1;
}

void rsf_bridge_report(void)
{
    rsf_dlss_pipeline_status status;
    rsf_frame_tap_status tap;

    say("layer: %lu candidate draws, %lu replayed for depth, last refusal %lu, last candidate "
        "%lux%lu samples %lu; %lu frames integrated, %lu refused, %lu without a replayed depth",
        bridge.depth_candidates, bridge.depth_replayed,
        (unsigned long)rsf_depth_replay_last_reject(bridge.layer_replay),
        bridge.depth_candidate_width, bridge.depth_candidate_height,
        bridge.depth_candidate_samples, bridge.layer_integrations,
        bridge.layer_integrations_refused, bridge.layer_depth_missing);
    say("translucent depth: %lu geometry draws reached the hook, %lu of them candidates",
        bridge.geometry_draws, bridge.depth_candidates);
    say("layer route: %s, %lu draws with an unjittered view, %lu without an available twin",
        bridge.layer_unjitter ? "unjittered direct recombine" : "jittered 1:1 integration",
        bridge.layer_draws_twinned, bridge.layer_draws_untwinned);
    if (bridge.ui) {
        rsf_ui_registry_counters counters;
        uint32_t slate = 0, canvas = 0, widget = 0;
        int moved = 0;
        unsigned int index;
        memset(&counters, 0, sizeof(counters));
        counters.struct_size = sizeof(counters);
        rsf_ui_registry_get_counters(bridge.ui, &counters);
        rsf_ui_registry_view(bridge.ui, RSF_UI_SET_SLATE_LAYOUT, &slate);
        rsf_ui_registry_view(bridge.ui, RSF_UI_SET_CANVAS_LAYOUT, &canvas);
        rsf_ui_registry_view(bridge.ui, RSF_UI_SET_WIDGET_TARGET, &widget);
        say("ui: named %lu slate layouts, %lu canvas layouts, %lu widget targets; %lu addresses "
            "forgotten on reuse, %lu adds refused full, %lu widget targets evicted as stale",
            (unsigned long)slate, (unsigned long)canvas, (unsigned long)widget,
            (unsigned long)counters.forgotten_on_reuse,
            (unsigned long)(counters.refused_full[RSF_UI_SET_SLATE_LAYOUT] +
                            counters.refused_full[RSF_UI_SET_CANVAS_LAYOUT] +
                            counters.refused_full[RSF_UI_SET_WIDGET_TARGET]),
            (unsigned long)counters.evicted[RSF_UI_SET_WIDGET_TARGET]);
        /* Suppress repeated class summaries when their counters are unchanged. */
        for (index = 0; index < 7; ++index) {
            moved = moved || bridge.ui_class_counts[index] != bridge.ui_reported_counts[index];
            bridge.ui_reported_counts[index] = bridge.ui_class_counts[index];
        }
        if (moved) {

            say("ui: %lu candidate draws: slate %lu, widget quad %lu, converter raster %lu, "
                "modulate %lu, scene %lu, unknown %lu, skipped %lu",
                bridge.ui_candidate_draws, bridge.ui_class_counts[RSF_AC7_DRAW_UI_SLATE],
                bridge.ui_class_counts[RSF_AC7_DRAW_UI_WIDGET_QUAD],
                bridge.ui_class_counts[RSF_AC7_DRAW_WIDGET_RASTER],
                bridge.ui_class_counts[RSF_AC7_DRAW_UI_MODULATE],
                bridge.ui_class_counts[RSF_AC7_DRAW_SCENE],
                bridge.ui_class_counts[RSF_AC7_DRAW_UNKNOWN],
                bridge.ui_class_counts[RSF_AC7_DRAW_SKIP]);
            if (bridge.ui_extract) {
                rsf_frame_tap_status divert_status;
                rsf_ui_layer_status layer_status;
                memset(&divert_status, 0, sizeof(divert_status));
                divert_status.struct_size = sizeof(divert_status);
                rsf_frame_tap_get_status(&divert_status);
                memset(&layer_status, 0, sizeof(layer_status));
                layer_status.struct_size = sizeof(layer_status);
                rsf_ui_layer_get_status(bridge.layer, &layer_status);
                say("ui extract: %lu draws diverted, %lu blends patched, %lu refused (last reason "
                    "%lu), %llu frames wrote the layer, %lu composited",
                    (unsigned long)divert_status.draws_diverted,
                    (unsigned long)divert_status.blend_states_patched,
                    (unsigned long)divert_status.divert_refused,
                    (unsigned long)divert_status.divert_last_refusal,
                    (unsigned long long)layer_status.frames_written, bridge.ui_composites);
                /* Report written layers without successful composites as an extraction failure. */
                if (layer_status.frames_written > 0 && bridge.ui_composites == 0) {
                    say("ui extract: the interface is being diverted and never composited, so it "
                        "is missing from the picture entirely. The layer has no target.");
                }
            }
            if (bridge.ui_widget_extent[0]) {
                say("ui: the interface is rasterized at %lux%lu and drawn into %lux%lu layers, "
                    "%u of them promoted",
                    bridge.ui_widget_extent[0], bridge.ui_widget_extent[1],
                    bridge.ui_layer_extent[0], bridge.ui_layer_extent[1],
                    (unsigned)bridge.ui_target_count);
            }
        }
    }
    say("translucent layer: last completed frame drew %lu indices in %lu draws into it",
        bridge.translucent_indices_last, bridge.translucent_draws_last);
    {
        rsf_depth_replay_detail detail;
        memset(&detail, 0, sizeof(detail));
        rsf_depth_replay_get_detail(bridge.layer_replay, &detail);
        say("layer depth: replay built %lux%lu, last draw %lux%lu samples %lu, depth view format "
            "%lu dimension %lu flags 0x%lx, its texture %lux%lu format %lu samples %lu, %lu draws, "
            "%s",
            bridge.layer_replay_width, bridge.layer_replay_height,
            (unsigned long)detail.draw_width, (unsigned long)detail.draw_height,
            (unsigned long)detail.draw_samples, (unsigned long)detail.dsv_format,
            (unsigned long)detail.dsv_dimension, (unsigned long)detail.dsv_flags,
            (unsigned long)detail.source_width, (unsigned long)detail.source_height,
            (unsigned long)detail.source_format, (unsigned long)detail.source_samples,
            (unsigned long)detail.draws, detail.refused ? "refused this frame" : "in use");
    }
    memset(&tap, 0, sizeof(tap));
    tap.struct_size = sizeof(tap);
    if (rsf_frame_tap_get_status(&tap) == RSF_FRAME_TAP_OK) {
        /* Suppress the remaining stage summary when evaluation/tap counters are unchanged. */
        if (tap.calls_seen == bridge.reported_calls && tap.passes_seen == bridge.reported_passes &&
            bridge.evaluated == bridge.reported_evaluated) {
            return;
        }
        bridge.reported_calls = tap.calls_seen;
        bridge.reported_passes = tap.passes_seen;
        bridge.reported_evaluated = bridge.evaluated;

        say("frame tap: %lu calls seen, %lu changed a binding, %lu passes matched, render %ux%u",
            (unsigned long)tap.calls_seen, (unsigned long)tap.calls_inspected,
            (unsigned long)tap.passes_seen, tap.render_width, tap.render_height);
        say("roles recognised: motion %lu, depth %lu, exposure %lu (a pass needs all three at once)",
            (unsigned long)tap.motion_seen, (unsigned long)tap.depth_seen,
            (unsigned long)tap.exposure_seen);
    }

    say("bridge: %lu passes, %lu view reads failed, %lu not the main view, %lu without jitter, "
        "%lu evaluated, %lu refused, last result %ld",
        bridge.passes, bridge.view_read_failures, bridge.not_main_view, bridge.no_jitter,
        bridge.evaluated, bridge.refused, bridge.last_result);

    /* Whether the frame's tail was ever described. Zero draws with a watch that was set is a
       different fault from a watch that was never armed, and both look like silence otherwise. */
    say("frame tail: %lu presents watched, %lu draws described, composite %s, %u chain target%s, "
        "%u interface layer%s",
        bridge.tail_frames, bridge.tail_draws,
        bridge.composite_found ? "identified" : "not identified yet",
        (unsigned)bridge.chain_target_count, bridge.chain_target_count == 1 ? "" : "s",
        (unsigned)bridge.ui_target_count, bridge.ui_target_count == 1 ? "" : "s");

    /* Report actual tap activity, not only the requested reinsertion setting. */
    if (bridge.reinsert_on) {
        say("ui: %lu unjittered view twins written, %lu interface draws given one",
            twins_written, twins_bound);
        say("reinsert: on, %lu frames, %lu evaluates at the gate, %lu bindings substituted, "
            "%lu targets redirected, %lu gates opened, %lu draws with a twin view",
            bridge.reinsert_frames, bridge.gate_evaluates, (unsigned long)tap.inputs_substituted,
            (unsigned long)tap.targets_redirected, (unsigned long)tap.gates_opened,
            (unsigned long)tap.draws_overridden);
        say("reinsert: %lu recombined results put into scene colour for the tonemap, %lu copies "
            "redirected between stand-ins, %lu copies with one side promoted, %lu uploads for the "
            "recombine had %lu size constants promoted, %lu constant uploads watched, %lu "
            "recombine gates declined as another render's camera, %lu plans restored after a "
            "failed rebuild",
            bridge.finishes, (unsigned long)tap.copies_redirected,
            (unsigned long)tap.copies_mismatched, bridge.size_uploads_patched,
            bridge.sizes_patched, (unsigned long)tap.updates_watched,
            bridge.gates_declined_camera, bridge.plan_restores);
        /* Expose incompatible promoted colour/depth pairs separately from draw classification. */
        say("reinsert: %lu depth mismatches, policy %lu (0 drop, 1 keep, 2 refuse), last pair "
            "target %lux%lu against depth %lux%lu format %lu",
            (unsigned long)tap.depth_mismatches,
            bridge.actions.reinsert_depth_policy
                ? (unsigned long)bridge.actions.reinsert_depth_policy()
                : 0ul,
            (unsigned long)tap.depth_mismatch_target_width,
            (unsigned long)tap.depth_mismatch_target_height,
            (unsigned long)tap.depth_mismatch_depth_width,
            (unsigned long)tap.depth_mismatch_depth_height,
            (unsigned long)tap.depth_mismatch_depth_format);
    }

    memset(&status, 0, sizeof(status));
    status.struct_size = sizeof(status);
    if (rsf_dlss_pipeline_get_status(&status) == RSF_DLSS_PIPELINE_OK) {
        say("pipeline: layer feature %ux%u, %llu integrated, %llu refused, last result %d",
            status.layer_width, status.layer_height,
            (unsigned long long)status.layer_frames_evaluated,
            (unsigned long long)status.layer_frames_refused, (int)status.layer_last_result);
        say("pipeline: running %u, dlss supported %u, render %ux%u, output %ux%u, evaluated %llu, "
            "refused %llu",
            status.running, status.dlss_supported, status.render_width, status.render_height,
            status.output_width, status.output_height,
            (unsigned long long)status.frames_evaluated,
            (unsigned long long)status.frames_refused);

        /* A render extent equal to output indicates native-resolution reconstruction; engine
           settings resets can otherwise hide a lost reduction ratio. */
        if (tap.render_width != 0 && status.output_width != 0 &&
            tap.render_width >= status.output_width) {
            say("note: the game is rendering at the presented size, so this is antialiasing at "
                "native resolution and not upscaling. Press F8 to put the render scale back");
        }
    }
}

void rsf_bridge_request_dump(const char* prefix)
{
    if (bridge.started) {
        /* A disabled-backend capture must not label a later enabled frame with this prefix. */
        if (bridge.enabled_requested) rsf_dlss_pipeline_request_dump(prefix);
        /* Arm bounded gate snapshots under the same prefix; native mode spans eight Present
           intervals and requests two matching composition snapshots. */
        snprintf(bridge.route_dump_prefix, sizeof(bridge.route_dump_prefix), "%s_route", prefix);
        bridge.route_dump_frame = 0;
        bridge.route_dump_serial = 0;
        bridge.route_dump_frames = rsf_bridge_native_owned() ? 8 : 2;
        if (rsf_bridge_native_owned()) rsf_native_composition_request(2);
        bridge.gate_trace_left = 4;
    }
}

void rsf_bridge_set_briefing_capture(const char* prefix)
{
    snprintf(bridge.briefing_capture_prefix, sizeof(bridge.briefing_capture_prefix), "%s",
             prefix ? prefix : "");
}

/* Create verified AC7 view twins on demand and publish upload/draw override callbacks.
   Keep the upload watch active for current-frame camera metadata even when overrides are off. */
static void configure_view_overrides(void)
{
    const int on = bridge.ui_unjitter || bridge.layer_unjitter;
    if (!bridge.started) {
        return;
    }
    if (on && !view_twins) {
        view_twins = rsf_constant_twins_create(bridge.device, RSF_AC7_VIEW_BUFFER_BYTES);
        if (!view_twins) {
            say("ui: no twin buffers could be made, so the interface keeps its view's jitter");
        }
    }
    rsf_frame_tap_set_constant_override(on && view_twins ? ui_constant_override : NULL, NULL);
    rsf_frame_tap_set_constant_override_format(bridge.layer_unjitter && view_twins ?
                                               DXGI_FORMAT_R16G16B16A16_FLOAT : 0);
    /* Always: the recombine route takes this frame's camera from the upload. */
    rsf_frame_tap_set_constant_watch(0u, on_constants, NULL);
}

void rsf_bridge_set_unjitter(int on)
{
    bridge.ui_unjitter = on != 0;
    configure_view_overrides();
    say("ui: interface drawn with %s", on ? "an unjittered twin of its view" : "its view as the game uploaded it");
}

void rsf_bridge_set_translucency_unjitter(int on)
{
    bridge.layer_unjitter = on != 0;
    configure_view_overrides();
    bridge.plan_stale = 1;
    /* A failed twin allocation must not silently select a raw jittered layer. */
    if (bridge.layer_unjitter && !view_twins) {
        bridge.layer_unjitter = 0;
        say("layer: no view twins available; retaining jittered 1:1 integration");
    }
    say("layer: %s", bridge.layer_unjitter ?
        "unjittered graphics-stage views, composited after scene SR without layer DLSS" :
        "jittered view, integrated at one to one before recombine");
}

void rsf_bridge_view_size(unsigned long* width, unsigned long* height)
{
    *width = (unsigned long)InterlockedCompareExchange(&bridge.view_width, 0, 0);
    *height = (unsigned long)InterlockedCompareExchange(&bridge.view_height, 0, 0);
}

int rsf_bridge_running(void)
{
    return bridge.started;
}
