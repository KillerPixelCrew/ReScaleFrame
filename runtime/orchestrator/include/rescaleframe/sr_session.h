/* SPDX-License-Identifier: GPL-3.0-only */
#ifndef RSF_SR_SESSION_H
#define RSF_SR_SESSION_H
#include <rescaleframe/backend.h>
#include <rescaleframe/runtime.h>
#ifdef __cplusplus
extern "C" {
#endif
#define RSF_SR_SESSION_ABI_VERSION 1u
typedef uint32_t rsf_sr_backend;
#define RSF_SR_NONE 0u
#define RSF_SR_FSR2 2u
#define RSF_SR_FSR3 3u
#define RSF_SR_FSR4 4u
#define RSF_SR_XESS 5u

typedef struct rsf_sr_session rsf_sr_session;
typedef struct rsf_sr_session_setup {
    uint32_t struct_size;
    uint32_t abi_version;
    /* D3D12 creation contract. Output extent is fixed for this owner; recreate for resize.
       Device/log targets are borrowed, while runtime_directory_utf8 is replaced by family paths. */
    rsf_sr_open_desc open;
    /* One directory per family permits SDK releases to coexist. Strings are copied at create. */
    const char* fsr2_directory_utf8;
    const char* fsr3_directory_utf8;
    const char* fsr4_directory_utf8;
    const char* xess_directory_utf8;
} rsf_sr_session_setup;
typedef struct rsf_sr_session_status {
    uint32_t struct_size;
    /* Last valid request can differ from effective after a refused transactional switch. */
    rsf_sr_backend requested;
    rsf_sr_backend effective;
    rsf_quality quality;
    rsf_backend_result last_switch_result;
    rsf_backend_result last_frame_result;
    /* Last successfully planned render extent in pixels. Effective NONE has no active context;
       previously planned dimensions/quality can remain in this diagnostic snapshot. */
    uint32_t render_width;
    uint32_t render_height;
    /* Provider-accepted attempts versus refused calls; acceptance does not validate image quality. */
    uint64_t frames_evaluated;
    uint64_t frames_refused;
    uint64_t version_id;
    char version_name[128];
} rsf_sr_session_status;

/* All operations run on the graphics owner thread. A settings UI queues requests to that thread.
   Device and command context remain caller-owned and outlive the session. The caller must finish
   submitted GPU work before switch, resize, or destroy. No game identifiers or hooks enter here. */
/* Create an empty session and copy the configured SDK paths. The caller owns device lifetime. */
RSF_RUNTIME_API rsf_backend_result rsf_sr_session_create(const rsf_sr_session_setup* setup,
                                                        rsf_sr_session** out);
/* Prepare and plan the replacement before committing. A refusal preserves the current session.
   NONE releases the active backend. Every successful change resets the next evaluated history. */
RSF_RUNTIME_API rsf_backend_result rsf_sr_session_select(rsf_sr_session* session,
                                                         rsf_sr_backend backend, rsf_quality quality,
                                                         uint64_t version_id);
/* Exposure policy is a context creation flag. Prepare a replacement before committing;
   refusal preserves the previous context. Caller completes submitted GPU work first. */
RSF_RUNTIME_API rsf_backend_result rsf_sr_session_set_auto_exposure(rsf_sr_session* session,
                                                                    uint32_t enabled);
/* Evaluate one eligible frame on the graphics owner thread. Frame resources are borrowed for the
   call and must be in the states declared by their resource records. */
RSF_RUNTIME_API rsf_backend_result rsf_sr_session_evaluate(rsf_sr_session* session,
                                                           void* command_context,
                                                           const rsf_sr_frame* frame);
/* Copy the current requested/effective backend, dimensions, counters, and last results. */
RSF_RUNTIME_API rsf_backend_result rsf_sr_session_get_status(const rsf_sr_session* session,
                                                             rsf_sr_session_status* status);
/* Release the active backend and session storage. Complete caller-submitted GPU work first. */
RSF_RUNTIME_API void rsf_sr_session_destroy(rsf_sr_session* session);
#ifdef __cplusplus
}
#endif
#endif
