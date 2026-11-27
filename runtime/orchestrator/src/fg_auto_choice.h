// SPDX-License-Identifier: MIT
#pragma once
#include <stdint.h>

// Private to consumers that link OrchestratorCore statically; not a Runtime DLL export
// (rust_exports.c), so the Runtime's exported names and ordinals are unchanged.
#ifdef __cplusplus
extern "C" {
#endif
/* FG Auto among the providers a host can serve (bit n = provider n): preferred, the adapter's
   rsf_fg_default_backend (zero for none), when served, otherwise FSR3 when served, otherwise
   zero. Off, Auto and unknown IDs are never chosen. Defined in fg_choice.rs. */
uint32_t rsf_fg_select_auto_backend(uint32_t preferred, uint32_t choices);
#ifdef __cplusplus
}
#endif
