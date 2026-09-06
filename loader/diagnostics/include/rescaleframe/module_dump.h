/* SPDX-License-Identifier: GPL-3.0-only */
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
} rsf_dump_options;

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
} rsf_dump_report;

/* Dump the module containing this process's main image. `module_base` may be null, in which case
   the process's own main module is used. Both output parameters may be null. */
rsf_dump_result rsf_dump_module(const void* module_base, const rsf_dump_options* options,
                                rsf_dump_report* report);

/* Measure the entropy of the code section of a loaded module without writing anything. */
rsf_dump_result rsf_measure_module_code(const void* module_base, double* entropy);

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
