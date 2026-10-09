// SPDX-License-Identifier: GPL-3.0-only
#pragma once
#include <rescaleframe/frame_generation.h>
// Optional process-lifetime observation. Never changes arguments, return values or hook chains.
// NVAPI's nvapi_QueryInterface, taken from the module Streamline already initialized. Never loads
// NVAPI itself, and is null when the diagnostics are not built or the module is absent.
using RsfNvapiQuery = void*(__cdecl*)(unsigned int);
RsfNvapiQuery rsf_nvapi_query();
void rsf_reflex_audit_start(rsf_backend_log_fn log, void* user);
void rsf_reflex_audit_stop();
void rsf_reflex_audit_report();
