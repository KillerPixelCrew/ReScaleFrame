// SPDX-License-Identifier: MIT
#pragma once
// Internal boundary between native_renderer.cpp and the Rust owner of the exact-build code sites
// (sites.rs, patches.rs, guards.rs in the rsf-ac7 crate). Not a DLL ABI: both sides link into
// ReScaleFrame.Game.AC7.dll and the native graph fixture. tools/generate-rust-bindings.py
// generates src/sites_abi.rs from this header.
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>
#include <rescaleframe/ac7_native_renderer.h>

#ifdef __cplusplus
extern "C" {
#endif

// Hooked entry sites, in the order of native_renderer.cpp's Hook enum. The first
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
// Activation: scene precision, then temporal velocity, at `base`. Applied patches stay owned when a
// later one refuses.
bool rsf_ac7_engine_patches_apply(rsf_ac7_code_patches* patches, unsigned char* base,
    const rsf_ac7_code_leaves* leaves);
// Retirement: temporal velocity, then scene precision. False, logged, keeps the renderer.
bool rsf_ac7_engine_patches_restore(rsf_ac7_code_patches* patches, const rsf_ac7_code_leaves* leaves);

#ifdef __cplusplus
}
#endif
