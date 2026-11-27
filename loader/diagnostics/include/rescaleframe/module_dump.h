/* SPDX-License-Identifier: MIT */
/* Loader diagnostics: read the running main module and write it out for offline analysis.

   This exists because a protected executable can be unreadable on disk while being perfectly
   readable in memory. It is diagnostics only. It performs no interception, touches no graphics
   object, and says nothing about rendering support. */

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

/* Shannon entropy of a byte range, 0.0 to 8.0. Pure, and the same measurement used to establish
   that the shipped file is encrypted. */
double rsf_shannon_entropy(const void* data, size_t size);

/* Mean entropy of evenly spaced windows across a range. Sampling keeps the cost bounded on a
   large section; a whole-section pass over tens of megabytes is not worth its cost when the
   answer is "ciphertext or not". Falls back to a single whole-range measurement when the range
   is smaller than one window. */
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
    /* Fields below were appended for sidecar schema 2. A caller built against the earlier header
       passes a shorter struct_size and gets the defaults. */
    /* Optional UTF-8 path of a TSV whose lines are `rva<TAB>name` (rva as 0x-hex or decimal,
       `#` starts a comment). Accepted lines are merged into the sidecar's "names" section so a
       game plugin can contribute engine-specific names. */
    const char* name_list_path_utf8;
    /* Wall-clock bound for the RTTI scan in milliseconds. Zero selects 20000. */
    uint32_t rtti_budget_ms;
    uint32_t reserved;
} rsf_dump_options;

/* Size of the options structure before the sidecar schema 2 fields were appended. */
#define RSF_DUMP_OPTIONS_V1_SIZE offsetof(rsf_dump_options, name_list_path_utf8)

typedef struct rsf_dump_report {
    uint32_t struct_size;
    uint32_t sections_written;
    uint32_t imports_described;
    uint32_t modules_listed;
    /* Rip-relative calls and jumps through the import table found in executable sections. The
       shipped AC7 file contains none, because its code is ciphertext, so a non-zero count is
       direct evidence that the captured image is decrypted. */
    uint32_t iat_references;
    /* Entropy of the section holding the entry point, measured at dump time. */
    double code_entropy;
    uint64_t load_base;
    uint64_t preferred_base;
    uint64_t bytes_written;
    /* Appended for sidecar schema 2. Filled only when the caller's struct_size covers them. */
    uint32_t rtti_classes;
    uint32_t function_starts;
    uint32_t threads_listed;
    uint32_t reserved;
} rsf_dump_report;

/* Size of the report structure before the sidecar schema 2 fields were appended. */
#define RSF_DUMP_REPORT_V1_SIZE offsetof(rsf_dump_report, rtti_classes)

/* Version of the JSON sidecar layout, written as "schema_version". Schema 1 had no such field.
   Schema 2 adds per-section hashes and entropy, threads, rtti, functions, names and timing; every
   schema 1 field keeps its meaning. */
#define RSF_MODULE_DUMP_SCHEMA_VERSION 2u

/* Dump the module containing this process's main image. `module_base` may be null, in which case
   the process's own main module is used. Both output parameters may be null. */
rsf_dump_result rsf_dump_module(const void* module_base, const rsf_dump_options* options,
                                rsf_dump_report* report);

/* Measure the entropy of the code section of a loaded module without writing anything. */
rsf_dump_result rsf_measure_module_code(const void* module_base, double* entropy);

/* Write bytes over code in the main module, at an offset from its load base.

   For a protected executable this can only run after the code has been decrypted, which is what
   the entropy measurement above establishes. Patching earlier writes into ciphertext that is
   about to be overwritten.

   `expected` and `expected_count`, when given, must match the bytes already there. A patch whose
   surroundings have changed is a patch aimed at the wrong place, and refusing is much better than
   corrupting an instruction stream. Returns the previous bytes in `previous` when asked, so a
   caller can put them back. */
rsf_dump_result rsf_patch_code(uint32_t rva, const uint8_t* bytes, uint32_t count,
                               const uint8_t* expected, uint32_t expected_count,
                               uint8_t* previous);

/* Append the currently loaded modules to a text file. Which graphics runtime a game selects is
   only visible well after startup, so this has to be sampled late and repeatedly. Modules loaded
   before the dump prove nothing: static imports are mapped whichever renderer is later chosen. */
rsf_dump_result rsf_write_module_list(const char* path_utf8, const char* label);

/* Poll until the code section falls below the threshold, then dump. Intended for a worker thread,
   never for DllMain. Returns RSF_DUMP_ERROR_STILL_ENCRYPTED if the timeout expires first. */
rsf_dump_result rsf_dump_when_decrypted(const rsf_dump_options* options, uint32_t poll_interval_ms,
                                        uint32_t timeout_ms, uint32_t stable_samples,
                                        rsf_dump_report* report);

#ifdef __cplusplus
} /* extern "C" */
#endif

#endif /* RSF_MODULE_DUMP_H */
