// SPDX-License-Identifier: MIT
#pragma once
// Private boundary between render_hooks.cpp and its Rust owner (render_hooks.rs): the render hook
// lifecycle, the ordinary World and outer Present guards, the reduced-render recovery
// transaction (render_hooks/recovery.rs), SR and HUDless publication
// (render_hooks/publication.rs), the FRAME, SUBMISSION and WINDOW scope
// (render_hooks/scope.rs), the imagespace, Effects and TAA seams (render_hooks/imagespace.rs) and
// the viewport, dynamic-resolution and prepass seams (render_hooks/producer.rs). Not a DLL ABI.
#include "render_records.h"

typedef struct rsf_d3d11_reduced_frame rsf_d3d11_reduced_frame;
typedef struct rsf_d3d11_host_capture rsf_d3d11_host_capture;
typedef struct rsf_d3d11_auxiliary_surface rsf_d3d11_auxiliary_surface;

RSF_FALLOUT4_RENDER_ABI_BEGIN
// Detours in creation and enable order. Rust owns each record's RVA, expected entry bytes and
// created state; render_hooks.cpp owns the typed detour and its typed trampoline.
enum {
    RenderHookDynamicResolution = 0, // void(void*, void*, void*, void*, void*)
    RenderHookPrepass = 1,           // void()
    RenderHookNativePresent = 2,     // void(void* renderer)
    RenderHookWorldBegin = 3,        // void()
    RenderHookViewport = 4,          // void(void* manager, bool)
    RenderHookImagespace = 5,        // void()
    RenderHookEffects = 6,           // void(void*, uint32_t, uint32_t, uint32_t, uint32_t)
    RenderHookTaa = 7,               // bool(void* effect)
    RenderHookCount = 8
};

// Runtime exports Rust resolved by name, null when absent. C++ keeps them typed for its callbacks.
typedef struct NativeRenderExports {
    void* upscale_auxiliaries;
    void* restore_auxiliaries;
    void* reduced_frame;
    void* capture_texture;
    void* capture_requested;
} NativeRenderExports;

RSF_FALLOUT4_RENDER_ASSERT(sizeof(NativeRenderExports) == 5 * sizeof(void*));

// Render receipt sink and bounded diagnostic.
void rsf_fallout4_render_report(const char*) RSF_FALLOUT4_RENDER_NOEXCEPT;
void rsf_fallout4_render_diagnostic(const char*) RSF_FALLOUT4_RENDER_NOEXCEPT;

// The typed detours and trampoline slots stay on the native side.
bool rsf_fallout4_render_create_hook(uint32_t hook, void* target) RSF_FALLOUT4_RENDER_NOEXCEPT;
bool rsf_fallout4_render_remove_hook(uint32_t hook, void* target) RSF_FALLOUT4_RENDER_NOEXCEPT;
bool rsf_fallout4_render_scopes_busy(void) RSF_FALLOUT4_RENDER_NOEXCEPT;
void rsf_fallout4_render_bind_exports(const NativeRenderExports*) RSF_FALLOUT4_RENDER_NOEXCEPT;
void rsf_fallout4_render_bind_host(const rsf_game_host_services*) RSF_FALLOUT4_RENDER_NOEXCEPT;

typedef struct NativeWorldAdmission {
    uintptr_t caller;
    uint64_t session;
    bool live, ordinary_world, frame_open, auxiliary_open, recovery_owned;
} NativeWorldAdmission;

typedef struct NativeRenderScopeKey {
    uint64_t frame, view, window;
    uintptr_t context, chain;
} NativeRenderScopeKey;

typedef uint32_t NativePresentMatch;
enum { PresentUnrelated = 0, PresentOwnerChanged = 1, PresentMatched = 2 };
typedef uint32_t NativePresentAdmission;
enum { PresentSourceChanged = 0, PresentRefused = 1, PresentComplete = 2 };

typedef bool (*RevalidatePresentScreen)(void) RSF_FALLOUT4_RENDER_NOEXCEPT;

RSF_FALLOUT4_RENDER_ASSERT(sizeof(NativeWorldAdmission) == 24 && offsetof(NativeWorldAdmission, live) == 16 &&
    offsetof(NativeWorldAdmission, recovery_owned) == 20);
RSF_FALLOUT4_RENDER_ASSERT(sizeof(NativeRenderScopeKey) == 40 && offsetof(NativeRenderScopeKey, context) == 24 &&
    offsetof(NativeRenderScopeKey, chain) == 32);

// A fresh snapshot of the calling thread's World. Pass pointers name that World's fields.
typedef struct NativeWorld {
    rsf_fallout4_cpu_identity cpu;
    rsf_game_render_pass* pass;
    rsf_game_render_pass* frame_pass;
    rsf_game_render_pass* submission_pass;
    rsf_game_render_pass* screen_basis;
    void* context;
    void* chain;
    bool upscale;
    bool recovery_exported;
    bool in_imagespace;
    bool inputs;
    bool published;
    bool window_open;
    bool frame_open;
    bool submission_open;
    bool refused;
    bool raster_refused;
    bool auxiliary_open;
    bool depth_valid;
    bool raster_valid;
    bool depth_interrupted;
    bool full_output_resolved;
} NativeWorld;
RSF_FALLOUT4_RENDER_ASSERT(sizeof(NativeWorld) == 104 && offsetof(NativeWorld, pass) == 40 &&
    offsetof(NativeWorld, context) == 72 && offsetof(NativeWorld, upscale) == 88 &&
    offsetof(NativeWorld, full_output_resolved) == 102);

// Reduced-render recovery. Rust owns the Runtime reduced-frame token, its verdicts, the surface
// plan, the reserved output owner, the receipt budgets and the owned-token count. C++ keeps the
// typed raster reads, COM identity, the held and candidate leases, the guarded Runtime call and
// the World flags. Every leaf acts on the calling thread's World.
enum { RecoverySurfaceCount = 5 };
enum { RecoveryLeaseHeld = 0, RecoveryLeaseCandidate = 1 };
typedef struct NativeRecoveryLease {
    const rsf_game_render_pass* pass;
    void* context;
    // Depth, motion, Pip-Boy, VATS and refraction normals, in surface order.
    void* textures[RecoverySurfaceCount];
} NativeRecoveryLease;
typedef struct NativeRecoveryLeaves {
    void (*report)(const char*) RSF_FALLOUT4_RENDER_NOEXCEPT;
    void (*refuse)(void) RSF_FALLOUT4_RENDER_NOEXCEPT;
    void (*facts)(NativeWorld*) RSF_FALLOUT4_RENDER_NOEXCEPT;
    bool (*same_source)(void) RSF_FALLOUT4_RENDER_NOEXCEPT;
    // FINISH verdict: World full-output and temporal publication flags.
    void (*resolve)(bool full_output, bool published) RSF_FALLOUT4_RENDER_NOEXCEPT;
    // Replace the candidate with a fresh raster lease of the FRAME pass; producer requires the
    // valid same-frame raster snapshot.
    bool (*read_raster)(bool producer) RSF_FALLOUT4_RENDER_NOEXCEPT;
    // Candidate and held name the same colour, depth, motion, Pip-Boy, VATS and refraction
    // resources; owners adds Context and chain.
    bool (*same_resources)(bool owners) RSF_FALLOUT4_RENDER_NOEXCEPT;
    // Move the candidate into the held lease, first replacing its render_rect when nonnull.
    void (*adopt)(const int32_t* render_rect) RSF_FALLOUT4_RENDER_NOEXCEPT;
    void (*discard)(void) RSF_FALLOUT4_RENDER_NOEXCEPT; // candidate
    void (*release)(void) RSF_FALLOUT4_RENDER_NOEXCEPT; // held
    void (*lease)(uint32_t slot, NativeRecoveryLease*) RSF_FALLOUT4_RENDER_NOEXCEPT;
    bool (*preflight)(const rsf_game_render_pass*, void* context, NativeOutputOwner*) RSF_FALLOUT4_RENDER_NOEXCEPT;
    bool (*raster_current)(void) RSF_FALLOUT4_RENDER_NOEXCEPT;
    // Borrow the full-output state for the held pass on the raster snapshot.
    bool (*begin_output)(void) RSF_FALLOUT4_RENDER_NOEXCEPT;
    // Call the Runtime on the held Context with the held pass as owner. False without the export
    // or a held Context; a throw answers RSF_ERROR_NOT_READY.
    bool (*reduce)(rsf_d3d11_reduced_frame*, int32_t* result) RSF_FALLOUT4_RENDER_NOEXCEPT;
} NativeRecoveryLeaves;

RSF_FALLOUT4_RENDER_ASSERT(sizeof(NativeRecoveryLease) == 7 * sizeof(void*));
RSF_FALLOUT4_RENDER_ASSERT(sizeof(NativeRecoveryLeaves) == 15 * sizeof(void*));

// SR and HUDless publication (render_hooks/publication.rs). Rust owns the admission, refusals,
// pass identity and roles, the SR-off decision, the screen verdict, the callback order and the
// output resolution calls. C++ keeps the World fields, SR references and guarded
// Runtime calls. Rust owns the HUDless framebuffer through the WINDOW handoff.
typedef struct NativePublicationInputs {
    const rsf_game_render_pass* pass;
    void* context;
    void* chain;
} NativePublicationInputs;
typedef struct NativeAuxiliaryInputs {
    void* textures[5];
    float raster[4];
} NativeAuxiliaryInputs;
RSF_FALLOUT4_RENDER_ASSERT(sizeof(NativeAuxiliaryInputs) == 56 && offsetof(NativeAuxiliaryInputs, raster) == 40);
typedef struct NativePublicationLeaves {
    void (*world)(NativeWorld*) RSF_FALLOUT4_RENDER_NOEXCEPT;
    // The shared bounded diagnostic receipt.
    void (*diagnostic)(const char*) RSF_FALLOUT4_RENDER_NOEXCEPT;
    // Replace the SR input lease with a fresh producer read on the depth snapshot.
    bool (*read_inputs)(void) RSF_FALLOUT4_RENDER_NOEXCEPT;
    void (*inputs)(NativePublicationInputs*) RSF_FALLOUT4_RENDER_NOEXCEPT;
    void (*release_inputs)(void) RSF_FALLOUT4_RENDER_NOEXCEPT;
    // The SR input lease names the held recovery colour, depth, motion, Pip-Boy, VATS and
    // refraction resources.
    bool (*same_recovery_resources)(void) RSF_FALLOUT4_RENDER_NOEXCEPT;
    bool (*depth_domains)(rsf_game_render_pass*) RSF_FALLOUT4_RENDER_NOEXCEPT;
    // RECONFIGURE planning on the input Context; false when it threw.
    bool (*plan)(const rsf_game_render_pass*) RSF_FALLOUT4_RENDER_NOEXCEPT;
    // 1 read, 0 unbound or refused, -1 threw.
    int32_t (*render_config)(rsf_game_render_config*) RSF_FALLOUT4_RENDER_NOEXCEPT;
    // Keep the input colour, depth and motion leases and record the screen basis.
    void (*retain_inputs)(const rsf_game_render_pass* basis) RSF_FALLOUT4_RENDER_NOEXCEPT;
    // Classify against the screen basis; first records the leased screen.
    void (*classify)(bool first, NativeScreen* current, NativeScreen* leased) RSF_FALLOUT4_RENDER_NOEXCEPT;
    void (*notify)(const rsf_game_render_pass*, void* context, uint32_t begin) RSF_FALLOUT4_RENDER_NOEXCEPT;
    // World pass from the scalar pass, Context and chain from the input lease, inputs set.
    void (*publish_inputs)(const rsf_game_render_pass* scalar) RSF_FALLOUT4_RENDER_NOEXCEPT;
    bool (*begin_output)(const rsf_game_render_pass*) RSF_FALLOUT4_RENDER_NOEXCEPT;
    void (*report_state)(uint32_t stage, const rsf_game_render_pass*, void* context, bool published,
        int32_t result) RSF_FALLOUT4_RENDER_NOEXCEPT;
    // Evaluate the input lease; a throw drains auxiliaries and answers RSF_ERROR_NOT_READY.
    bool (*auxiliary_inputs)(NativeAuxiliaryInputs*) RSF_FALLOUT4_RENDER_NOEXCEPT;
    bool (*auxiliary_colour)(void*) RSF_FALLOUT4_RENDER_NOEXCEPT;
    bool (*same_texture)(void*, void*) RSF_FALLOUT4_RENDER_NOEXCEPT;
    int32_t (*reconstruct)(const rsf_game_render_pass*, const rsf_d3d11_auxiliary_surface*, uint32_t) RSF_FALLOUT4_RENDER_NOEXCEPT;
    bool (*end_output)(void) RSF_FALLOUT4_RENDER_NOEXCEPT;
    // Retain the exact acquired texture and open WINDOW.
    void (*publish_window)(void* texture, const rsf_game_render_pass* window) RSF_FALLOUT4_RENDER_NOEXCEPT;
} NativePublicationLeaves;
RSF_FALLOUT4_RENDER_ASSERT(sizeof(NativePublicationInputs) == 3 * sizeof(void*));
RSF_FALLOUT4_RENDER_ASSERT(sizeof(NativePublicationLeaves) == 21 * sizeof(void*));

// FRAME, SUBMISSION and WINDOW scope (render_hooks/scope.rs). Rust owns the before/after owner
// lease verdict, the FRAME pass identity and keys, FRAME then SUBMISSION begin, the balanced
// SUBMISSION close before Present or after Swap, NO_FG on refusal, the WINDOW then FRAME close
// order, the recovery, auxiliary and resolution continuation and the conditional World reset.
// C++ keeps the typed owner read, COM identity and the Context and chain leases, the scope
// counters, the guarded auxiliary and resolution calls, the guarded pass callback and receipts and
// the World fields. Rust also uses the recovery leaves' report, refuse and same_source and retires
// recovery through them.
typedef struct NativeRenderOwner {
    uintptr_t renderer, device, context, chain, window, graph, view;
    uint32_t frame;
} NativeRenderOwner;
bool rsf_fallout4_render_read_owner(NativeRenderOwner*) RSF_FALLOUT4_RENDER_NOEXCEPT;
bool rsf_fallout4_render_read_manager(uintptr_t*) RSF_FALLOUT4_RENDER_NOEXCEPT;
bool rsf_fallout4_render_same_object(void*, void*) RSF_FALLOUT4_RENDER_NOEXCEPT;
bool rsf_fallout4_render_check_owner(void*, void*, uintptr_t device, uintptr_t window) RSF_FALLOUT4_RENDER_NOEXCEPT;
bool rsf_fallout4_render_auxiliary_depth(void*) RSF_FALLOUT4_RENDER_NOEXCEPT;
bool rsf_fallout4_render_auxiliary_colour(void*, void*, uint32_t width, uint32_t height) RSF_FALLOUT4_RENDER_NOEXCEPT;

typedef struct NativeScopeLeaves {
    void (*world)(NativeWorld*) RSF_FALLOUT4_RENDER_NOEXCEPT;
    // Renderer cell, RendererData device, Context, window and chain, scene graph view, frame
    // counter and accepted window; false when any is unreadable, null or not the renderer.
    bool (*read_owner)(NativeRenderOwner*) RSF_FALLOUT4_RENDER_NOEXCEPT;
    // Lease the owner's Context and chain as the candidate, then require an immediate Context on
    // the owner device, the chain on that device and the chain output window.
    bool (*lease_owner)(const NativeRenderOwner*) RSF_FALLOUT4_RENDER_NOEXCEPT;
    // Move the candidate into the World Context and chain.
    void (*adopt_owner)(void) RSF_FALLOUT4_RENDER_NOEXCEPT;
    void (*discard_owner)(void) RSF_FALLOUT4_RENDER_NOEXCEPT;
    // FRAME open with its counter; SUBMISSION open.
    void (*open_frame)(void) RSF_FALLOUT4_RENDER_NOEXCEPT;
    void (*open_submission)(void) RSF_FALLOUT4_RENDER_NOEXCEPT;
    // Clear the open flag, and the WINDOW or FRAME counter.
    void (*close_submission)(void) RSF_FALLOUT4_RENDER_NOEXCEPT;
    void (*close_window)(void) RSF_FALLOUT4_RENDER_NOEXCEPT;
    void (*close_frame)(void) RSF_FALLOUT4_RENDER_NOEXCEPT;
    // The guarded auxiliary restore; true when none is open or it drained.
    bool (*drain_auxiliaries)(void) RSF_FALLOUT4_RENDER_NOEXCEPT;
    bool (*finish_resolution)(void) RSF_FALLOUT4_RENDER_NOEXCEPT;
    // World = .
    void (*reset)(void) RSF_FALLOUT4_RENDER_NOEXCEPT;
    // The publication leaves' notify, diagnostic and report_state.
    void (*notify)(const rsf_game_render_pass*, void* context, uint32_t begin) RSF_FALLOUT4_RENDER_NOEXCEPT;
    void (*diagnostic)(const char*) RSF_FALLOUT4_RENDER_NOEXCEPT;
    void (*report_state)(uint32_t stage, const rsf_game_render_pass*, void* context, bool published,
        int32_t result) RSF_FALLOUT4_RENDER_NOEXCEPT;
} NativeScopeLeaves;

RSF_FALLOUT4_RENDER_ASSERT(sizeof(NativeRenderOwner) == 64 && offsetof(NativeRenderOwner, frame) == 56);
RSF_FALLOUT4_RENDER_ASSERT(sizeof(NativeScopeLeaves) == 16 * sizeof(void*));

// Imagespace, Effects and TAA seams (render_hooks/imagespace.rs). Rust owns the verified caller
// and argument selection, the imagespace entry, the before and after Effects sequence around the
// SR seam, the imagespace exit order of recovery RETIRE, auxiliary drain and output resolution
// with its receipts and refusals, and the TAA exclusion predicate with its bounded receipt. C++
// keeps the typed detours, the original calls, the return addresses, the LastError saves and
// restores, the typed native manager read, the guarded auxiliary and resolution calls, the World
// fields and the scope guard that runs the imagespace exit when the original returns or unwinds.
// Rust also uses the recovery leaves' report, refuse, same_source and raster_current, and the
// publication leaves for the SR seam.
typedef struct NativeEffectsCall {
    uintptr_t caller, manager;
    uint32_t first, last, mode, flags;
} NativeEffectsCall;
typedef struct NativeImagespaceLeaves {
    void (*world)(NativeWorld*) RSF_FALLOUT4_RENDER_NOEXCEPT;
    // World inside the selected imagespace.
    void (*enter)(void) RSF_FALLOUT4_RENDER_NOEXCEPT;
    // Clear the World's imagespace, temporal publication and full-output flags.
    void (*leave)(void) RSF_FALLOUT4_RENDER_NOEXCEPT;
    // The native imagespace manager cell; false when unreadable.
    bool (*manager)(uintptr_t*) RSF_FALLOUT4_RENDER_NOEXCEPT;
    // The guarded auxiliary restore; true when none is open or it drained.
    bool (*drain_auxiliaries)(void) RSF_FALLOUT4_RENDER_NOEXCEPT;
    bool (*end_output)(void) RSF_FALLOUT4_RENDER_NOEXCEPT;
    // The publication leaves' report_state and diagnostic.
    void (*report_state)(uint32_t stage, const rsf_game_render_pass*, void* context, bool published,
        int32_t result) RSF_FALLOUT4_RENDER_NOEXCEPT;
    void (*diagnostic)(const char*) RSF_FALLOUT4_RENDER_NOEXCEPT;
} NativeImagespaceLeaves;

RSF_FALLOUT4_RENDER_ASSERT(sizeof(NativeEffectsCall) == 32 && offsetof(NativeEffectsCall, first) == 16 &&
    offsetof(NativeEffectsCall, flags) == 28);
RSF_FALLOUT4_RENDER_ASSERT(sizeof(NativeImagespaceLeaves) == 8 * sizeof(void*));

// Viewport selector, dynamic-resolution writer and depth prepass seams (render_hooks/producer.rs).
// Rust owns the native-target and verified-caller selection, the owned full-output selector write
// after the native viewport intent and its refusal, the HUDless receipt and seam, the depth
// invalidation before DR, the overlapping DR refusal and transfer, the DR configuration admission
// on the screen and leased owner with its refusal, the nested prepass selection, invalidation and
// interruption, and the same-source raster and depth capture with its refusal. C++ keeps the typed
// detours, the original calls, the return addresses, the LastError saves and restores, the nested
// prepass counter and its guard, the typed screen, owner, resolution and depth reads, and the
// World fields and snapshots. Rust also uses the recovery leaves' refuse and same_source, and the
// publication leaves for the HUDless seam.
// The World source before the original prepass and its selection, held across the original.
typedef struct NativePrepassCall {
    rsf_fallout4_cpu_identity identity;
    bool selected;
} NativePrepassCall;
typedef struct NativeProducerLeaves {
    void (*world)(NativeWorld*) RSF_FALLOUT4_RENDER_NOEXCEPT;
    // Clear the depth snapshot, and the raster snapshot when raster is set.
    void (*invalidate)(bool raster) RSF_FALLOUT4_RENDER_NOEXCEPT;
    void (*interrupt_depth)(bool interrupted) RSF_FALLOUT4_RENDER_NOEXCEPT;
    // World raster and source refusal.
    void (*refuse_raster)(void) RSF_FALLOUT4_RENDER_NOEXCEPT;
    // native_resolution_write after the complete original DR writer.
    bool (*native_dr)(void) RSF_FALLOUT4_RENDER_NOEXCEPT;
    // classify_native_screen refusals of the World source on an empty pass in the ordinary World.
    uint32_t (*screen_refusals)(void) RSF_FALLOUT4_RENDER_NOEXCEPT;
    // The scope leaves' typed owner read.
    bool (*read_owner)(NativeRenderOwner*) RSF_FALLOUT4_RENDER_NOEXCEPT;
    // configure_native_resolution on the FRAME pass and World Context with the recovery reserve.
    bool (*configure)(void) RSF_FALLOUT4_RENDER_NOEXCEPT;
    // Raster capture into the World raster snapshot; depth capture into the caller's snapshot.
    bool (*capture_raster)(const rsf_fallout4_cpu_identity*) RSF_FALLOUT4_RENDER_NOEXCEPT;
    bool (*capture_depth)(const rsf_fallout4_cpu_identity*, NativeDepthSnapshot*) RSF_FALLOUT4_RENDER_NOEXCEPT;
    // Copy into the World depth snapshot.
    void (*adopt_depth)(const NativeDepthSnapshot*) RSF_FALLOUT4_RENDER_NOEXCEPT;
    // The publication leaves' report_state and diagnostic.
    void (*report_state)(uint32_t stage, const rsf_game_render_pass*, void* context, bool published,
        int32_t result) RSF_FALLOUT4_RENDER_NOEXCEPT;
    void (*diagnostic)(const char*) RSF_FALLOUT4_RENDER_NOEXCEPT;
} NativeProducerLeaves;

RSF_FALLOUT4_RENDER_ASSERT(sizeof(NativePrepassCall) == 48 && offsetof(NativePrepassCall, selected) == 40);
RSF_FALLOUT4_RENDER_ASSERT(sizeof(NativeProducerLeaves) == 13 * sizeof(void*));

// The adapter's render callbacks are live: set by start once every hook is queued, cleared by
// quiesce, stop and a failed start.
bool rsf_fallout4_render_live(void) RSF_FALLOUT4_RENDER_NOEXCEPT;
bool rsf_fallout4_render_admit_world(const NativeWorldAdmission*, const rsf_fallout4_cpu_identity*) RSF_FALLOUT4_RENDER_NOEXCEPT;
NativePresentMatch rsf_fallout4_render_match_present(uintptr_t caller, uintptr_t renderer,
    bool submission_open, const NativeRenderScopeKey*) RSF_FALLOUT4_RENDER_NOEXCEPT;
NativePresentAdmission rsf_fallout4_render_admit_present(NativePresentMatch, uint64_t session,
    const rsf_fallout4_cpu_identity*, RevalidatePresentScreen) RSF_FALLOUT4_RENDER_NOEXCEPT;
// Preflight, RESERVE and confirmation before the first native ratio write.
bool rsf_fallout4_recovery_reserve(const NativeRecoveryLeaves*, const rsf_game_render_pass* planned) RSF_FALLOUT4_RENDER_NOEXCEPT;
// The calling thread's token, zero when none is owned. The SR seam SEALs before any temporal
// reader and FINISHes as it returns; the imagespace and Effects exits RETIRE in Rust.
uint64_t rsf_fallout4_recovery_token(void) RSF_FALLOUT4_RENDER_NOEXCEPT;
void rsf_fallout4_render_capture_recovery(const NativeDepthSnapshot*, void (*facts)(NativeWorld*) RSF_FALLOUT4_RENDER_NOEXCEPT,
    void (*report)(const char*), bool (*requested)(void) RSF_FALLOUT4_RENDER_NOEXCEPT,
    void (*capture)(void*, const rsf_d3d11_host_capture*) RSF_FALLOUT4_RENDER_NOEXCEPT) RSF_FALLOUT4_RENDER_NOEXCEPT;
// Clear with the World; called only where the World resets without a token.
void rsf_fallout4_recovery_reset(void) RSF_FALLOUT4_RENDER_NOEXCEPT;
// Select a verified imagespace call and enter imagespace before the original. True asks the
// caller to run the exit after the original returns or unwinds.
bool rsf_fallout4_render_enter_imagespace(const NativeImagespaceLeaves*, const NativeRecoveryLeaves*,
    uintptr_t caller) RSF_FALLOUT4_RENDER_NOEXCEPT;
// The selected imagespace exit: RETIRE, auxiliary drain, output resolution end and leave.
void rsf_fallout4_render_leave_imagespace(const NativeImagespaceLeaves*, const NativeRecoveryLeaves*) RSF_FALLOUT4_RENDER_NOEXCEPT;
// Before the original Effects range: the pre-SR receipt and the SR seam on the verified call.
bool rsf_fallout4_render_before_effects(const NativeImagespaceLeaves*, const NativePublicationLeaves*,
    const NativeRecoveryLeaves*, const NativeEffectsCall*, rsf_fallout4_cpu_identity*) RSF_FALLOUT4_RENDER_NOEXCEPT;
// After the original Effects range: the Effects receipt, RETIRE and auxiliary drain.
void rsf_fallout4_render_after_effects(const NativeImagespaceLeaves*, const NativeRecoveryLeaves*,
    const rsf_fallout4_cpu_identity*) RSF_FALLOUT4_RENDER_NOEXCEPT;
// The TAA active answer from the original answer.
bool rsf_fallout4_render_gate_taa(const NativeImagespaceLeaves*, const NativeRecoveryLeaves*,
    bool original) RSF_FALLOUT4_RENDER_NOEXCEPT;
// After the original viewport selector store: the owned full-output selector write on the native
// targets, then the HUDless receipt and seam after the live HUDless selection.
void rsf_fallout4_render_after_viewport(const NativeProducerLeaves*, const NativePublicationLeaves*,
    const NativeRecoveryLeaves*, uintptr_t caller, uintptr_t manager, bool enabled) RSF_FALLOUT4_RENDER_NOEXCEPT;
// Before the original DR writer: depth invalidation on an open FRAME.
void rsf_fallout4_render_before_dynamic_resolution(const NativeProducerLeaves*) RSF_FALLOUT4_RENDER_NOEXCEPT;
// After the original DR writer: the overlap refusal and transfer, then the configuration admission.
void rsf_fallout4_render_after_dynamic_resolution(const NativeProducerLeaves*, const NativeRecoveryLeaves*,
    uintptr_t caller, uintptr_t manager) RSF_FALLOUT4_RENDER_NOEXCEPT;
// Before the original prepass, at its nested depth: selection, invalidation and interruption.
void rsf_fallout4_render_before_prepass(const NativeProducerLeaves*, const NativeRecoveryLeaves*,
    uintptr_t caller, uint32_t depth, NativePrepassCall*) RSF_FALLOUT4_RENDER_NOEXCEPT;
// After the original prepass: the same-source raster and depth capture of the selected call.
void rsf_fallout4_render_after_prepass(const NativeProducerLeaves*, const NativeRecoveryLeaves*,
    const NativePrepassCall*) RSF_FALLOUT4_RENDER_NOEXCEPT;
// The screen verdict on a World pass.
void rsf_fallout4_render_screen_facts(const NativePublicationLeaves*, const NativeRecoveryLeaves*,
    rsf_game_render_pass*, bool first) RSF_FALLOUT4_RENDER_NOEXCEPT;
// FRAME then SUBMISSION before the original ordinary WorldBegin, on the World just reset and
// bound to the admitted source. A refused owner lease refuses the World with its diagnostic.
void rsf_fallout4_render_begin_scope(const NativeScopeLeaves*, const NativeRecoveryLeaves*) RSF_FALLOUT4_RENDER_NOEXCEPT;
// The SUBMISSION end of an admitted complete outer Present.
void rsf_fallout4_render_close_submission(const NativeScopeLeaves*) RSF_FALLOUT4_RENDER_NOEXCEPT;
// After the original Swap: balance a withheld SUBMISSION, end WINDOW then FRAME, retire recovery,
// drain auxiliaries, finish the resolution source and reset the World when nothing is owned.
void rsf_fallout4_render_finish_scope(const NativeScopeLeaves*, const NativeRecoveryLeaves*) RSF_FALLOUT4_RENDER_NOEXCEPT;
RSF_FALLOUT4_RENDER_ABI_END
