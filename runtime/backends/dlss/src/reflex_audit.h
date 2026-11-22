// SPDX-License-Identifier: MIT
#pragma once
#include <rescaleframe/frame_generation.h>
// Optional process-lifetime observation. Never changes arguments, return values or hook chains.
void rsf_reflex_audit_start(rsf_backend_log_fn log, void* user);
void rsf_reflex_audit_stop();
void rsf_reflex_driver_sleep(void* device, rsf_backend_log_fn log, void* user);
