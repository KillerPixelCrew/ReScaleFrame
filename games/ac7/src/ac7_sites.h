// SPDX-License-Identifier: MIT
#pragma once
// C declarations only: the internal boundary between the native renderer (native_renderer.rs) and
// the owners of the exact-build code sites, the renderer transaction and the TrueSky plugin patches
// (sites.rs, patches.rs, guards.rs, renderer.rs, truesky.rs), all in the rsf-ac7 crate, and the
// export plugin.c reaches. Not a DLL ABI: everything links into ReScaleFrame.Game.AC7.dll and the
// native graph fixture. tools/generate-rust-bindings.py generates src/sites_abi.rs from this header.
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>
#include <rescaleframe/ac7_native_renderer.h>

#ifdef __cplusplus
extern "C" {
#endif

// Hooked entry sites, in the order of native_renderer.rs's hook constants. The first
// RSF_AC7_PROCESS_HOOKS are the reviewed graph pass Process functions and carry a pass role.
#define RSF_AC7_HOOK_SITE_COUNT 65u
#define RSF_AC7_PROCESS_HOOKS 6u
// The pixel uniform bind evidence hooks, inclusive. They are installed only with diagnostics.
#define RSF_AC7_EVIDENCE_FIRST 38u
#define RSF_AC7_EVIDENCE_LAST 43u
// Bytes compared at each entry and helper site.
#define RSF_AC7_SITE_BYTES 16u
#define RSF_AC7_VELOCITY_PATCHES 3u
// Hook body guards. Stop waits for entry guards before disabling hooks and for both kinds after.
#define RSF_AC7_GUARD_ENTRY 0u
#define RSF_AC7_GUARD_OUTER 1u

// Native code leaves lent to the Rust owner for one call.
typedef struct rsf_ac7_code_leaves {
    // True when `count` bytes of live code at `address` equal `expected`. Unreadable memory differs.
    bool (*matches)(const unsigned char* address, const unsigned char* expected, size_t count);
    // Replaces `count` bytes at `site` that equal `expected` under code protection and flushes the
    // instruction cache. False leaves the code unchanged.
    bool (*write)(unsigned char* site, const unsigned char* expected, const unsigned char* replacement,
        size_t count);
    rsf_ac7_renderer_log_fn log; // may be null
    void* user;
} rsf_ac7_code_leaves;

// One engine code patch: `size` bytes at `rva`, native and patched.
typedef struct rsf_ac7_code_patch {
    uint32_t rva;
    uint32_t size;
    unsigned char native[16];
    unsigned char replacement[16];
} rsf_ac7_code_patch;

// Engine patch ownership kept in the native renderer and changed only by the Rust owner.
typedef struct rsf_ac7_code_patches {
    unsigned char* velocity_module; // module the applied velocity patches belong to
    uint32_t velocity_applied;      // velocity patches applied, in table order
    unsigned char* scene_precision_site; // patched scene-precision site, or null
} rsf_ac7_code_patches;

// Exact-build admission: every hooked entry site, then the enlarged UI depth/view gates, then the
// helper sites, compared at `base`. Logs the first difference and returns false.
/* Admission uses matches and the optional logger. A writer is required only for patch calls. */
bool rsf_ac7_build_admit(const unsigned char* base, const rsf_ac7_code_leaves* leaves);
// RVA and pass role of a hooked entry site; zero out of range. Only Process sites have a role.
uint32_t rsf_ac7_hook_rva(uint32_t hook);
uint32_t rsf_ac7_hook_role(uint32_t hook);
// Whether a verified site is hooked: always, except evidence hooks without diagnostics.
bool rsf_ac7_hook_wanted(uint32_t hook, bool diagnostics);

// Per-thread hook body counting. `kind` is RSF_AC7_GUARD_*.
void rsf_ac7_guard_enter(uint32_t kind);
void rsf_ac7_guard_leave(uint32_t kind);
// Hook bodies running: entry guards, plus outer guards when `outer_too`.
uint32_t rsf_ac7_guards_held(bool outer_too);

// The temporal velocity patch at `index`; false out of range.
bool rsf_ac7_velocity_patch(uint32_t index, rsf_ac7_code_patch* out);
// All-or-nothing temporal velocity group at `base`: preflight, write, roll back on failure.
bool rsf_ac7_velocity_apply(rsf_ac7_code_patches* patches, unsigned char* base,
    const rsf_ac7_code_leaves* leaves);
// Restores owned velocity patches in reverse; false keeps the unrestored ones owned.
bool rsf_ac7_velocity_restore(rsf_ac7_code_patches* patches, const rsf_ac7_code_leaves* leaves);

// TrueSky plugin (TrueSkyPluginRender_MT.dll) patches and call relays (truesky.rs). The sites,
// expected bytes, effect fingerprint and the install, rollback and restore order are Rust's; the
// module lookup, relay memory, D3D11 state and protected writes stay native.
#define RSF_AC7_RELAY_BYTES 7u

// A `call [reg+disp]` in TrueSky's code redirected through a pointer slot within reach of its
// `call [rip+rel32]` form. Both byte sequences are kept, so the restore needs nothing else.
typedef struct rsf_ac7_call_relay {
    unsigned char* call; // the patched call, or null once its native bytes are back
    void* slot;          // relay memory holding the handler, or null once released
    unsigned char native[RSF_AC7_RELAY_BYTES];
    unsigned char relayed[RSF_AC7_RELAY_BYTES];
    uint32_t size; // 6 or 7
} rsf_ac7_call_relay;

// TrueSky patch and relay ownership kept in the native renderer. Only the Rust owner changes it,
// always under the renderer's cloud resolution lock.
typedef struct rsf_ac7_truesky_patches {
    unsigned char* resolution_site;   // cloud divisor 1, or null
    unsigned char* depth_branch;      // depth pass selector, or null
    unsigned char* depth_format_site; // RG32F depth bounds request, or null
    rsf_ac7_call_relay depth_call;    // depth producer dispatch relay
    rsf_ac7_call_relay motion_call;   // composite_tile draw relay
    bool resolution_refused;          // the full cloud grid stays off for this renderer
    bool motion_refused;              // composite_tile fingerprint refused for this renderer
} rsf_ac7_truesky_patches;

// Native leaves of the TrueSky owner, lent for one call. Every entry is required. None throws.
typedef struct rsf_ac7_truesky_leaves {
    // Compares and writes plugin code and logs to the renderer's host log.
    rsf_ac7_code_leaves code;
    rsf_ac7_native_renderer* renderer;
    // Base of the loaded module named `name` (UTF-16, terminated), or null.
    unsigned char* (*module)(const uint16_t* name);
    // Relay memory within rel32 search range of `call` holding `handler`, or null.
    void* (*relay_allocate)(unsigned char* call, void* handler);
    void (*relay_release)(void* slot);
    void* depth_dispatch; // relay handler of the depth producer dispatch
    void* composite_draw; // relay handler of the composite_tile draw
    // Whether the depth dispatch relay verified its one-to-one shader and bindings on the device.
    bool (*depth_ready)(rsf_ac7_native_renderer* renderer);
    // Releases the cloud motion depth resources.
    void (*motion_release)(rsf_ac7_native_renderer* renderer);
    // Releases the one-to-one depth shader and its device and clears readiness.
    void (*depth_release)(rsf_ac7_native_renderer* renderer);
} rsf_ac7_truesky_leaves;

// Each call runs with the renderer's cloud resolution lock held.
// Per sky render while active: installs the motion and depth relays, then the full cloud grid once
// the depth relay is ready. A refusal is logged and keeps the supported native cloud resolution.
void rsf_ac7_truesky_apply(rsf_ac7_truesky_patches* patches, const rsf_ac7_truesky_leaves* leaves);
// The depth relay found unverified bindings: the full grid is refused for this renderer and the
// native divisor restored for the next allocation.
void rsf_ac7_truesky_refuse_full_grid(rsf_ac7_truesky_patches* patches,
    const rsf_ac7_truesky_leaves* leaves);
// Retirement in the former order. False, logged, keeps everything not yet restored for a retry.
bool rsf_ac7_truesky_restore(rsf_ac7_truesky_patches* patches, const rsf_ac7_truesky_leaves* leaves);

// Native calls of the native renderer, bound at run time by plugin.c and the native graph fixture
// so that MinHook, the Windows input facts, the render scope create call and the shader factory
// leaf stay off the link line of every other rsf-ac7 consumer. native_renderer_platform.c defines
// the table. Every entry is required.
typedef struct rsf_ac7_native_calls {
    // MinHook, returning MH_STATUS.
    int32_t (*hook_initialize)(void);
    int32_t (*hook_create)(void* target, void* detour, void** original);
    int32_t (*hook_enable)(void* target);
    int32_t (*hook_disable)(void* target);
    int32_t (*hook_remove)(void* target);
    // rsf_win32_input_kind and the XInput facts (integrations/windows).
    uint32_t (*input_kind)(uint32_t message, intptr_t lparam);
    void (*controller_install)(void (*callback)(void));
    void (*controller_disable)(void);
    bool (*controller_remove)(void);
    // rsf_ac7_render_scopes_create_passes; `out` receives the rsf_ac7_render_scopes.
    int (*scopes_create)(uint64_t session_id, uint32_t capacity, rsf_ac7_render_scope_fn state_callback,
        rsf_ac7_render_scope_fn pass_callback, void* user, void** out);
    // Calls the engine factory create, a void* (*)(void* rhi, void* result, const void* code),
    // into created. A C++ exception from it returns false.
    bool (*shader_factory)(void* create, void* rhi, void* result, const void* code, void** created);
} rsf_ac7_native_calls;

extern const rsf_ac7_native_calls rsf_ac7_native_calls_table;
// Binds the table before any renderer leaf is used. The table must outlive the module.
void rsf_ac7_native_renderer_bind(const rsf_ac7_native_calls* calls);

#ifdef __cplusplus
}
#endif
