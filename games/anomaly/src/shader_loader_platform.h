// SPDX-License-Identifier: MIT
#pragma once
// C declarations of the private Anomaly shader-loader ABI implemented by the Rust crate
// (shader_loader.rs, shader_loader/d3d.rs, shader_loader/reflection.rs). No native implementation remains;
// the C++ projection used by the standalone device fixtures lives in tests/anomaly_motion_shader_facade.h.
#include <assert.h>
#include <stddef.h>
#include <stdint.h>
#include <stdbool.h>

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
static_assert(sizeof(rsf_anomaly_shader_request) == 88 && offsetof(rsf_anomaly_shader_request, defines) == 40 &&
    offsetof(rsf_anomaly_shader_request, bytecode) == 72, "request layout shared with shader_loader.rs");
// The admitted manifest is Rust-owned and immutable; free it with rsf_anomaly_shader_admitted_free.
// Its namespace and rows borrow its storage until then.
typedef struct rsf_anomaly_shader_admitted rsf_anomaly_shader_admitted;  // opaque
typedef struct {
    rsf_anomaly_shader_bytes name, original, motion;
    uint8_t stage, family;
} rsf_anomaly_shader_entry;
static_assert(sizeof(rsf_anomaly_shader_entry) == 56 && offsetof(rsf_anomaly_shader_entry, stage) == 48 &&
    offsetof(rsf_anomaly_shader_entry, family) == 49, "entry layout shared with shader_loader/manifest.rs");
typedef struct {
    rsf_anomaly_shader_bytes name_space;
    const rsf_anomaly_shader_entry* entries;  // ordered by original SHA-256
    size_t count;
} rsf_anomaly_shader_manifest_view;
typedef struct {
    void* user;
    void (*report)(void*, rsf_anomaly_shader_bytes message);
} rsf_anomaly_shader_refusal;

// Reflection owners. A snapshot is a complete copy owned by Rust; a reflection is its borrowed view.
typedef struct rsf_anomaly_shader_reflection rsf_anomaly_shader_reflection;
typedef struct rsf_anomaly_shader_snapshot rsf_anomaly_shader_snapshot;
// Loaded D3D compiler and reflector pair used by the deployed-tree fixtures.
typedef struct rsf_anomaly_shader_d3d rsf_anomaly_shader_d3d;

// Compile or stock-publish callback: (user, ok, bytecode-or-error) returns false to stop.
typedef bool (*rsf_anomaly_shader_publish_compile)(void*, bool, rsf_anomaly_shader_bytes);
// Prepared-variant callback: (user, entry, outcome, compatibility, skin, detail, variant, stock, reflection).
// The two snapshots transfer to the receiver, which frees them with rsf_anomaly_shader_snapshot_drop.
typedef void (*rsf_anomaly_shader_publish_prepared)(void*, const rsf_anomaly_shader_entry*, uint8_t, uint8_t,
    int32_t, rsf_anomaly_shader_bytes, rsf_anomaly_shader_bytes, rsf_anomaly_shader_snapshot*,
    rsf_anomaly_shader_snapshot*);

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

const rsf_anomaly_shader_reflection* rsf_anomaly_shader_snapshot_view(const rsf_anomaly_shader_snapshot*);
bool rsf_anomaly_shader_snapshot_keeps_buffer_sizes(const rsf_anomaly_shader_snapshot*,
    const rsf_anomaly_shader_snapshot*);
void rsf_anomaly_shader_snapshot_drop(rsf_anomaly_shader_snapshot*);

// Loads the compiler named by a NUL-terminated UTF-16 string plus the system d3dcompiler_47 reflector.
rsf_anomaly_shader_d3d* rsf_anomaly_shader_d3d_load(const uint16_t*);
void rsf_anomaly_shader_d3d_unload(rsf_anomaly_shader_d3d*);
rsf_anomaly_shader_snapshot* rsf_anomaly_shader_d3d_reflect(const rsf_anomaly_shader_d3d*, rsf_anomaly_shader_bytes);
// Compiles the request with the include tree at the given UTF-16 path; Rust opens include files.
bool rsf_anomaly_shader_d3d_compile(const rsf_anomaly_shader_d3d*, rsf_anomaly_shader_wide tree,
    const rsf_anomaly_shader_request*, void* user, rsf_anomaly_shader_publish_compile);
// Reproduces the stock bytecode from deployment/original (when the stock callback is given), then prepares the
// motion variant from deployment/motion.
void rsf_anomaly_shader_d3d_prepare(const rsf_anomaly_shader_d3d*, const rsf_anomaly_shader_admitted*,
    const rsf_anomaly_shader_request*, rsf_anomaly_shader_wide deployment, void* user,
    rsf_anomaly_shader_publish_compile stock, rsf_anomaly_shader_publish_prepared);
#ifdef __cplusplus
}
#endif
