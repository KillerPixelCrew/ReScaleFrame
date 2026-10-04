/* SPDX-License-Identifier: GPL-3.0-only */
#pragma once
#include <rescaleframe/runtime.h>
#include <rescaleframe/frame_generation.h>
#include <wchar.h>
#ifdef __cplusplus
extern "C" {
#endif
typedef struct rsf_fg_choice {
    uint32_t backend, choices;
    int32_t last_result;
} rsf_fg_choice;
/* Zero is Off. The mask names implemented provider IDs, not GPU compatibility.
   Selection saves the next-start provider without replacing a live chain. */
RSF_RUNTIME_API uint32_t rsf_fg_choice_start(const wchar_t* path, uint32_t fallback, uint32_t choices);
RSF_RUNTIME_API rsf_backend_result rsf_fg_choice_save(uint32_t backend);
RSF_RUNTIME_API rsf_fg_choice rsf_fg_choice_get(void);
#ifdef __cplusplus
}
#endif
