// SPDX-License-Identifier: GPL-3.0-only
#pragma once
#include <stddef.h>
#include <stdint.h>
#ifdef __cplusplus
extern "C" {
#endif
#define RSF_SHADER_PATCH_VERSION 1u
enum { RSF_PATCH_OK=0, RSF_PATCH_ARGUMENT=-1, RSF_PATCH_ABI=-2, RSF_PATCH_SOURCE=-3,
    RSF_PATCH_HASH=-4, RSF_PATCH_ANCHOR=-5, RSF_PATCH_COMPILE=-6, RSF_PATCH_ORIGINAL=-7,
    RSF_PATCH_REFLECTION=-8, RSF_PATCH_FORMAT=-9, RSF_PATCH_INTERNAL=-10 };
typedef struct rsf_patch_bytes { const void* data; size_t size; } rsf_patch_bytes;
/* Callback bytes remain valid until the next callback; the service copies them.
   The callback must not throw or re-enter the same session. */
typedef int32_t (*rsf_patch_source_fn)(void*, const char*, rsf_patch_bytes*);
typedef struct rsf_patch_define { const char* name; const char* value; } rsf_patch_define;
typedef struct rsf_patch_request {
    uint32_t struct_size, abi_version;
    rsf_patch_bytes source, original_bytecode;
    const char* entry;
    const char* target;
    uint32_t flags;
    const rsf_patch_define* defines;
    size_t define_count;
    rsf_patch_source_fn resolve;
    void* user;
} rsf_patch_request;
typedef struct rsf_patch_output {
    uint32_t struct_size, abi_version;
    rsf_patch_bytes bytecode; /* session-owned until next prepare or close */
    uint32_t cache_hit;
    char reason[256];
} rsf_patch_output;
typedef struct rsf_patch_session rsf_patch_session;
typedef struct rsf_patch_root {
    uint32_t struct_size, abi_version;
    const char* name;
    const char* source_sha256;
    const char* patched_sha256;
    const char* tag; /* Caller-owned meaning; the core does not interpret it. */
    uint32_t stage; /* First letter of the native target. */
} rsf_patch_root;
typedef struct rsf_patch_counters {
    uint32_t struct_size, abi_version;
    uint64_t intercepted, built, cache_hits, refused;
    uint64_t refusals[11]; /* index = -rsf_patch_result */
} rsf_patch_counters;
typedef struct rsf_shader_patch_api {
    uint32_t struct_size, abi_version;
    int32_t (*open)(rsf_patch_bytes pack, const wchar_t* cache, void* d3dcompile, rsf_patch_session**);
    const char* (*hash)(const rsf_patch_session*); /* borrowed until close */
    uint32_t (*root_count)(const rsf_patch_session*);
    int32_t (*root)(const rsf_patch_session*, uint32_t index, rsf_patch_root*);
    int32_t (*prepare)(rsf_patch_session*, const rsf_patch_request*, rsf_patch_output*);
    void (*close)(rsf_patch_session*);
    int32_t (*attach_d3d11)(void* device);
    int32_t (*create_d3d11)(void* device, const char* target, rsf_patch_bytes, void** shader);
    int32_t (*constant_buffer)(void* device, uint32_t size, void** buffer);
    int32_t (*upload_constant)(void* context, void* buffer, rsf_patch_bytes);
    int32_t (*bind_constant)(void* context, uint32_t stage, uint32_t slot, void* buffer);
    int32_t (*counters)(rsf_patch_counters*);
    int32_t (*dump)(const wchar_t* directory);
} rsf_shader_patch_api;
typedef int32_t (*rsf_get_shader_patch_api_fn)(rsf_shader_patch_api*);
#if defined(RSF_RUNTIME_BUILD)
#define RSF_PATCH_EXPORT __declspec(dllexport)
#else
#define RSF_PATCH_EXPORT
#endif
RSF_PATCH_EXPORT int32_t rsf_get_shader_patch_api(rsf_shader_patch_api*);
#ifdef __cplusplus
}
#endif
