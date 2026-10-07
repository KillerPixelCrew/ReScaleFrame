/* SPDX-License-Identifier: GPL-3.0-only */
#pragma once
#include <rescaleframe/runtime.h>
#include <rescaleframe/frame_generation.h>
#include <wchar.h>
#ifdef __cplusplus
extern "C" {
#endif
typedef struct rsf_fg_choice {
    /* Selected provider ID and bit-per-provider available choices, including bit zero for Off. */
    uint32_t backend, choices;
    /* Last save result, independent of a live presentation provider's switch result. */
    int32_t last_result;
} rsf_fg_choice;
/* Zero is Off. The mask names implemented provider IDs, not GPU compatibility.
   Selection saves the next-start provider without replacing a live chain. */
RSF_RUNTIME_API uint32_t rsf_fg_choice_start(const wchar_t* path, uint32_t fallback, uint32_t choices);
/* Persist [Rendering] FrameGeneration and update the choice only after a successful write.
   An absent path fails saving. Calls are serialized internally; path is copied at start. */
RSF_RUNTIME_API rsf_backend_result rsf_fg_choice_save(uint32_t backend);
/* Return a copied snapshot; no pointer into shared settings escapes. */
RSF_RUNTIME_API rsf_fg_choice rsf_fg_choice_get(void);
#ifdef __cplusplus
}
#endif
