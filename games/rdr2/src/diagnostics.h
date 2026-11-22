// SPDX-License-Identifier: MIT
#pragma once
#include <cstdint>

struct rsf_rdr2_dump_result {
    int32_t result;
    uint32_t sections, imports, iat_refs;
    double entropy;
    uint64_t base, bytes;
};
struct rsf_rdr2_census_result {
    uint64_t calls, written, presents, dropped;
    uint32_t sites, hooks, conflicts;
};
struct rsf_rdr2_diagnostics_leaves {
    double (*measure)();
    void (*dump)(const char* directory, double threshold, uint32_t require_decrypted,
        uint32_t poll_ms, uint32_t timeout_ms, uint32_t stable_samples, rsf_rdr2_dump_result*);
    void (*modules)(const char* path, const char* label);
    bool (*census)(rsf_rdr2_census_result*);
};
static_assert(sizeof(rsf_rdr2_dump_result) == 40 && sizeof(rsf_rdr2_census_result) == 48);
static_assert(sizeof(rsf_rdr2_diagnostics_leaves) == 32);

// Workers copy their paths and retain process-lived native function pointers.
extern "C" void rsf_rdr2_start_diagnostics(const wchar_t* modules, const wchar_t* dumps,
    const rsf_rdr2_diagnostics_leaves*) noexcept;
// Returns -100 if initialization succeeds and the request worker fails to start.
extern "C" int32_t rsf_rdr2_start_frame_probe(const wchar_t* library, const wchar_t* prefix,
    int32_t (*initialise)(const char*, const char*), int32_t (*trigger)(uint32_t)) noexcept;
