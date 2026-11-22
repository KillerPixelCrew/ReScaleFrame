/* SPDX-License-Identifier: MIT */
/* Native leaves of the Rust render hook owner (render_hooks.rs and render_hooks/). Rust owns every
   verdict: World and Present admission, the reduced-render recovery transaction, SR and HUDless
   publication, the FRAME, SUBMISSION and WINDOW scope, the imagespace, Effects and TAA seams and
   the viewport, dynamic-resolution and prepass seams. This file keeps the typed detours and their
   MinHook trampolines, the return addresses, the last-error saves and restores, the calling
   thread's World with its COM leases and counters, and the guarded Runtime calls. A detour calls
   its original on its own C frame, so a native C++ unwind crosses only these shells. Each shell
   runs its exit work in a termination handler, as the former destructors did, and swallows
   nothing. */
#define COBJMACROS
#include "render_hooks_platform.h"
#include "cpu_events_platform.h"
#include "render_inputs.h"
#include "render_inputs_platform.h"
#include "resolution_platform.h"
#include "screen.h"
#include "depth_domains.h"
#include "native_guard.h"
#include <rescaleframe/d3d11_host.h>
#include <MinHook.h>
#include <string.h>

typedef void (*Viewport)(void*, bool);
typedef void (*Imagespace)(void);
typedef void (*WorldBegin)(void);
typedef void (*Effects)(void*, uint32_t, uint32_t, uint32_t, uint32_t);
typedef bool (*TaaActive)(void*);
typedef void (*NativePresent)(void*);
typedef void (*Prepass)(void);
typedef void (*DynamicResolution)(void*, void*, void*, void*, void*);
typedef int32_t (*UpscaleAuxiliaries)(void*, void*, const rsf_d3d11_auxiliary_pass*);
typedef int32_t (*RestoreAuxiliaries)(void*, const rsf_d3d11_auxiliary_scope*);
typedef int32_t (*ReducedFrame)(void*, void*, rsf_d3d11_reduced_frame*);
typedef int32_t (*CaptureTexture)(void*, void*, const rsf_d3d11_host_capture*);
typedef int (*CaptureRequested)(void);

/* The trampolines MinHook writes at creation and Rust clears at removal. */
static Viewport viewport;
static Imagespace imagespace;
static WorldBegin world_begin;
static Effects effects;
static TaaActive taa_active;
static NativePresent native_present;
static Prepass prepass;
static DynamicResolution dynamic_resolution;

/* Runtime exports Rust resolved by name and bound before any hook is enabled. */
static UpscaleAuxiliaries upscale_auxiliaries;
static RestoreAuxiliaries restore_auxiliaries;
static ReducedFrame reduced_frame;
static CaptureTexture capture_texture;
static CaptureRequested capture_requested;

/* Bound by the Rust prepare (render_hooks.rs) before any hook is enabled. */
static rsf_game_host_services host;

/* Detours in flight and open scopes. Rust adds the resolution owner to the busy answer. */
static volatile LONG entered, windows, frames, auxiliaries;
static volatile LONG announced;

#define RSF_FALLOUT4_EMPTY_SCREEN {RSF_SCREEN_UNKNOWN, 0, 0, UINT32_MAX, 0}
static const NativeScreen empty_screen = RSF_FALLOUT4_EMPTY_SCREEN;

/* The calling thread's World. Its COM members each hold one reference; world_clear releases them. */
typedef struct World {
    rsf_fallout4_cpu_identity cpu;
    rsf_game_render_pass pass, frame_pass, submission_pass, screen_basis;
    NativeScreen screen;
    NativeDepthSnapshot depth_snapshot, raster_snapshot;
    /* Recovery leases. Rust (render_hooks/recovery.rs) owns the token and decides adoption and
       release; the candidate lives only for one Rust step. */
    NativeInputs recovery_inputs, recovery_candidate;
    /* SR input and HUDless framebuffer leases. Rust (render_hooks/publication.rs) releases each
       when its step returns, as the former stack leases ended. */
    NativeInputs sr_inputs;
    /* Candidate owner Context and chain lease. Rust (render_hooks/scope.rs) adopts it into the
       World or discards it when the begin step returns. */
    ID3D11DeviceContext* owner_context;
    IDXGISwapChain* owner_chain;
    bool full_output_resolved;
    rsf_d3d11_auxiliary_scope auxiliary_scope;
    ID3D11DeviceContext* context;
    IDXGISwapChain* chain;
    ID3D11Texture2D* final_colour;
    ID3D11Texture2D* colour;
    ID3D11Texture2D* depth;
    ID3D11Texture2D* motion;
    bool in_imagespace, inputs, published, window_open, frame_open, submission_open, refused, depth_interrupted,
        auxiliary_open, raster_refused;
} World;

RSF_FALLOUT4_THREAD_LOCAL static World world = {.screen = RSF_FALLOUT4_EMPTY_SCREEN};
RSF_FALLOUT4_THREAD_LOCAL static bool ordinary_world;
/* Nested prepass depth; the detour's termination handler restores it when the original returns or unwinds. */
RSF_FALLOUT4_THREAD_LOCAL static uint32_t prepass_depth;

_Static_assert(sizeof(rsf_d3d11_reduced_frame) == 56 && offsetof(rsf_d3d11_reduced_frame, result_flags) == 52,
    "rsf_d3d11_reduced_frame layout");
_Static_assert(sizeof(rsf_d3d11_auxiliary_surface) == 80 && offsetof(rsf_d3d11_auxiliary_surface, flags) == 72,
    "rsf_d3d11_auxiliary_surface layout");

static void com_release(void** slot)
{
    void* object = *slot;
    *slot = NULL;
    if (object) IUnknown_Release((IUnknown*)object);
}

/* ComPtr assignment: take a reference on the new object, then drop the old one. */
static void com_assign(void** slot, void* value)
{
    if (value) IUnknown_AddRef((IUnknown*)value);
    void* old = *slot;
    *slot = value;
    if (old) IUnknown_Release((IUnknown*)old);
}

/* ComPtr move assignment: the target takes the source's reference and the source clears. */
static void com_move(void** target, void** source)
{
    void* value = *source;
    *source = NULL;
    void* old = *target;
    *target = value;
    if (old) IUnknown_Release((IUnknown*)old);
}

/* World = {}: release every lease and restore the empty screen. */
static void world_clear(void)
{
    rsf_fallout4_native_inputs_release(&world.recovery_inputs);
    rsf_fallout4_native_inputs_release(&world.recovery_candidate);
    rsf_fallout4_native_inputs_release(&world.sr_inputs);
    com_release((void**)&world.owner_context);
    com_release((void**)&world.owner_chain);
    com_release((void**)&world.context);
    com_release((void**)&world.chain);
    com_release((void**)&world.final_colour);
    com_release((void**)&world.colour);
    com_release((void**)&world.depth);
    com_release((void**)&world.motion);
    memset(&world, 0, sizeof(world));
    world.screen = empty_screen;
}

/* Rust owns the live flag with its release store and acquire load. */
static bool live(void)
{
    return rsf_fallout4_render_live();
}

static void current_cpu_identity(rsf_fallout4_cpu_identity* identity)
{
    memset(identity, 0, sizeof(*identity));
    rsf_fallout4_cpu_current_identity(identity);
}

static void report(const char* message)
{
    if (host.log) {
        RSF_FOREIGN_TRY { host.log(host.user, message); }
        RSF_FOREIGN_CATCH { }
    }
}

static void notify(const rsf_game_render_pass* pass, void* context, uint32_t begin)
{
    if (host.render_pass) {
        RSF_FOREIGN_TRY { host.render_pass(host.user, context, pass, begin); }
        RSF_FOREIGN_CATCH { }
    }
}

static bool drain_auxiliaries(void)
{
    if (!world.auxiliary_open) return true;
    int32_t result = RSF_ERROR_NOT_READY;
    RSF_FOREIGN_TRY { result = restore_auxiliaries(host.user, &world.auxiliary_scope); }
    RSF_FOREIGN_CATCH { }
    if (result != RSF_OK) {
        world.refused = true;
        return false;
    }
    world.auxiliary_open = false;
    InterlockedDecrement(&auxiliaries);
    return true;
}

/* Recovery leaves (render_hooks_platform.h). Rust sequences the transaction and decides every
   verdict; these read the World, compare COM identity, hold the leases and guard the Runtime. */
static void read_world(NativeWorld* out)
{
    *out = (NativeWorld){
        .cpu = world.cpu,
        .pass = &world.pass,
        .frame_pass = &world.frame_pass,
        .submission_pass = &world.submission_pass,
        .screen_basis = &world.screen_basis,
        .context = world.context,
        .chain = world.chain,
        .upscale = host.process_upscale != NULL,
        .recovery_exported = reduced_frame != NULL,
        .in_imagespace = world.in_imagespace,
        .inputs = world.inputs,
        .published = world.published,
        .window_open = world.window_open,
        .frame_open = world.frame_open,
        .submission_open = world.submission_open,
        .refused = world.refused,
        .raster_refused = world.raster_refused,
        .auxiliary_open = world.auxiliary_open,
        .depth_valid = world.depth_snapshot.valid,
        .raster_valid = world.raster_snapshot.raster_valid,
        .depth_interrupted = world.depth_interrupted,
        .full_output_resolved = world.full_output_resolved,
    };
}

static void recovery_refuse(void)
{
    world.refused = true;
}

static bool recovery_same_source(void)
{
    return world.cpu.session == host.session_id && rsf_fallout4_cpu_render_source_is_current(&world.cpu);
}

static void recovery_resolve(bool full_output, bool published)
{
    world.full_output_resolved = full_output;
    world.published = published;
}

static bool recovery_read_raster(bool producer)
{
    rsf_fallout4_native_inputs_release(&world.recovery_candidate);
    if (!producer) return rsf_fallout4_read_native_raster_inputs(&world.recovery_candidate, &world.frame_pass, NULL);
    return world.raster_snapshot.raster_valid &&
        rsf_fallout4_read_native_raster_inputs(&world.recovery_candidate, &world.frame_pass, &world.raster_snapshot);
}

static bool recovery_same_resources(bool owners)
{
    return rsf_fallout4_inputs_same_resources(&world.recovery_candidate, &world.recovery_inputs, owners);
}

static void recovery_adopt(const int32_t* render_rect)
{
    NativeInputs* current = &world.recovery_candidate;
    if (render_rect) memcpy(current->pass.render_rect, render_rect, sizeof(current->pass.render_rect));
    rsf_fallout4_native_inputs_move(&world.recovery_inputs, current);
}

static void recovery_discard(void)
{
    rsf_fallout4_native_inputs_release(&world.recovery_candidate);
}

static void recovery_release(void)
{
    rsf_fallout4_native_inputs_release(&world.recovery_inputs);
}

static void recovery_lease(uint32_t slot, NativeRecoveryLease* lease)
{
    const NativeInputs* inputs = slot == RecoveryLeaseCandidate ? &world.recovery_candidate : &world.recovery_inputs;
    *lease = (NativeRecoveryLease){
        .pass = &inputs->pass,
        .context = inputs->context,
        .textures = {inputs->depth, inputs->motion, inputs->pipboy, inputs->vats, inputs->refraction_normals},
    };
}

static bool recovery_raster_current(void)
{
    return rsf_fallout4_depth_raster_current(&world.raster_snapshot);
}

static bool recovery_begin_output(void)
{
    return rsf_fallout4_resolution_begin_output(&world.raster_snapshot, &world.recovery_inputs.pass);
}

static bool recovery_reduce(rsf_d3d11_reduced_frame* request, int32_t* result)
{
    if (!reduced_frame || !world.recovery_inputs.context) return false;
    request->owner = &world.recovery_inputs.pass;
    *result = RSF_ERROR_NOT_READY;
    RSF_FOREIGN_TRY { *result = reduced_frame(host.user, world.recovery_inputs.context, request); }
    RSF_FOREIGN_CATCH { }
    return true;
}

static const NativeRecoveryLeaves recovery_leaves = {report, recovery_refuse, read_world, recovery_same_source,
    recovery_resolve, recovery_read_raster, recovery_same_resources, recovery_adopt, recovery_discard,
    recovery_release, recovery_lease, rsf_fallout4_resolution_preflight, recovery_raster_current,
    recovery_begin_output, recovery_reduce};

static bool recovery_owned(void)
{
    return rsf_fallout4_recovery_token() != 0;
}

static bool reserve_recovery(const rsf_game_render_pass* planned)
{
    return rsf_fallout4_recovery_reserve(&recovery_leaves, planned);
}

static bool publication_auxiliary_inputs(NativeAuxiliaryInputs* out)
{
    const NativeInputs* inputs = &world.sr_inputs;
    if (!rsf_fallout4_render_auxiliary_depth(inputs->depth)) return false;
    const float* raster = world.depth_snapshot.actual_raster;
    *out = (NativeAuxiliaryInputs){
        .textures = {inputs->depth, inputs->motion, inputs->pipboy, inputs->vats, inputs->refraction_normals},
        .raster = {raster[0], raster[1], raster[2], raster[3]},
    };
    return true;
}

static bool publication_auxiliary_colour(void* texture)
{
    const NativeInputs* inputs = &world.sr_inputs;
    return rsf_fallout4_render_auxiliary_colour(texture, inputs->context, inputs->pass.camera.output_width,
        inputs->pass.camera.output_height);
}

/* Evaluate the input lease; a throw drains auxiliaries and answers RSF_ERROR_NOT_READY. */
static int32_t publication_reconstruct(const rsf_game_render_pass* pass,
    const rsf_d3d11_auxiliary_surface* surfaces, uint32_t count)
{
    void* context = world.sr_inputs.context;
    int32_t result = RSF_ERROR_NOT_READY;
    bool threw = false;
    RSF_FOREIGN_TRY {
        if (count == 0) {
            result = host.process_upscale(host.user, context, pass);
        } else {
            const rsf_d3d11_auxiliary_pass request = {
                sizeof(request), RSF_D3D11_HOST_ABI_VERSION, pass, count, surfaces};
            if (!recovery_owned()) {
                world.auxiliary_scope = (rsf_d3d11_auxiliary_scope){sizeof(world.auxiliary_scope),
                    RSF_D3D11_HOST_ABI_VERSION, pass->session_id, pass->native_frame, pass->view_key};
                world.auxiliary_open = true;
                InterlockedIncrement(&auxiliaries);
            }
            /* The host adopts the sealed recovery originals while retaining this actual
               fractional viewport/jitter temporal mapping. One evaluation, one owner. */
            result = upscale_auxiliaries(host.user, context, &request);
            if (result != RSF_OK && !recovery_owned()) (void)drain_auxiliaries();
        }
    }
    RSF_FOREIGN_CATCH { threw = true; }
    if (threw) {
        (void)drain_auxiliaries();
        return RSF_ERROR_NOT_READY;
    }
    return result;
}

/* SR and HUDless publication leaves (render_hooks_platform.h). Rust (render_hooks/publication.rs)
   sequences both seams. These leaves hold the SR input and retained WINDOW references
   in the native World and guard Runtime calls. */
static bool publication_read_inputs(void)
{
    rsf_fallout4_native_inputs_release(&world.sr_inputs);
    return rsf_fallout4_read_native_inputs(&world.sr_inputs, report, &world.depth_snapshot);
}

static void publication_inputs(NativePublicationInputs* out)
{
    *out = (NativePublicationInputs){&world.sr_inputs.pass, world.sr_inputs.context, world.sr_inputs.chain};
}

static void publication_release_inputs(void)
{
    rsf_fallout4_native_inputs_release(&world.sr_inputs);
}

static bool publication_same_recovery_resources(void)
{
    return rsf_fallout4_inputs_same_resources(&world.sr_inputs, &world.recovery_inputs, false);
}

static bool publication_depth_domains(rsf_game_render_pass* pass)
{
    return rsf_fallout4_depth_make(&world.depth_snapshot, &world.cpu, pass, &pass->depth_domains, report);
}

static bool publication_plan(const rsf_game_render_pass* planning)
{
    return rsf_fallout4_resolution_reconfigure(host.process_upscale, host.user, world.sr_inputs.context, planning);
}

static int32_t publication_render_config(rsf_game_render_config* config)
{
    return host.render_config ? rsf_fallout4_resolution_config(host.render_config, host.user, config) : 0;
}

static void publication_retain_inputs(const rsf_game_render_pass* basis)
{
    com_assign((void**)&world.colour, world.sr_inputs.colour);
    com_assign((void**)&world.depth, world.sr_inputs.depth);
    com_assign((void**)&world.motion, world.sr_inputs.motion);
    world.screen_basis = *basis;
}

static void publication_classify(bool first, NativeScreen* current, NativeScreen* leased)
{
    rsf_fallout4_screen_classify(&world.cpu, &world.screen_basis, true, current);
    if (first) world.screen = *current;
    *leased = world.screen;
}

static void publication_notify(const rsf_game_render_pass* pass, void* context, uint32_t begin)
{
    notify(pass, context, begin);
}

static void publication_publish_inputs(const rsf_game_render_pass* scalar)
{
    world.pass = *scalar;
    com_assign((void**)&world.context, world.sr_inputs.context);
    com_assign((void**)&world.chain, world.sr_inputs.chain);
    world.inputs = true;
}

static bool publication_begin_output(const rsf_game_render_pass* pass)
{
    return rsf_fallout4_resolution_begin_output(&world.depth_snapshot, pass);
}

static void publication_report_state(uint32_t stage, const rsf_game_render_pass* pass, void* context,
    bool published, int32_t result)
{
    rsf_fallout4_resolution_report(stage, pass, context, published, result, report);
}

static void publication_publish_window(void* texture, const rsf_game_render_pass* window)
{
    com_assign((void**)&world.final_colour, texture);
    world.pass = *window;
    world.window_open = true;
    InterlockedIncrement(&windows);
}

static const NativePublicationLeaves publication_leaves = {read_world, rsf_fallout4_render_diagnostic,
    publication_read_inputs, publication_inputs, publication_release_inputs, publication_same_recovery_resources,
    publication_depth_domains, publication_plan, publication_render_config, publication_retain_inputs,
    publication_classify, publication_notify, publication_publish_inputs, publication_begin_output,
    publication_report_state, publication_auxiliary_inputs, publication_auxiliary_colour,
    rsf_fallout4_render_same_object, publication_reconstruct, rsf_fallout4_resolution_end_output,
    publication_publish_window};

/* FRAME, SUBMISSION and WINDOW scope leaves (render_hooks_platform.h). Rust (render_hooks/scope.rs)
   decides the owner lease verdict, the FRAME identity and the begin and close order; these read
   the native owner, compare COM identity, hold the Context and chain leases and keep the counters. */
static bool scope_lease_owner(const NativeRenderOwner* before)
{
    bool owned = false;
    RSF_FOREIGN_TRY {
        com_assign((void**)&world.owner_context, (void*)before->context);
        com_assign((void**)&world.owner_chain, (void*)before->chain);
        owned = rsf_fallout4_render_check_owner(world.owner_context, world.owner_chain, before->device,
            before->window);
    }
    RSF_FOREIGN_CATCH { owned = false; }
    return owned;
}

static void scope_adopt_owner(void)
{
    com_move((void**)&world.context, (void**)&world.owner_context);
    com_move((void**)&world.chain, (void**)&world.owner_chain);
}

static void scope_discard_owner(void)
{
    com_release((void**)&world.owner_context);
    com_release((void**)&world.owner_chain);
}

static void scope_open_frame(void)
{
    world.frame_open = true;
    InterlockedIncrement(&frames);
}

static void scope_open_submission(void)
{
    world.submission_open = true;
}

static void scope_close_submission(void)
{
    world.submission_open = false;
}

static void scope_close_window(void)
{
    world.window_open = false;
    InterlockedDecrement(&windows);
}

static void scope_close_frame(void)
{
    world.frame_open = false;
    InterlockedDecrement(&frames);
}

static void scope_reset(void)
{
    world_clear();
}

static const NativeScopeLeaves scope_leaves = {read_world, rsf_fallout4_render_read_owner, scope_lease_owner,
    scope_adopt_owner, scope_discard_owner, scope_open_frame, scope_open_submission, scope_close_submission,
    scope_close_window, scope_close_frame, drain_auxiliaries, rsf_fallout4_resolution_finish_source, scope_reset,
    publication_notify, rsf_fallout4_render_diagnostic, publication_report_state};

/* Imagespace, Effects and TAA leaves (render_hooks_platform.h). Rust (render_hooks/imagespace.rs)
   selects the calls and sequences entry, the SR seam, retirement, drain, output resolution and
   the TAA gate; these read the World, read the native manager cell and keep the World flags. */
static void imagespace_enter(void)
{
    world.in_imagespace = true;
}

static void imagespace_leave(void)
{
    world.in_imagespace = world.published = world.full_output_resolved = false;
}

static const NativeImagespaceLeaves imagespace_leaves = {read_world, imagespace_enter, imagespace_leave,
    rsf_fallout4_render_read_manager, drain_auxiliaries, rsf_fallout4_resolution_end_output,
    publication_report_state, rsf_fallout4_render_diagnostic};

/* Viewport, DR and prepass leaves (render_hooks_platform.h). Rust (render_hooks/producer.rs)
   selects the calls and sequences the selector write, the HUDless seam, the DR admission and the
   prepass capture; these read the World, call the typed screen, owner, resolution and depth
   readers and keep the World fields and snapshots. */
static void producer_invalidate(bool raster)
{
    memset(&world.depth_snapshot, 0, sizeof(world.depth_snapshot));
    if (raster) memset(&world.raster_snapshot, 0, sizeof(world.raster_snapshot));
}

static void producer_interrupt_depth(bool interrupted)
{
    world.depth_interrupted = interrupted;
}

static void producer_refuse_raster(void)
{
    world.raster_refused = world.refused = true;
}

static uint32_t producer_screen_refusals(void)
{
    return rsf_fallout4_ordinary_screen_refusals(&world.cpu);
}

static bool producer_configure(void)
{
    return rsf_fallout4_resolution_configure(&host, &world.frame_pass, world.context, reserve_recovery);
}

/* The raster capture does not establish temporal admission. The depth capture uses
   the ABI identity recorded across the admitted original prepass call. */
static bool producer_capture_raster(const rsf_fallout4_cpu_identity* identity)
{
    return rsf_fallout4_depth_capture_raster(identity, &world.raster_snapshot, report);
}

static bool producer_capture_depth(const rsf_fallout4_cpu_identity* identity, NativeDepthSnapshot* snapshot)
{
    return rsf_fallout4_depth_capture(identity, snapshot, report);
}

static void producer_adopt_depth(const NativeDepthSnapshot* snapshot)
{
    world.depth_snapshot = *snapshot;
}

static const NativeProducerLeaves producer_leaves = {read_world, producer_invalidate, producer_interrupt_depth,
    producer_refuse_raster, rsf_fallout4_resolution_native_dr, producer_screen_refusals,
    rsf_fallout4_render_read_owner, producer_configure, producer_capture_raster, producer_capture_depth,
    producer_adopt_depth, publication_report_state, rsf_fallout4_render_diagnostic};

static void observe_viewport(void* manager, bool enabled)
{
    const uintptr_t caller = RSF_FALLOUT4_CALLER();
    InterlockedIncrement(&entered);
    RSF_FOREIGN_TRY {
        viewport(manager, enabled);
        const DWORD error = GetLastError();
        /* Rust follows the native intent with the owned selector and runs the HUDless seam. */
        rsf_fallout4_render_after_viewport(&producer_leaves, &publication_leaves, &recovery_leaves, caller,
            (uintptr_t)manager, enabled);
        SetLastError(error);
    }
    RSF_FOREIGN_FINALLY { InterlockedDecrement(&entered); }
}

static void observe_world_begin(void)
{
    InterlockedIncrement(&entered);
    const DWORD error = GetLastError();
    const uintptr_t caller = RSF_FALLOUT4_CALLER();
    rsf_fallout4_cpu_identity cpu;
    current_cpu_identity(&cpu);
    const bool previous = ordinary_world;
    RSF_FOREIGN_TRY {
        const bool active = live();
        const NativeWorldAdmission candidate = {caller, active ? host.session_id : 0, active, ordinary_world,
            world.frame_open, world.auxiliary_open, recovery_owned()};
        if (rsf_fallout4_render_admit_world(&candidate, &cpu)) {
            ordinary_world = true;
            world_clear();
            rsf_fallout4_recovery_reset();
            current_cpu_identity(&world.cpu);
            rsf_fallout4_render_begin_scope(&scope_leaves, &recovery_leaves);
        }
        SetLastError(error);
        world_begin();
    }
    RSF_FOREIGN_FINALLY {
        ordinary_world = previous;
        InterlockedDecrement(&entered);
    }
}

static void observe_dynamic_resolution(void* manager, void* a, void* b, void* c, void* d)
{
    const uintptr_t caller = RSF_FALLOUT4_CALLER();
    InterlockedIncrement(&entered);
    RSF_FOREIGN_TRY {
        const DWORD error = GetLastError();
        rsf_fallout4_render_before_dynamic_resolution(&producer_leaves);
        SetLastError(error);
        dynamic_resolution(manager, a, b, c, d);
        const DWORD native_error = GetLastError();
        /* Rust refuses an overlapping writer and admits the selected configuration. */
        rsf_fallout4_render_after_dynamic_resolution(&producer_leaves, &recovery_leaves, caller, (uintptr_t)manager);
        SetLastError(native_error);
    }
    RSF_FOREIGN_FINALLY { InterlockedDecrement(&entered); }
}

static void observe_prepass(void)
{
    const uintptr_t caller = RSF_FALLOUT4_CALLER();
    InterlockedIncrement(&entered);
    const DWORD error = GetLastError();
    ++prepass_depth;
    RSF_FOREIGN_TRY {
        /* Rust records the World source, selects the call and invalidates the previous snapshots; the
           capture after the original uses that record. */
        NativePrepassCall call;
        memset(&call, 0, sizeof(call));
        rsf_fallout4_render_before_prepass(&producer_leaves, &recovery_leaves, caller, prepass_depth, &call);
        SetLastError(error);
        prepass();
        const DWORD native_error = GetLastError();
        rsf_fallout4_render_after_prepass(&producer_leaves, &recovery_leaves, &call);
        SetLastError(native_error);
    }
    RSF_FOREIGN_FINALLY {
        --prepass_depth;
        InterlockedDecrement(&entered);
    }
}

static void observe_imagespace(void)
{
    const uintptr_t caller = RSF_FALLOUT4_CALLER();
    InterlockedIncrement(&entered);
    const DWORD error = GetLastError();
    /* Rust selects the call and enters imagespace; the exit runs after the original returns or
       unwinds, between its own LastError save and restore. */
    const bool selected = rsf_fallout4_render_enter_imagespace(&imagespace_leaves, &recovery_leaves, caller);
    RSF_FOREIGN_TRY {
        SetLastError(error);
        imagespace();
    }
    RSF_FOREIGN_FINALLY {
        if (selected) {
            const DWORD exit_error = GetLastError();
            rsf_fallout4_render_leave_imagespace(&imagespace_leaves, &recovery_leaves);
            SetLastError(exit_error);
        }
        InterlockedDecrement(&entered);
    }
}

static void observe_effects(void* manager, uint32_t first, uint32_t last, uint32_t mode, uint32_t flags)
{
    const uintptr_t caller = RSF_FALLOUT4_CALLER();
    InterlockedIncrement(&entered);
    RSF_FOREIGN_TRY {
        const DWORD error = GetLastError();
        /* Rust verifies the call and arguments against the native manager read, then sequences the
           pre-SR receipt and the SR seam before the original, and the Effects receipt, RETIRE and
           auxiliary drain after it. */
        const NativeEffectsCall call = {caller, (uintptr_t)manager, first, last, mode, flags};
        rsf_fallout4_cpu_identity receipt;
        memset(&receipt, 0, sizeof(receipt));
        const bool admitted = rsf_fallout4_render_before_effects(&imagespace_leaves, &publication_leaves,
            &recovery_leaves, &call, &receipt);
        SetLastError(error);
        effects(manager, first, last, mode, flags);
        const DWORD native_error = GetLastError();
        if (admitted) rsf_fallout4_render_after_effects(&imagespace_leaves, &recovery_leaves, &receipt);
        SetLastError(native_error);
    }
    RSF_FOREIGN_FINALLY { InterlockedDecrement(&entered); }
}

static bool observe_taa(void* effect)
{
    bool active = false;
    InterlockedIncrement(&entered);
    RSF_FOREIGN_TRY {
        const bool result = taa_active(effect);
        const DWORD error = GetLastError();
        /* Rust decides the exclusion and its once-per-combination receipt. */
        active = rsf_fallout4_render_gate_taa(&imagespace_leaves, &recovery_leaves, result);
        SetLastError(error);
    }
    RSF_FOREIGN_FINALLY { InterlockedDecrement(&entered); }
    return active;
}

static bool recovery_capture_requested(void)
{
    bool requested = false;
    RSF_FOREIGN_TRY { requested = capture_texture && capture_requested && capture_requested(); }
    RSF_FOREIGN_CATCH { requested = false; }
    return requested;
}

static void recovery_capture(void* context, const rsf_d3d11_host_capture* capture)
{
    RSF_FOREIGN_TRY { (void)capture_texture(host.user, context, capture); }
    RSF_FOREIGN_CATCH { }
}

static bool revalidate_present_screen(void)
{
    if (world.inputs) rsf_fallout4_render_screen_facts(&publication_leaves, &recovery_leaves, &world.pass, false);
    return world.refused;
}

static void observe_native_present(void* renderer)
{
    const uintptr_t caller = RSF_FALLOUT4_CALLER();
    InterlockedIncrement(&entered);
    const DWORD error = GetLastError();
    /* Close the same CPU phase after the original, even if it reenters. */
    rsf_fallout4_cpu_identity phase_owner;
    memset(&phase_owner, 0, sizeof(phase_owner));
    volatile bool phase_active = false;
    RSF_FOREIGN_TRY {
        const NativeRenderScopeKey leased = {world.frame_pass.native_frame, world.frame_pass.view_key,
            world.frame_pass.window_key, (uintptr_t)world.context, (uintptr_t)world.chain};
        const NativePresentMatch matched = rsf_fallout4_render_match_present(caller, (uintptr_t)renderer,
            world.submission_open, &leased);
        if (matched != PresentUnrelated) {
            const rsf_fallout4_cpu_identity cpu = world.cpu;
            if (matched == PresentMatched)
                rsf_fallout4_render_capture_recovery(&world.raster_snapshot, read_world, report,
                    recovery_capture_requested, recovery_capture);
            switch (rsf_fallout4_render_admit_present(matched, host.session_id, &cpu, revalidate_present_screen)) {
            case PresentComplete:
                rsf_fallout4_render_close_submission(&scope_leaves);
                phase_owner = cpu;
                phase_active = rsf_fallout4_cpu_begin_present_phase(&phase_owner, leased.frame, leased.window);
                if (InterlockedExchange(&announced, 1) == 0)
                    report("Fallout4 native SUBMISSION end before original outer Present; FRAME/WINDOW remain open.");
                break;
            case PresentRefused:
                rsf_fallout4_render_diagnostic(
                    "Fallout4 pre-Present screen/refusal changed; submission completion withheld from FG.");
                break;
            case PresentSourceChanged:
                /* Missing completion refuses prepare; the old scope drains after Swap. */
                world.refused = true;
                rsf_fallout4_render_diagnostic(
                    "Fallout4 pre-Present source/owner changed; submission completion withheld from FG.");
                break;
            }
        }
        SetLastError(error);
        native_present(renderer);
    }
    RSF_FOREIGN_FINALLY {
        if (phase_active) {
            const DWORD exit_error = GetLastError();
            rsf_fallout4_cpu_end_present_phase(&phase_owner);
            SetLastError(exit_error);
        }
        InterlockedDecrement(&entered);
    }
}

/* NativeRenderHook order. MinHook writes each typed trampoline at creation; the detours call
   them on their own C frames. Rust (render_hooks.rs) owns the RVAs, entry bytes and created
   state of these records. */
static void* render_detour(uint32_t hook)
{
    switch (hook) {
    case RenderHookDynamicResolution: return (void*)&observe_dynamic_resolution;
    case RenderHookPrepass: return (void*)&observe_prepass;
    case RenderHookNativePresent: return (void*)&observe_native_present;
    case RenderHookWorldBegin: return (void*)&observe_world_begin;
    case RenderHookViewport: return (void*)&observe_viewport;
    case RenderHookImagespace: return (void*)&observe_imagespace;
    case RenderHookEffects: return (void*)&observe_effects;
    case RenderHookTaa: return (void*)&observe_taa;
    default: return NULL;
    }
}

static void** render_original(uint32_t hook)
{
    switch (hook) {
    case RenderHookDynamicResolution: return (void**)&dynamic_resolution;
    case RenderHookPrepass: return (void**)&prepass;
    case RenderHookNativePresent: return (void**)&native_present;
    case RenderHookWorldBegin: return (void**)&world_begin;
    case RenderHookViewport: return (void**)&viewport;
    case RenderHookImagespace: return (void**)&imagespace;
    case RenderHookEffects: return (void**)&effects;
    case RenderHookTaa: return (void**)&taa_active;
    default: return NULL;
    }
}

void rsf_fallout4_render_report(const char* message)
{
    report(message);
}

bool rsf_fallout4_render_create_hook(uint32_t hook, void* target)
{
    return hook < RenderHookCount && MH_CreateHook(target, render_detour(hook), render_original(hook)) == MH_OK;
}

bool rsf_fallout4_render_remove_hook(uint32_t hook, void* target)
{
    if (hook >= RenderHookCount || MH_RemoveHook(target) != MH_OK) return false;
    *render_original(hook) = NULL;
    return true;
}

/* The native half of busy; Rust adds the resolution owner. */
bool rsf_fallout4_render_scopes_busy(void)
{
    return InterlockedCompareExchange(&entered, 0, 0) || InterlockedCompareExchange(&windows, 0, 0) ||
        InterlockedCompareExchange(&frames, 0, 0) || InterlockedCompareExchange(&auxiliaries, 0, 0);
}

void rsf_fallout4_render_bind_exports(const NativeRenderExports* exports)
{
    upscale_auxiliaries = (UpscaleAuxiliaries)exports->upscale_auxiliaries;
    restore_auxiliaries = (RestoreAuxiliaries)exports->restore_auxiliaries;
    reduced_frame = (ReducedFrame)exports->reduced_frame;
    capture_texture = (CaptureTexture)exports->capture_texture;
    capture_requested = (CaptureRequested)exports->capture_requested;
}

void rsf_fallout4_render_bind_host(const rsf_game_host_services* services)
{
    host = *services;
}

/* Rust (render_hooks/scope.rs) sequences the post-Swap scope end. */
void rsf_fallout4_cpu_finish_render_scope(void)
{
    rsf_fallout4_render_finish_scope(&scope_leaves, &recovery_leaves);
}
