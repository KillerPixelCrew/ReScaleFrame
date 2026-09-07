/* SPDX-License-Identifier: GPL-3.0-only */
/* Sets of pipeline objects, kept by address, with the one rule that makes that safe.

   Identifying a draw cheaply means settling what each object is once, when the game creates it, and
   then comparing pointers at the draw. The comparison is the easy half. The hard half is that a
   pointer only means something while the object behind it is alive: D3D11 hands out a released
   address again immediately, and this project has watched a freshly created vertex shader land on
   the address a pixel shader vacated two lines earlier. A set that remembered the old answer would
   then be confidently wrong about a live object, which is worse than knowing nothing.

   So every creation forgets the address first, across every set, and only then records what the new
   object is. That is the whole design, and it is why this is a module rather than a few arrays: the
   forgetting has to be impossible to skip.

   What an object *is* does not live here. Whether a declaration is Slate's, or a texture is a
   widget target, is a game question answered in `games/<id>`; this holds the answer and nothing
   more. Bounded, so a misfiring rule cannot grow without limit, and allocation free after creation,
   because the caller is inside the game's own creation path. */

#ifndef RSF_UI_IDENTIFY_H
#define RSF_UI_IDENTIFY_H

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

#define RSF_UI_IDENTIFY_ABI_VERSION 1u

/* Which set an object belongs to. An object is in at most one: they are answers to the same
   question, and a thing that is both Slate's declaration and the canvas's is a bug in the rule that
   said so, not a state to represent. */
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
/* The set is full. Reported rather than silently dropped: a set that quietly stopped recording is
   indistinguishable from a rule that stopped matching, and this project has spent runs on that
   distinction. */
#define RSF_UI_ERROR_FULL ((rsf_ui_result)-3)

/* Upper bounds. A frame has a handful of interface declarations, a couple of dozen converter
   targets, and however many shaders a settings file names. */
#define RSF_UI_MAX_LAYOUTS 16u
#define RSF_UI_MAX_WIDGET_TARGETS 32u
#define RSF_UI_MAX_NAMED_SHADERS 32u

typedef struct rsf_ui_registry rsf_ui_registry;

/* Create an empty registry. Returns null only when out of memory. */
rsf_ui_registry* rsf_ui_registry_create(uint32_t abi_version);
void rsf_ui_registry_destroy(rsf_ui_registry* registry);

/* Record that `object` belongs to `set`.

   Does not forget the address first: `rsf_ui_registry_forget` is a separate call because the caller
   knows something this cannot, namely that an address is being handed out afresh. Adding an object
   already in the same set succeeds and changes nothing. */
rsf_ui_result rsf_ui_registry_add(rsf_ui_registry* registry, rsf_ui_set set, void* object);

/* Drop `object` from every set. Call this for every object the game creates, before deciding what
   the new one is, whether or not the address was ever recorded. Cheap when it was not. */
void rsf_ui_registry_forget(rsf_ui_registry* registry, void* object);

int rsf_ui_registry_contains(const rsf_ui_registry* registry, rsf_ui_set set, const void* object);

/* The set as a contiguous array, for handing to a game's own rule.

   Valid until the next add or forget, which is the same thread and the same call stack in practice:
   the sets change on the game's creation path and are read on its draw path. */
void* const* rsf_ui_registry_view(const rsf_ui_registry* registry, rsf_ui_set set,
                                  uint32_t* count_out);

/* How many adds were refused because a set was full, and how many recorded objects were dropped
   because their address was handed out again. Both are worth reporting: the first says a rule is
   matching more than it should, the second says how often the hazard this module exists for
   actually occurs in a real frame, which nobody has measured. */
typedef struct rsf_ui_registry_counters {
    uint32_t struct_size;
    uint32_t refused_full[RSF_UI_SET_COUNT];
    uint32_t forgotten_on_reuse;
    uint32_t recorded[RSF_UI_SET_COUNT];
} rsf_ui_registry_counters;

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
