/* SPDX-License-Identifier: MIT */
#ifndef RSF_AC7_SCOPE_OWNER_H
#define RSF_AC7_SCOPE_OWNER_H

/* Internal C interface for the Rust scope owner, its C entry points and their exception leaves.
   tools/generate-rust-bindings.py generates src/scope_abi.rs from this header. */
#include <stdbool.h>
#include <stddef.h>
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

/* render_scope_entry.c: SEH leaves that stop C++ exception code 0xE06D7363 and return false. */
extern const rsf_ac7_scope_calls rsf_ac7_scope_exception_calls;

// rsf_ac7_render_scopes_create_passes with the exception leaves. A null pass callback is the
// single-callback form of rsf_ac7_render_scopes_create.
int rsf_ac7_scope_owner_create(uint64_t session_id, uint32_t capacity,
    rsf_ac7_render_scope_fn state_callback, rsf_ac7_render_scope_fn pass_callback, void* user,
    const rsf_ac7_scope_calls* calls, rsf_ac7_render_scopes** out);

// Copies live engine memory under the native adapter's guarded read.
typedef bool (*rsf_ac7_scope_read)(void* output, const void* input, size_t size);

bool rsf_ac7_scope_primary_view(rsf_ac7_scope_read read, uint64_t view, uint32_t width,
    uint32_t height);
bool rsf_ac7_scope_primary_renderer(rsf_ac7_scope_read read, uint64_t renderer, uint32_t width,
    uint32_t height);
// Fills the native view, family, frame, rectangles and mip bias of an initialized scope.
bool rsf_ac7_scope_primary_facts(rsf_ac7_scope_read read, uint64_t renderer, uint32_t width,
    uint32_t height, rsf_ac7_render_scope* scope);

#ifdef __cplusplus
}
#endif
#endif
