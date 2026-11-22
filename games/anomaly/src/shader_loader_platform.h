// SPDX-License-Identifier: MIT
#pragma once
#include "shader_frame_layout.h"
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

typedef struct rsf_anomaly_shader_reflection rsf_anomaly_shader_reflection;
#ifdef __cplusplus
extern "C" {
#endif
// Null on refusal, after reporting the reason once.
rsf_anomaly_shader_admitted* rsf_anomaly_shader_manifest(rsf_anomaly_shader_bytes, const rsf_anomaly_shader_refusal*);
void rsf_anomaly_shader_admitted_free(rsf_anomaly_shader_admitted*);
void rsf_anomaly_shader_admitted_view(const rsf_anomaly_shader_admitted*, rsf_anomaly_shader_manifest_view*);
const rsf_anomaly_shader_entry* rsf_anomaly_shader_admitted_find(const rsf_anomaly_shader_admitted*,
    rsf_anomaly_shader_bytes original);
const char* rsf_anomaly_shader_outcome_name(uint8_t);
bool rsf_anomaly_shader_linkable(const rsf_anomaly_shader_reflection*, const rsf_anomaly_shader_reflection*);
bool rsf_anomaly_shader_tess_linkable(const rsf_anomaly_shader_reflection*, const rsf_anomaly_shader_reflection*,
    const rsf_anomaly_shader_reflection*, const rsf_anomaly_shader_reflection*);
#ifdef __cplusplus
}

#include "shader_loader.h"
#include <cstddef>
namespace rsf::anomaly::motion {
static_assert(sizeof(rsf_anomaly_shader_request) == 88 && offsetof(rsf_anomaly_shader_request, defines) == 40 &&
    offsetof(rsf_anomaly_shader_request, bytecode) == 72);


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
const rsf_anomaly_shader_reflection* reflection_view(const Reflection&) noexcept;
}
#endif
