// SPDX-License-Identifier: MIT
#pragma once
#include <stddef.h>
#include <stdint.h>
#ifndef __cplusplus
#include <stdbool.h>
#endif

// Private synchronous ABI. All spans borrow storage through the call or callback.
typedef struct { const uint8_t* data; size_t len; } rsf_anomaly_shader_bytes;
typedef struct { const uint16_t* data; size_t len; } rsf_anomaly_shader_wide;
typedef struct { rsf_anomaly_shader_bytes name, value; } rsf_anomaly_shader_define;
typedef struct { const rsf_anomaly_shader_define* data; size_t len; } rsf_anomaly_shader_defines;
typedef struct {
    rsf_anomaly_shader_bytes target, entry;
    uint32_t flags;
    rsf_anomaly_shader_defines defines;
    rsf_anomaly_shader_bytes source, bytecode;
} rsf_anomaly_shader_request;
typedef struct {
    rsf_anomaly_shader_bytes name_space, original, motion, name;
    uint8_t stage;
} rsf_anomaly_shader_identity;

typedef struct {
    uint32_t calls[2], call_length, source, length, entry, flags, defines, define_capacity;
} rsf_anomaly_shader_frame_layout;
typedef struct {
    rsf_anomaly_shader_bytes source, entry;
    uint32_t flags;
    rsf_anomaly_shader_defines defines;
} rsf_anomaly_shader_decoded_frame;
typedef bool (*rsf_anomaly_shader_read)(const void*, uintptr_t address, uint8_t* out, size_t);
typedef bool (*rsf_anomaly_shader_publish)(void*, const rsf_anomaly_shader_decoded_frame*);

typedef struct {
    rsf_anomaly_shader_bytes name, original, motion, tag;
    uint32_t stage;
} rsf_anomaly_shader_root;
typedef bool (*rsf_anomaly_shader_read_root)(const void*, uint32_t, rsf_anomaly_shader_root*);
// The admitted manifest is Rust-owned and immutable; free it with rsf_anomaly_shader_admitted_free.
// Its namespace and rows borrow its storage until then.
typedef struct rsf_anomaly_shader_admitted rsf_anomaly_shader_admitted;  // opaque
typedef struct {
    rsf_anomaly_shader_bytes name, original, motion;
    uint8_t stage, family;
} rsf_anomaly_shader_entry;
typedef struct {
    rsf_anomaly_shader_bytes name_space;
    const rsf_anomaly_shader_entry* entries;  // ordered by original SHA-256
    size_t count;
} rsf_anomaly_shader_manifest_view;
typedef struct {
    void* user;
    void (*report)(void*, rsf_anomaly_shader_bytes message);
} rsf_anomaly_shader_refusal;

typedef struct {
    rsf_anomaly_shader_bytes name;
    uint32_t index, reg, system_value, component, mask;
} rsf_anomaly_shader_parameter;
typedef struct {
    rsf_anomaly_shader_bytes name;
    uint32_t offset, size;
} rsf_anomaly_shader_variable;
typedef struct {
    rsf_anomaly_shader_bytes name;
    uint32_t size;
    struct { const rsf_anomaly_shader_variable* data; size_t len; } variables;
} rsf_anomaly_shader_buffer;
typedef struct {
    rsf_anomaly_shader_bytes name;
    uint32_t type, point, count, return_type, dimension;
} rsf_anomaly_shader_binding;
typedef struct {
    uint32_t version;
    struct { const rsf_anomaly_shader_parameter* data; size_t len; } inputs, outputs, patch;
    struct { const rsf_anomaly_shader_binding* data; size_t len; } bindings;
    struct { const rsf_anomaly_shader_buffer* data; size_t len; } buffers;
} rsf_anomaly_shader_reflection;
typedef struct {
    void* user;
    // Stock and variant have separate storage. An invocation only invalidates the same slot.
    bool (*reflect)(void*, bool stock, rsf_anomaly_shader_bytes, rsf_anomaly_shader_reflection*);
    void (*complete)(void*, uint8_t outcome, uint8_t compatibility, int32_t skin,
        rsf_anomaly_shader_bytes detail, rsf_anomaly_shader_bytes variant);
} rsf_anomaly_shader_preparation;
// Returns compiled bytes on success, diagnostic bytes on refusal, until the next call.
typedef bool (*rsf_anomaly_shader_compile)(void*, bool motion, rsf_anomaly_shader_bytes,
    rsf_anomaly_shader_bytes*);

#ifdef __cplusplus
extern "C" {
#endif
const char* rsf_anomaly_shader_family_name(uint8_t);
uint8_t rsf_anomaly_shader_family_stage(uint8_t);
const char* rsf_anomaly_shader_compatibility_name(uint8_t);
// Null on refusal, after reporting the reason once.
rsf_anomaly_shader_admitted* rsf_anomaly_shader_manifest(rsf_anomaly_shader_bytes, const rsf_anomaly_shader_refusal*);
rsf_anomaly_shader_admitted* rsf_anomaly_shader_pack_manifest(rsf_anomaly_shader_bytes, uint32_t count,
    const void* session, rsf_anomaly_shader_read_root, const rsf_anomaly_shader_refusal*);
void rsf_anomaly_shader_admitted_free(rsf_anomaly_shader_admitted*);
void rsf_anomaly_shader_admitted_view(const rsf_anomaly_shader_admitted*, rsf_anomaly_shader_manifest_view*);
const rsf_anomaly_shader_entry* rsf_anomaly_shader_admitted_find(const rsf_anomaly_shader_admitted*,
    rsf_anomaly_shader_bytes original);
bool rsf_anomaly_shader_sha256(rsf_anomaly_shader_bytes, uint8_t (*out)[64]);
bool rsf_anomaly_shader_stage_matches(rsf_anomaly_shader_bytes, uint8_t);
uint8_t rsf_anomaly_shader_frame(const rsf_anomaly_shader_frame_layout*, uintptr_t module, uintptr_t return_address,
    uintptr_t stack, const void* reader, rsf_anomaly_shader_read, void* request, rsf_anomaly_shader_publish);
uint8_t rsf_anomaly_shader_compatible(uint8_t, const rsf_anomaly_shader_reflection*, const rsf_anomaly_shader_reflection*);
bool rsf_anomaly_shader_linkable(const rsf_anomaly_shader_reflection*, const rsf_anomaly_shader_reflection*);
bool rsf_anomaly_shader_tess_linkable(const rsf_anomaly_shader_reflection*, const rsf_anomaly_shader_reflection*,
    const rsf_anomaly_shader_reflection*, const rsf_anomaly_shader_reflection*);
int32_t rsf_anomaly_shader_skin_mode(rsf_anomaly_shader_defines);
bool rsf_anomaly_shader_cache_key(const rsf_anomaly_shader_request*, const rsf_anomaly_shader_identity*, uint8_t (*out)[64]);
void rsf_anomaly_shader_prepare_tree(const rsf_anomaly_shader_request*, const rsf_anomaly_shader_identity*,
    rsf_anomaly_shader_wide deployment, rsf_anomaly_shader_wide cache, const rsf_anomaly_shader_preparation*,
    rsf_anomaly_shader_compile);
void rsf_anomaly_shader_finish_runtime(const rsf_anomaly_shader_request*, uint8_t stage, int32_t status, bool cache_hit,
    rsf_anomaly_shader_bytes variant, rsf_anomaly_shader_bytes detail, const rsf_anomaly_shader_preparation*);
#ifdef __cplusplus
}

#include "shader_loader.h"
#include <cstddef>
namespace rsf::anomaly::motion {
static_assert(sizeof(ManifestEntry) == sizeof(rsf_anomaly_shader_entry) && sizeof(ManifestEntry) == 56 &&
    offsetof(ManifestEntry, name) == offsetof(rsf_anomaly_shader_entry, name) &&
    offsetof(ManifestEntry, original_sha256) == offsetof(rsf_anomaly_shader_entry, original) &&
    offsetof(ManifestEntry, motion_sha256) == offsetof(rsf_anomaly_shader_entry, motion) &&
    offsetof(ManifestEntry, stage) == offsetof(rsf_anomaly_shader_entry, stage) &&
    offsetof(ManifestEntry, family) == offsetof(rsf_anomaly_shader_entry, family) &&
    sizeof(ManifestText) == sizeof(rsf_anomaly_shader_bytes) && sizeof(Family) == 1);
// ManifestEntry is the C++ spelling of one Rust-owned row.
inline const ManifestEntry& manifest_entry(const rsf_anomaly_shader_entry& row) noexcept
{
    return *reinterpret_cast<const ManifestEntry*>(&row);
}
bool admit_runtime_manifest(Manifest&, std::string_view name_space, uint32_t count, const void* session,
    rsf_anomaly_shader_read_root, std::string& error);
Prepared finish_runtime_shader(const ShaderRequest&, const ManifestEntry&, Reflector&, int32_t status, bool cache_hit,
    std::span<const std::byte>, std::string_view detail);
}
#endif
