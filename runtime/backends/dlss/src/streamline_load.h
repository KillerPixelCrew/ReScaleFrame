/* SPDX-License-Identifier: MIT */
#pragma once
#include <rescaleframe/backend.h>
#include <rescaleframe/log.h>
#include <windows.h>

#ifdef __cplusplus
extern "C" {
#endif
/* Streamline's own verification of an interposer's embedded signature (sl_security.h). Defined
   next to the Streamline headers in dlss_streamline.cpp, the one place that header may be
   included. Answers 0 in a build that cannot verify signatures. */
uint32_t rsf_dlss_verify_runtime_signature(const wchar_t* path);

/* Loads sl.interposer.dll from an absolute path, after the signature check when one is required.
   This puts a signed NVIDIA module into a game process, so the path alone must never decide which
   module answers: the search is the same fixed one for every Streamline owner. Logs why a load
   was refused and returns null. Defined in Rust (runtime/orchestrator/src/streamline_sr/native.rs). */
HMODULE rsf_streamline_load_interposer(const wchar_t* path, uint32_t require_signature,
    rsf_backend_log_fn log, void* user);
#ifdef __cplusplus
}
#endif
