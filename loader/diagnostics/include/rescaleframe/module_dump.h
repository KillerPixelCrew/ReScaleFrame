/* SPDX-License-Identifier: GPL-3.0-only */
/* In-process PE64 diagnostics, guarded code writes and researched Unreal cvar access.
   Module pointers must name trusted loaded images with valid headers; this is not a parser for
   arbitrary untrusted bytes. Caller-owned strings/buffers are borrowed for each synchronous call.
   Dumping/entropy alone does not establish renderer readiness or verified hook ownership. */

#ifndef RSF_MODULE_DUMP_H
#define RSF_MODULE_DUMP_H

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

#define RSF_MODULE_DUMP_ABI_VERSION 1u

typedef int32_t rsf_dump_result;
#define RSF_DUMP_OK ((rsf_dump_result)0)
#define RSF_DUMP_ERROR_INVALID_ARGUMENT ((rsf_dump_result)-1)
#define RSF_DUMP_ERROR_ABI_MISMATCH ((rsf_dump_result)-2)
#define RSF_DUMP_ERROR_NOT_A_PE ((rsf_dump_result)-3)
#define RSF_DUMP_ERROR_WRITE_FAILED ((rsf_dump_result)-4)
#define RSF_DUMP_ERROR_STILL_ENCRYPTED ((rsf_dump_result)-5)

/* Shannon entropy in bits per byte, 0..8. Null/empty input returns zero; otherwise data
   must address size readable bytes. No platform state or allocation is used. */
double rsf_shannon_entropy(const void* data, size_t size);

/* Mean entropy of samples evenly spaced across the range, including both ends when
   samples > 1. Null/zero arguments return zero; a range no larger than window is measured whole.
   Overlapping windows are allowed. Caller supplies a readable size-byte range. */
double rsf_sampled_entropy(const void* data, size_t size, size_t window, size_t samples);

typedef struct rsf_dump_options {
    uint32_t struct_size;
    uint32_t abi_version;
    /* Directory the dump and its side-car are written to. Must exist. */
    const char* output_directory_utf8;
    /* Base name for both outputs. When null, the module's own file name is used. */
    const char* output_name_utf8;
    /* Entropy at or above this counts as still encrypted. 7.0 separates the observed
       ciphertext (7.997) from ordinary code by a wide margin. */
    double entropy_threshold;
    /* When non-zero, refuse to write a dump whose code section is still above the threshold. */
    uint32_t require_decrypted;
} rsf_dump_options;

typedef struct rsf_dump_report {
    uint32_t struct_size;
    uint32_t sections_written;
    uint32_t imports_described;
    uint32_t modules_listed;
    /* Heuristic byte-scan count of RIP-relative indirect calls/jumps targeting exact IAT
       slots in executable sections. It supplements entropy but is not disassembly or a general
       proof that an entire protected image is decrypted. */
    uint32_t iat_references;
    /* Sampled entropy of the largest executable section at dump time. */
    double code_entropy;
    uint64_t load_base;
    uint64_t preferred_base;
    uint64_t bytes_written;
} rsf_dump_report;

/* Write <name>.dump and <name>.dump.json from a trusted loaded PE64 image. Null
   module_base selects the main image; options is required and report is optional. Caller initializes
   options size/version and report size. Existing files are overwritten; output directory must exist.
   Raw offsets equal RVAs for offline analysis; imports/entrypoint are not repaired for execution.
   A sidecar failure can leave the dump file written. Error paths do not always populate report. */
rsf_dump_result rsf_dump_module(const void* module_base, const rsf_dump_options* options,
                                rsf_dump_report* report);

/* Measure the largest executable section without writing. Null module_base selects the
   main image; entropy is required. Unreadable regions are measured as zero-filled bytes. */
rsf_dump_result rsf_measure_module_code(const void* module_base, double* entropy);

/* Patch count bytes at main-image RVA after the caller establishes decryption/readiness
   and a safe execution boundary. This helper does not poll entropy or suspend other threads.
   Optional expected bytes must match count and the existing contents; mismatch returns
   INVALID_ARGUMENT before writing. Optional previous receives count bytes only after protection
   change succeeds. Restore page protection and flush instruction cache after the write. */
rsf_dump_result rsf_patch_code(uint32_t rva, const uint8_t* bytes, uint32_t count,
                               const uint8_t* expected, uint32_t expected_count,
                               uint8_t* previous);

/* Research-only Unreal cvar write using a verified console-manager singleton RVA and
   vtable byte offset. Scan 4-byte slots through object+0x100 for floats within 0.0001 of
   expected_current and replace every match; a distinctive expected value is required. This is
   not structural proof of the object layout. found_offset optionally receives the first match.
   Null manager returns ABI_MISMATCH, absent variable NOT_A_PE, no replacement STILL_ENCRYPTED;
   these diagnostic result names are reused outside literal PE/decryption failures. */
rsf_dump_result rsf_console_set_float(const char* name_utf8, float expected_current,
                                      float new_value, uint32_t singleton_rva, uint32_t find_slot,
                                      uint32_t* found_offset);

/* Set the two Unreal cvar thread copies at a researched value_offset. Both must equal
   expected_current before either is written; mismatch returns STILL_ENCRYPTED and protection
   failure WRITE_FAILED. Singleton RVA, lookup slot and object offset must match this executable.
   Checking values is a refusal guard, not synchronization with concurrent engine writers. */
rsf_dump_result rsf_console_set_int(const char* name_utf8, int32_t expected_current,
                                    int32_t new_value, uint32_t value_offset,
                                    uint32_t singleton_rva, uint32_t find_slot);

/* Read-only researched cvar lookup. Optional manager_out/variable_out receive copied
   addresses; floats receives float_count leading values when non-null. Caller must establish that
   the engine object is readable for that range and the lookup/vtable layout matches this build. */
rsf_dump_result rsf_console_probe(const char* name_utf8, uint32_t singleton_rva,
                                  uint32_t find_slot, uint64_t* manager_out,
                                  uint64_t* variable_out, float* floats, uint32_t float_count);

/* Append a bounded loaded-module snapshot to path_utf8. Null label uses "sample".
   Late loads are diagnostic evidence; module presence does not identify the active renderer. */
rsf_dump_result rsf_write_module_list(const char* path_utf8, const char* label);

/* Worker-only polling: after stable_samples consecutive entropy readings below threshold,
   dump the main image. poll_interval_ms and stable_samples must be nonzero; timeout_ms == 0
   waits indefinitely. Timeout returns STILL_ENCRYPTED and optionally the last measured entropy.
   options must contain the same valid size/version/path fields required by rsf_dump_module.
   No loader-lock, render-thread or engine-state synchronization is provided. */
rsf_dump_result rsf_dump_when_decrypted(const rsf_dump_options* options, uint32_t poll_interval_ms,
                                        uint32_t timeout_ms, uint32_t stable_samples,
                                        rsf_dump_report* report);

#ifdef __cplusplus
} /* extern "C" */
#endif

#endif /* RSF_MODULE_DUMP_H */
