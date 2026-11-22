/* SPDX-License-Identifier: MIT */
#pragma once
#include <rescaleframe/backend.h>
#include <rescaleframe/log.h>
#include <windows.h>

#ifdef __cplusplus
extern "C" {
#endif
/* Loads sl.interposer.dll from an absolute path, after the signature check when one is required.
   This puts a signed NVIDIA module into a game process, so the path alone must never decide which
   module answers: the search is the same fixed one for every Streamline owner. Logs why a load
   was refused and returns null. The signature check is Streamline's own (sl_security.h) and lives in
   runtime/orchestrator/src/streamline_sr/signature.rs. Defined in Rust (runtime/orchestrator/src/streamline_sr/native.rs). */
HMODULE rsf_streamline_load_interposer(const wchar_t* path, uint32_t require_signature,
    rsf_backend_log_fn log, void* user);
#ifdef __cplusplus
}
#endif
