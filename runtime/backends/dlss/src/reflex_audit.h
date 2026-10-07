// SPDX-License-Identifier: GPL-3.0-only
#pragma once
#include <rescaleframe/frame_generation.h>
// Optional process-lifetime observation. Never changes arguments, return values or hook chains.
// Install or reset a bounded diagnostic sink. Callback/log user must survive until stop returns.
// Observers invoke the sink under their mutex, so it must not reenter this audit API.
void rsf_reflex_audit_start(rsf_backend_log_fn log, void* user);
// Detach/drain the sink; resident detours remain installed to preserve later hook chains.
void rsf_reflex_audit_stop();
// Emit the latest observed ReflexSync policy, if available. No-op without NVAPI build support.
void rsf_reflex_audit_report();
