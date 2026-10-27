/* SPDX-License-Identifier: GPL-3.0-only */
/* Vendor call census: records every call a game makes into vendor upscaler, frame generation,
   latency and presentation entry points, with the caller's module and RVA.

   The library is a diagnostic probe. It forwards every call with the original arguments and return
   value and never touches the GPU. Install it as early as the process allows; it is safe to call
   rsf_census_install from DllMain. rsf_census_flush and rsf_census_shutdown wait on disk I/O and
   must not be called from DllMain. */

#ifndef RSF_VENDOR_CENSUS_H
#define RSF_VENDOR_CENSUS_H

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

#define RSF_CENSUS_ABI_VERSION 1u

typedef int32_t rsf_census_result;
#define RSF_CENSUS_OK ((rsf_census_result)0)
#define RSF_CENSUS_ERROR_INVALID_ARGUMENT ((rsf_census_result)-1)
#define RSF_CENSUS_ERROR_ABI_MISMATCH ((rsf_census_result)-2)
#define RSF_CENSUS_ERROR_ALREADY_INSTALLED ((rsf_census_result)-3)
#define RSF_CENSUS_ERROR_NOT_INSTALLED ((rsf_census_result)-4)
#define RSF_CENSUS_ERROR_IO ((rsf_census_result)-5)
#define RSF_CENSUS_ERROR_HOOK ((rsf_census_result)-6)

/* Option flags. */
#define RSF_CENSUS_FLAG_NO_STATIC_SCAN 0x1u /* skip the statically linked FSR/NGX signature scan */
#define RSF_CENSUS_FLAG_NO_DXGI 0x2u        /* do not hook DXGI, D3D12 and swapchain entry points */
#define RSF_CENSUS_FLAG_NO_VULKAN 0x4u      /* do not hook Vulkan entry points */
/* Enable the D3D12 debug layer and DRED before the first D3D12 device is created and record every
   debug-layer message (warnings and errors) as a d3d12_debug record. Slows rendering; for diagnosis. */
#define RSF_CENSUS_FLAG_D3D12_DEBUG 0x8u
/* Bind nothing but D3D12CreateDevice, for the debug layer, plus the fatal-exception flush: a baseline
   that separates the game's own D3D12 behaviour from the effect of the census hooks. */
#define RSF_CENSUS_FLAG_D3D12_DEBUG_ONLY 0x10u
/* Bisection switches: leave the kernelbase GetProcAddress detours, or the LoadLibrary detours, out.
   Without the LoadLibrary detours only modules mapped at install time are hooked. */
#define RSF_CENSUS_FLAG_NO_GETPROCADDRESS_HOOK 0x20u
#define RSF_CENSUS_FLAG_NO_LOADLIBRARY_HOOK 0x40u

/* Zero fields select defaults, so a zeroed structure with struct_size and abi_version set is valid. */
typedef struct rsf_census_options {
    uint32_t struct_size;  /* sizeof(rsf_census_options) */
    uint32_t abi_version;  /* RSF_CENSUS_ABI_VERSION */
    const char* output_dir_utf8; /* directory for census.jsonl and census-summary.json; created when
                                    missing; NULL selects the current directory */
    uint64_t byte_budget;        /* hard limit for census.jsonl; 0 selects 64 MiB */
    uint32_t full_record_cap;    /* full records per call site before sampling; 0 selects 32 */
    uint32_t sample_interval;    /* after the cap, record 1 call in this many; 0 selects 600 */
    uint32_t summary_period_ms;  /* census-summary.json rewrite period; 0 selects 2000 */
    uint32_t flags;              /* RSF_CENSUS_FLAG_* */
} rsf_census_options;

typedef struct rsf_census_counters {
    uint32_t struct_size; /* caller sets sizeof(rsf_census_counters) */
    uint32_t abi_version;
    uint64_t calls_seen;       /* every intercepted call, recorded or not */
    uint64_t records_queued;   /* records handed to the writer */
    uint64_t records_written;  /* records in census.jsonl */
    uint64_t records_dropped_queue;  /* queue full */
    uint64_t records_dropped_budget; /* byte budget exhausted */
    uint64_t records_dropped_size;   /* one record exceeded the line buffer */
    uint64_t bytes_written;    /* census.jsonl size */
    uint64_t present_index;    /* Present and vkQueuePresentKHR calls begun so far */
    uint32_t sites;            /* unique (function, caller module, RVA) sites */
    uint32_t site_overflow;    /* calls that found the site table full */
    uint32_t hooks_installed;  /* detours and thunks bound */
    uint32_t hook_conflicts;   /* a second implementation of an already bound entry, left alone */
    uint32_t modules_hooked;   /* vendor modules whose exports were hooked */
    uint32_t static_findings;  /* statically linked vendor code findings */
} rsf_census_counters;

/* Start recording. Hooks GetProcAddress and the LoadLibrary family, hooks vendor modules that are
   already loaded, and starts a writer thread. No LoadLibrary and no waiting on other threads. */
rsf_census_result rsf_census_install(const rsf_census_options* options);

/* Drain the queue to disk and rewrite census-summary.json. Blocks on disk I/O. */
rsf_census_result rsf_census_flush(void);

rsf_census_result rsf_census_get_counters(rsf_census_counters* out);

/* Flush, stop the writer and disable the hooks. Optional: a process normally ends with the hooks
   in place. Not for DllMain. */
rsf_census_result rsf_census_shutdown(void);

#ifdef __cplusplus
}
#endif

#endif
