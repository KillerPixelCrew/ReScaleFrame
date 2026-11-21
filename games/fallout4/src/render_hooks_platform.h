// SPDX-License-Identifier: MIT
#pragma once
// Private boundary between render_hooks.cpp and its Rust owner (render_hooks.rs): the render hook
// lifecycle, the ordinary World and outer Present guards, the reduced-render recovery
// transaction (render_hooks/recovery.rs) and SR and HUDless publication
// (render_hooks/publication.rs). Not a DLL ABI.
#include "cpu_events_platform.h"
#include "resolution.h"
#include <cstddef>

struct rsf_d3d11_reduced_frame;

namespace rsf::fallout4 {
// Detours in creation and enable order. Rust owns each record's RVA, expected entry bytes and
// created state; render_hooks.cpp owns the typed detour and its typed trampoline.
enum NativeRenderHook : uint32_t {
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
struct NativeRenderExports {
    void* upscale_auxiliaries;
    void* restore_auxiliaries;
    void* reduced_frame;
    void* capture_texture;
    void* capture_requested;
};

// Leaves the Rust lifecycle cannot express without MinHook or the C++ callback state. Constant
// for the process. Original engine functions are never called from Rust.
struct NativeRenderHookLeaves {
    // MH_CreateHook with the hook's detour, writing its typed trampoline.
    bool (*create_hook)(uint32_t hook, void* target) noexcept;
    // MH_RemoveHook; clears the typed trampoline only when removal succeeded.
    bool (*remove_hook)(uint32_t hook, void* target) noexcept;
    // MH_QueueEnableHook, MH_QueueDisableHook and MH_ApplyQueued, each true only for MH_OK.
    bool (*queue_enable_hook)(void* target) noexcept;
    bool (*queue_disable_hook)(void* target) noexcept;
    bool (*apply_queued)() noexcept;
    // Any detour in flight or any WINDOW, FRAME or auxiliary scope open. Rust adds owned
    // recovery tokens and the resolution owner.
    bool (*scopes_busy)() noexcept;
    // Replace the callbacks' Runtime exports and host services.
    void (*bind_exports)(const NativeRenderExports*) noexcept;
    void (*bind_host)(const rsf_game_host_services*) noexcept;
};

static_assert(sizeof(NativeRenderExports) == 5 * sizeof(void*));
static_assert(sizeof(NativeRenderHookLeaves) == 8 * sizeof(void*));

// The process-lived leaf table of render_hooks.cpp.
const NativeRenderHookLeaves* render_hook_leaves() noexcept;

struct NativeWorldAdmission {
    uintptr_t caller;
    uint64_t session;
    bool live, ordinary_world, frame_open, auxiliary_open, recovery_owned;
};

struct NativeRenderScopeKey {
    uint64_t frame, view, window;
    uintptr_t context, chain;
};

enum class NativePresentMatch : uint32_t { unrelated = 0, owner_changed = 1, matched = 2 };
enum class NativePresentAdmission : uint32_t { source_changed = 0, refused = 1, complete = 2 };

using ReadPresentOwner = bool (*)(NativeRenderScopeKey*, uintptr_t* outer_chain) noexcept;
using RevalidatePresentScreen = bool (*)() noexcept;

static_assert(sizeof(NativeWorldAdmission) == 24 && offsetof(NativeWorldAdmission, live) == 16 &&
    offsetof(NativeWorldAdmission, recovery_owned) == 20);
static_assert(sizeof(NativeRenderScopeKey) == 40 && offsetof(NativeRenderScopeKey, context) == 24 &&
    offsetof(NativeRenderScopeKey, chain) == 32);

// Reduced-render recovery. Rust owns the Runtime reduced-frame token, its verdicts, the surface
// plan, the reserved output owner, the receipt budgets and the owned-token count. C++ keeps the
// typed raster reads, COM identity, the held and candidate leases, the guarded Runtime call and
// the World flags. Every leaf acts on the calling thread's World.
constexpr uint32_t RecoverySurfaceCount = 5;
enum NativeRecoveryLeaseSlot : uint32_t { RecoveryLeaseHeld = 0, RecoveryLeaseCandidate = 1 };
struct NativeRecoveryFacts {
    void* context;          // World Context
    uint64_t source;        // World CPU source
    bool exported;          // rsf_d3d11_host_reduced_frame is bound
    bool frame_open;
    bool full_output_resolved;
};
struct NativeRecoveryLease {
    const rsf_game_render_pass* pass;
    void* context;
    // Depth, motion, Pip-Boy, VATS and refraction normals, in surface order.
    void* textures[RecoverySurfaceCount];
};
struct NativeRecoveryLeaves {
    void (*report)(const char*) noexcept;
    void (*refuse)() noexcept;
    void (*facts)(NativeRecoveryFacts*) noexcept;
    bool (*same_source)() noexcept;
    // FINISH verdict: World full-output and temporal publication flags.
    void (*resolve)(bool full_output, bool published) noexcept;
    // Replace the candidate with a fresh raster lease of the FRAME pass; producer requires the
    // valid same-frame raster snapshot.
    bool (*read_raster)(bool producer) noexcept;
    // Candidate and held name the same colour, depth, motion, Pip-Boy, VATS and refraction
    // resources; owners adds Context and chain.
    bool (*same_resources)(bool owners) noexcept;
    // Move the candidate into the held lease, first replacing its render_rect when nonnull.
    void (*adopt)(const int32_t* render_rect) noexcept;
    void (*discard)() noexcept; // candidate
    void (*release)() noexcept; // held
    void (*lease)(uint32_t slot, NativeRecoveryLease*) noexcept;
    bool (*preflight)(const rsf_game_render_pass*, void* context, NativeOutputOwner*) noexcept;
    bool (*raster_current)() noexcept;
    // Borrow the full-output state for the held pass on the raster snapshot.
    bool (*begin_output)() noexcept;
    // Call the Runtime on the held Context with the held pass as owner. False without the export
    // or a held Context; a throw answers RSF_ERROR_NOT_READY.
    bool (*reduce)(rsf_d3d11_reduced_frame*, int32_t* result) noexcept;
};

static_assert(sizeof(NativeRecoveryFacts) == 24 && offsetof(NativeRecoveryFacts, exported) == 16);
static_assert(sizeof(NativeRecoveryLease) == 7 * sizeof(void*));
static_assert(sizeof(NativeRecoveryLeaves) == 15 * sizeof(void*));

// SR and HUDless publication (render_hooks/publication.rs). Rust owns the admission, refusals,
// pass identity and roles, the SR-off decision, the screen verdict, the callback order and the
// output resolution calls. C++ keeps the typed native reads, COM identity, the SR input and
// HUDless framebuffer leases, the engine depth and screen readers, the guarded Runtime calls and
// the World fields. Rust also uses the recovery leaves' report, refuse, same_source and resolve.
struct NativeScreen;
struct NativePublicationWorld {
    rsf_fallout4_cpu_identity cpu;
    const rsf_game_render_pass* pass;          // World pass
    const rsf_game_render_pass* frame_pass;
    const rsf_game_render_pass* screen_basis;
    void* context;                             // World Context and outer chain
    void* chain;
    bool upscale;                              // process_upscale is bound
    bool in_imagespace, inputs, window_open, frame_open, submission_open;
    bool refused, raster_refused, auxiliary_open;
    bool depth_valid;                          // same-source prepass depth snapshot
};
struct NativePublicationInputs {
    const rsf_game_render_pass* pass;
    void* context;
    void* chain;
};
struct NativePublicationFramebuffer {
    void* context;
    void* chain;
    void* texture;
    uint64_t native_frame, window_key;
};
struct NativePublicationLeaves {
    void (*world)(NativePublicationWorld*) noexcept;
    // The shared bounded diagnostic receipt.
    void (*diagnostic)(const char*) noexcept;
    // Replace the SR input lease with a fresh producer read on the depth snapshot.
    bool (*read_inputs)() noexcept;
    void (*inputs)(NativePublicationInputs*) noexcept;
    void (*release_inputs)() noexcept;
    // The SR input lease names the held recovery colour, depth, motion, Pip-Boy, VATS and
    // refraction resources.
    bool (*same_recovery_resources)() noexcept;
    bool (*depth_domains)(rsf_game_render_pass*) noexcept;
    // RECONFIGURE planning on the input Context; false when it threw.
    bool (*plan)(const rsf_game_render_pass*) noexcept;
    // 1 read, 0 unbound or refused, -1 threw.
    int32_t (*render_config)(rsf_game_render_config*) noexcept;
    // Keep the input colour, depth and motion leases and record the screen basis.
    void (*retain_inputs)(const rsf_game_render_pass* basis) noexcept;
    // Classify against the screen basis; first records the leased screen.
    void (*classify)(bool first, NativeScreen* current, NativeScreen* leased) noexcept;
    void (*notify)(const rsf_game_render_pass*, void* context, uint32_t begin) noexcept;
    // World pass from the scalar pass, Context and chain from the input lease, inputs set.
    void (*publish_inputs)(const rsf_game_render_pass* scalar) noexcept;
    bool (*begin_output)(const rsf_game_render_pass*) noexcept;
    void (*report_state)(uint32_t stage, const rsf_game_render_pass*, void* context, bool published,
        int32_t result) noexcept;
    // Evaluate the input lease; a throw drains auxiliaries and answers RSF_ERROR_NOT_READY.
    int32_t (*reconstruct)(const rsf_game_render_pass*) noexcept;
    bool (*end_output)() noexcept;
    // Replace the framebuffer lease with a fresh logical framebuffer read.
    bool (*read_framebuffer)() noexcept;
    void (*framebuffer)(NativePublicationFramebuffer*) noexcept;
    // Width, height and sample count of the framebuffer texture.
    void (*framebuffer_extent)(uint32_t extent[3]) noexcept;
    void (*release_framebuffer)() noexcept;
    // Final colour lease, World pass and open WINDOW.
    void (*publish_window)(const rsf_game_render_pass* window) noexcept;
};
enum class NativeSrOutcome : uint32_t { refused = 0, published = 1, failed = 2 };

static_assert(sizeof(NativePublicationWorld) == 96 && offsetof(NativePublicationWorld, pass) == 40 &&
    offsetof(NativePublicationWorld, upscale) == 80 && offsetof(NativePublicationWorld, depth_valid) == 89);
static_assert(sizeof(NativePublicationInputs) == 3 * sizeof(void*));
static_assert(sizeof(NativePublicationFramebuffer) == 40 &&
    offsetof(NativePublicationFramebuffer, native_frame) == 24);
static_assert(sizeof(NativePublicationLeaves) == 22 * sizeof(void*));

extern "C" {
// Binds the leaves. The table must be complete and live for the process.
bool rsf_fallout4_render_publish(const NativeRenderHookLeaves*) noexcept;
// The adapter's render callbacks are live: set by start once every hook is queued, cleared by
// quiesce, stop and a failed start.
bool rsf_fallout4_render_live() noexcept;
bool rsf_fallout4_render_admit_world(const NativeWorldAdmission*, const rsf_fallout4_cpu_identity*) noexcept;
NativePresentMatch rsf_fallout4_render_match_present(uintptr_t caller, uintptr_t renderer,
    bool submission_open, const NativeRenderScopeKey*, ReadPresentOwner) noexcept;
NativePresentAdmission rsf_fallout4_render_admit_present(NativePresentMatch, uint64_t session,
    const rsf_fallout4_cpu_identity*, RevalidatePresentScreen) noexcept;
// Preflight, RESERVE and confirmation before the first native ratio write.
bool rsf_fallout4_recovery_reserve(const NativeRecoveryLeaves*, const rsf_game_render_pass* planned) noexcept;
// The SR seam (rsf_fallout4_render_emit_sr) SEALs before any temporal reader and FINISHes as it
// returns. CANCEL an uncommitted token, or FINISH then RETIRE a committed one.
bool rsf_fallout4_recovery_retire(const NativeRecoveryLeaves*) noexcept;
uint64_t rsf_fallout4_recovery_token() noexcept;
bool rsf_fallout4_recovery_committed() noexcept;
// Clear with the World; called only where the World resets without a token.
void rsf_fallout4_recovery_reset() noexcept;
// The SR seam before the original Effects range. Failed asks the caller for its callback
// failure receipt.
NativeSrOutcome rsf_fallout4_render_emit_sr(const NativePublicationLeaves*,
    const NativeRecoveryLeaves*) noexcept;
// The HUDless seam after the native HUDless viewport selection.
void rsf_fallout4_render_emit_hudless(const NativePublicationLeaves*,
    const NativeRecoveryLeaves*) noexcept;
// The screen verdict on a World pass.
void rsf_fallout4_render_screen_facts(const NativePublicationLeaves*, const NativeRecoveryLeaves*,
    rsf_game_render_pass*, bool first) noexcept;
// Session, source, submission, scope, primary/after-simulation/NO_FG flags, unknown screen.
void rsf_fallout4_render_identify(rsf_game_render_pass*, const rsf_fallout4_cpu_identity*) noexcept;
}
}
