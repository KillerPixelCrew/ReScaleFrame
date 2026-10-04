/* SPDX-License-Identifier: GPL-3.0-only */
#ifndef RSF_FG_SESSION_H
#define RSF_FG_SESSION_H
#include <rescaleframe/frame_generation.h>
#include <rescaleframe/runtime.h>
#ifdef __cplusplus
extern "C" {
#endif
typedef struct rsf_fg_session rsf_fg_session;
/* FSR2 is SR only. FSR3/4 share an entry point; select the family in setup.feature_major. */
RSF_RUNTIME_API const rsf_generation_provider* rsf_fg_get_provider(uint32_t backend);
typedef struct rsf_fg_host {
    uint32_t struct_size;
    uint32_t abi_version;
    void* user;
    /* Stop all graphics API callers and drain their submitted GPU work. Keep them stopped
       until resume(). A render-thread mutex alone does not satisfy Streamline's contract. */
    rsf_backend_result (*quiesce)(void* user);
    void (*resume)(void* user);
    /* Release every host-owned reference to the old chain, including its buffers/views.
       adopt borrows the new chain until release; the provider owns the actual chain. */
    rsf_backend_result (*release)(void* user);
    rsf_backend_result (*adopt)(void* user, void* chain);
    /* Restore plain presentation after a refusal, or when generation is deselected. */
    rsf_backend_result (*restore_plain)(void* user);
} rsf_fg_host;
typedef struct rsf_fg_session_status {
    uint32_t struct_size;
    uint32_t has_provider;
    uint32_t plain_available;
    uint64_t chain_generation;
    rsf_backend_result last_switch;
    rsf_backend_result last_frame;
    rsf_fg_status provider;
} rsf_fg_session_status;
/* These are presentation-owner-thread operations except marker(). Provider vtables and host
   callbacks outlive the session. Select borrows setup strings only during its synchronous call.
   A null provider deselects generation. FSR2 has no FG provider; use an independent FG provider
   with FSR2 SR. The host must reject conflicting pre-existing SDK/presentation owners. */
RSF_RUNTIME_API rsf_backend_result rsf_fg_session_create(const rsf_fg_host*, rsf_fg_session**);
RSF_RUNTIME_API rsf_backend_result rsf_fg_session_select(rsf_fg_session*,
    const rsf_generation_provider*, const rsf_generation_setup*, const rsf_fg_options*);
RSF_RUNTIME_API rsf_backend_result rsf_fg_session_begin(rsf_fg_session*, uint64_t frame_id);
RSF_RUNTIME_API rsf_backend_result rsf_fg_session_marker(rsf_fg_session*, uint64_t frame_id,
    rsf_latency_marker, uint64_t timestamp, uint32_t controller_input);
RSF_RUNTIME_API rsf_backend_result rsf_fg_session_prepare(rsf_fg_session*, void* command_list, const rsf_fg_frame*);
RSF_RUNTIME_API rsf_backend_result rsf_fg_session_after_present(rsf_fg_session*, uint64_t frame_id);
/* Discard CPU bookkeeping after a failed/skipped frame. This does not retire GPU resources. */
RSF_RUNTIME_API rsf_backend_result rsf_fg_session_abort(rsf_fg_session*, uint64_t frame_id);
RSF_RUNTIME_API rsf_backend_result rsf_fg_session_retirement(rsf_fg_session*, rsf_fg_retirement*);
RSF_RUNTIME_API rsf_backend_result rsf_fg_session_get_status(rsf_fg_session*, rsf_fg_session_status*);
/* Refuses destruction if quiescence, retirement or host release fails, leaving the handle live. */
RSF_RUNTIME_API rsf_backend_result rsf_fg_session_destroy(rsf_fg_session*);
#ifdef __cplusplus
}
#endif
#endif
