/* SPDX-License-Identifier: MIT */
#pragma once
#include <stdbool.h>
#include <wchar.h>
#include <rescaleframe/render_host.h>
#include <rescaleframe/backend.h>

typedef bool (*rsf_rdr2_fingerprint_fn)(const wchar_t* executable, char* digest);

typedef struct rsf_rdr2_runtime_leaves {
    rsf_rdr2_fingerprint_fn fingerprint;
    void (*diagnostics)(const wchar_t* modules_path, const wchar_t* dump_directory);
    bool (*native_settings)(const wchar_t* path);
} rsf_rdr2_runtime_leaves;

#ifdef __cplusplus
extern "C" {
#endif
void rsf_rdr2_fill_runtime_setup(const wchar_t*, const wchar_t*, rsf_render_host_setup*);
unsigned int rsf_rdr2_asi_start_runtime(void* module, const rsf_rdr2_runtime_leaves*);
unsigned int rsf_rdr2_attach_start_runtime(void* module, rsf_rdr2_fingerprint_fn);
void rsf_rdr2_runtime_log(void*, const char*);
#ifdef __cplusplus
}

inline rsf_render_host_setup rsf_rdr2_runtime_setup(const wchar_t* configuration, const wchar_t* directory)
{
    rsf_render_host_setup setup{};
    rsf_rdr2_fill_runtime_setup(configuration, directory, &setup);
    return setup;
}

#endif
