/* SPDX-License-Identifier: GPL-3.0-only */
/* Unwired scaffolding: no production caller yet. Kept for the planned ABI 2 frame-callback
   work (see docs/representation-plan.md). Do not delete as dead code. */
#ifndef RSF_RENDER_LINKS_H
#define RSF_RENDER_LINKS_H
#include <rescaleframe/backend.h>
#include <rescaleframe/runtime.h>
#ifdef __cplusplus
extern "C" {
#endif

#define RSF_RENDER_LINK_ABI_VERSION 1u
typedef struct rsf_render_links rsf_render_links;
typedef struct rsf_render_link {
    uint32_t struct_size;
    uint32_t abi_version;
    uint64_t session_id;
    uint64_t source_frame_id;
    uint64_t submission_id;
    uint64_t resource_generation;
    uint64_t producer_key;
} rsf_render_link;

/* Use a session ID unique to this integration lifetime. The plugin chooses a key owned
   by the engine's queued render work. Bind while the CPU
   source identity is known, resolve on the consuming thread, and release at the owner's
   retirement boundary. A key must not be reused while bound. No live entries are evicted.
   All metadata is copied; engine objects and GPU resources are never retained here. */
RSF_RUNTIME_API rsf_backend_result rsf_render_links_create(uint64_t session_id, uint32_t capacity,
    rsf_render_links** out);
RSF_RUNTIME_API rsf_backend_result rsf_render_links_bind(rsf_render_links*, uint64_t producer_key,
    uint64_t source_frame_id, uint64_t resource_generation, rsf_render_link* out);
RSF_RUNTIME_API rsf_backend_result rsf_render_links_lookup(rsf_render_links*, uint64_t producer_key,
    uint64_t resource_generation, rsf_render_link* out);
/* The copied ticket prevents a late release from removing a newly bound, reused key. */
RSF_RUNTIME_API rsf_backend_result rsf_render_links_release(rsf_render_links*, const rsf_render_link*);
/* Call after producers/consumers stop. Refuses destruction while any key remains bound.
   Engine-object retirement does not establish GPU input retirement or a Present boundary. */
RSF_RUNTIME_API rsf_backend_result rsf_render_links_destroy(rsf_render_links*);

#ifdef __cplusplus
}
#endif
#endif
