/* SPDX-License-Identifier: GPL-3.0-only */
/* Bounded pointer-membership registries for game-defined draw classification. Objects are not
   retained: call forget on every newly created object before adding its classification, so a
   recycled COM address cannot inherit an old role. The caller supplies all game-specific rules
   and serializes reads and mutations. Registry views borrow internal storage. */

#ifndef RSF_UI_IDENTIFY_H
#define RSF_UI_IDENTIFY_H

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/* ABI 2 adds recent-membership eviction and its counters. */
#define RSF_UI_IDENTIFY_ABI_VERSION 2u

/* Game-defined roles. add does not enforce exclusive membership; caller rules must do so. */
typedef uint32_t rsf_ui_set;
#define RSF_UI_SET_SLATE_LAYOUT ((rsf_ui_set)0)
#define RSF_UI_SET_CANVAS_LAYOUT ((rsf_ui_set)1)
#define RSF_UI_SET_WIDGET_TARGET ((rsf_ui_set)2)
#define RSF_UI_SET_FORCE_SHADER ((rsf_ui_set)3)
#define RSF_UI_SET_SKIP_SHADER ((rsf_ui_set)4)
#define RSF_UI_SET_COUNT ((rsf_ui_set)5)

typedef int32_t rsf_ui_result;
#define RSF_UI_OK ((rsf_ui_result)0)
#define RSF_UI_ERROR_INVALID_ARGUMENT ((rsf_ui_result)-1)
#define RSF_UI_ERROR_ABI_MISMATCH ((rsf_ui_result)-2)
/* Fixed membership set capacity exhausted. note_recent instead evicts the oldest entry. */
#define RSF_UI_ERROR_FULL ((rsf_ui_result)-3)

/* Upper bounds. A frame has a handful of interface declarations, a few dozen converter targets,
   and however many shaders a settings file names. The widget targets are 64 to match what the
   frame tap's candidate set holds, and they are a working set rather than a history: see
   `rsf_ui_registry_note_recent`. */
#define RSF_UI_MAX_LAYOUTS 16u
#define RSF_UI_MAX_WIDGET_TARGETS 64u
#define RSF_UI_MAX_NAMED_SHADERS 32u

typedef struct rsf_ui_registry rsf_ui_registry;

/* Create an empty registry; null for ABI mismatch or allocation failure. */
rsf_ui_registry* rsf_ui_registry_create(uint32_t abi_version);
/* Free the registry's identity arrays; no GPU object is released. Null is accepted. */
void rsf_ui_registry_destroy(rsf_ui_registry* registry);

/* Record that `object` belongs to `set`.

   Does not forget the address first: `rsf_ui_registry_forget` is a separate call because the caller
   knows something this cannot, namely that an address is being handed out afresh. Adding an object
   already in the same set succeeds and changes nothing. */
rsf_ui_result rsf_ui_registry_add(rsf_ui_registry* registry, rsf_ui_set set, void* object);

/* Record that `object` was just seen in `set`, keeping the set as a working set of the most recent.

   For a set whose membership is re-observed continuously rather than fixed at creation, which is
   the widget targets: a converter target is confirmed every frame a Slate draw fills it. Entries
   are ordered by most recent observation; repeated observation moves an entry to the back.
   Adding to a full set evicts its front entry and increments evicted. forget uses unordered
   removal, so after invalidation that ordering is rebuilt by subsequent observations.

   Returns 1 when the membership changed, a new entry with or without an eviction, and 0 when the
   object was already there, so a caller republishing the set does it only when it has to. */
int rsf_ui_registry_note_recent(rsf_ui_registry* registry, rsf_ui_set set, void* object);

/* Drop `object` from every set. Call this for every object the game creates, before deciding what
   the new one is, whether or not the address was ever recorded. Cheap when it was not. */
void rsf_ui_registry_forget(rsf_ui_registry* registry, void* object);

/* Nonzero for membership; zero for absent objects or invalid registry/set arguments. */
int rsf_ui_registry_contains(const rsf_ui_registry* registry, rsf_ui_set set, const void* object);

/* Borrow the contiguous membership array, optionally returning its count. Null for invalid input.
   Mutations (add/note_recent/forget) can change its entries/order; destruction ends its lifetime.
   Serialize consumption with mutation or copy the identities before publishing to another thread. */
void* const* rsf_ui_registry_view(const rsf_ui_registry* registry, rsf_ui_set set,
                                  uint32_t* count_out);

/* Cumulative rejected adds, invalidated memberships, successful insertions, and evictions.
   recorded counts insertions, including reinsertions, rather than current set size. */
typedef struct rsf_ui_registry_counters {
    uint32_t struct_size;
    uint32_t refused_full[RSF_UI_SET_COUNT];
    uint32_t forgotten_on_reuse;
    uint32_t recorded[RSF_UI_SET_COUNT];
    /* Appended in ABI 2. Entries `rsf_ui_registry_note_recent` pushed out of a full set. */
    uint32_t evicted[RSF_UI_SET_COUNT];
} rsf_ui_registry_counters;

/* Copy diagnostic totals into a caller structure initialized with struct_size. */
rsf_ui_result rsf_ui_registry_get_counters(const rsf_ui_registry* registry,
                                           rsf_ui_registry_counters* counters);

/* CRC32C of a shader's bytecode, for naming one in a settings file.

   Castagnoli rather than the zlib polynomial, and no claim beyond being a stable name: two shaders
   colliding would both be forced or both skipped, which is why the override lists are a debugging
   aid and not the identification. Computed once at creation, because the bytecode is only borrowed
   for the duration of the creation call. */
uint32_t rsf_ui_shader_hash(const void* bytecode, uint32_t bytes);

#ifdef __cplusplus
} /* extern "C" */
#endif

#endif /* RSF_UI_IDENTIFY_H */
