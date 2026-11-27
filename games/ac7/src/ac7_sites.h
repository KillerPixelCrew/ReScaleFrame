// SPDX-License-Identifier: MIT
#pragma once
// C declarations only: the site and patch records of the native renderer (rsf-ac7-plugin) that the
// native graph fixture (in rsf-ac7) reads, the fixture's patch entries, and the call table plugin.c
// binds. Not a DLL ABI: everything links into ReScaleFrame.Game.AC7.dll and the
// native graph fixture. build.rs binds it into the crate's sys module.
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
// Capacity of a code patch record's byte arrays.
#define RSF_AC7_SITE_BYTES 16u
#define RSF_AC7_VELOCITY_PATCHES 3u

// One engine code patch: `size` bytes at `rva`, native and patched.
typedef struct rsf_ac7_code_patch {
    uint32_t rva;
    uint32_t size;
    unsigned char native[16];
    unsigned char replacement[16];
} rsf_ac7_code_patch;

// Engine patch ownership kept in the native renderer and changed only by patches.rs.
typedef struct rsf_ac7_code_patches {
    unsigned char* velocity_module; // module the applied velocity patches belong to
    uint32_t velocity_applied;      // velocity patches applied, in table order
    unsigned char* scene_precision_site; // patched scene-precision site, or null
} rsf_ac7_code_patches;

// Native graph fixture entries (warp-fixture). The temporal velocity patch at `index`; false out
// of range.
bool rsf_ac7_velocity_patch(uint32_t index, rsf_ac7_code_patch* out);
// All-or-nothing temporal velocity group at `base`: preflight, write, roll back on failure.
bool rsf_ac7_velocity_apply(rsf_ac7_code_patches* patches, unsigned char* base);
// Restores owned velocity patches in reverse; false keeps the unrestored ones owned.
bool rsf_ac7_velocity_restore(rsf_ac7_code_patches* patches);

// TrueSky plugin (TrueSkyPluginRender_MT.dll) patches and call relays (truesky.rs).
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



// Native calls of the native renderer, bound at run time by plugin.c and the native graph fixture
// so that the Windows input facts, the render scope create call and the shader factory leaf stay
// off the link line of the plugin crate's unit tests. native_renderer_platform.c defines the
// table. Every entry is required. MinHook the renderer calls directly (rsf-minhook-sys).
typedef struct rsf_ac7_native_calls {
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
