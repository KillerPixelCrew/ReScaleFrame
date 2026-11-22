/* SPDX-License-Identifier: MIT */
#ifndef RSF_DLSS_REFLEX_AUDIT_H
#define RSF_DLSS_REFLEX_AUDIT_H
#include <rescaleframe/frame_generation.h>

#ifdef __cplusplus
extern "C" {
#endif

/* Optional process-lifetime NVAPI observation, implemented in reflex_audit.rs.
 * Never changes arguments, return values or hook chains. */
void rsf_reflex_audit_start(rsf_backend_log_fn log, void* user);
void rsf_reflex_audit_stop(void);
void rsf_reflex_driver_sleep(void* device, rsf_backend_log_fn log, void* user);

#ifdef __cplusplus
}
#endif
#endif
