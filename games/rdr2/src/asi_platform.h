/* SPDX-License-Identifier: MIT */
#pragma once
#include "runtime_setup.h"
#include <rescaleframe/frame_capture.h>
#include <rescaleframe/module_dump.h>
#include <rescaleframe/vendor_census.h>

/* C link services supplied by the ASI. The separately linked game plugin has no dependency
   on the optional RenderDoc library, module-dump library or census. */
typedef struct rsf_rdr2_asi_services {
    rsf_rdr2_fingerprint_fn fingerprint;
    rsf_dump_result (*measure)(const void*, double*);
    rsf_dump_result (*dump)(const rsf_dump_options*, uint32_t, uint32_t, uint32_t, rsf_dump_report*);
    rsf_dump_result (*modules)(const char*, const char*);
    rsf_census_result (*census_install)(const rsf_census_options*);
    rsf_census_result (*census_flush)(void);
    rsf_census_result (*census_counters)(rsf_census_counters*);
    rsf_capture_result (*frame_initialise)(const char*, const char*);
    rsf_capture_result (*frame_trigger)(uint32_t);
    bool (*native_capture)(const wchar_t*);
    bool (*social_current_index)(void*, uint32_t*);
} rsf_rdr2_asi_services;

#ifdef __cplusplus
extern "C" {
#endif
bool rsf_rdr2_social_club_current_index(void* swapchain, uint32_t* index);
void rsf_rdr2_asi_process_attach(void* module, const rsf_rdr2_asi_services*);
#ifdef __cplusplus
}
#endif
