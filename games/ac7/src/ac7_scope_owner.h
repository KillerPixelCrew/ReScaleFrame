// SPDX-License-Identifier: MIT
#pragma once
// Internal boundary between render_scope.cpp and the Rust owner of the queued render scopes
// (render_scope.rs in the rsf-ac7 crate). Not a DLL ABI: both sides link into
// ReScaleFrame.Game.AC7.dll and the fixtures that call ac7_render_scope.h.
// tools/generate-rust-bindings.py generates src/scope_abi.rs from this header.
//
// The Rust owner exports every call of ac7_render_scope.h except the two create calls, which
// render_scope.cpp keeps so it can lend its exception leaves.
#include <stdbool.h>
#include <stdint.h>
#include <rescaleframe/ac7_render_scope.h>

#ifdef __cplusplus
extern "C" {
#endif

// Calls into consumer code, made from the engine's RHI stream. Each contains any C++ exception and
// reports it as false, so nothing unwinds through the Rust owner or the engine's command iterator.
// Every entry is required.
typedef struct rsf_ac7_scope_calls {
    // callback(user, command_list, scope, begin) for a nonnull callback.
    bool (*notify)(rsf_ac7_render_scope_fn callback, void* user, void* command_list,
        const rsf_ac7_render_scope* scope, uint32_t begin);
    // lease->resolve(lease->object, scope) for a nonnull resolve.
    bool (*resolve)(const rsf_ac7_scope_lease* lease, rsf_ac7_render_scope* scope);
    // lease->release(lease->object) for a nonnull release.
    bool (*release)(const rsf_ac7_scope_lease* lease);
} rsf_ac7_scope_calls;

// rsf_ac7_render_scopes_create_passes with the exception leaves. A null pass callback is the
// single-callback form of rsf_ac7_render_scopes_create.
int rsf_ac7_scope_owner_create(uint64_t session_id, uint32_t capacity,
    rsf_ac7_render_scope_fn state_callback, rsf_ac7_render_scope_fn pass_callback, void* user,
    const rsf_ac7_scope_calls* calls, rsf_ac7_render_scopes** out);

#ifdef __cplusplus
}
#endif
