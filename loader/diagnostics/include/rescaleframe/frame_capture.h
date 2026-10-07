/* SPDX-License-Identifier: GPL-3.0-only */
/* Optional process-global RenderDoc API binding for early in-game capture. Configure on
   one startup thread before concurrent trigger/count calls; this module provides no lock or
   shutdown. Loaded capture-library references remain for the process lifetime. */

#ifndef RSF_FRAME_CAPTURE_H
#define RSF_FRAME_CAPTURE_H

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef int32_t rsf_capture_result;
#define RSF_CAPTURE_OK ((rsf_capture_result)0)
#define RSF_CAPTURE_ERROR_INVALID_ARGUMENT ((rsf_capture_result)-1)
#define RSF_CAPTURE_ERROR_LIBRARY_MISSING ((rsf_capture_result)-2)
#define RSF_CAPTURE_ERROR_API_MISSING ((rsf_capture_result)-3)
#define RSF_CAPTURE_ERROR_NOT_READY ((rsf_capture_result)-4)

/* Load the supplied DLL path and negotiate RenderDoc API 1.6.0 before the graphics
   device is created. Caller supplies an explicit path; no fallback path is attempted. Optional
   output_prefix_utf8 becomes the file template. Null library path returns INVALID_ARGUMENT;
   subsequent calls after successful initialization return OK without changing configuration.
   Disable RenderDoc hotkeys/overlay and permit NVIDIA vendor extensions for the observed hybrid
   device path. Capture configuration does not establish replay or DLSS compatibility. */
rsf_capture_result rsf_capture_initialise(const char* library_path_utf8,
                                          const char* output_prefix_utf8);

/* Queue a capture on the RenderDoc API; frames <= 1 requests a single capture, otherwise
   request that many presented frames. Return NOT_READY until initialization succeeds.
   OK means requested, not that a capture file has been written. */
rsf_capture_result rsf_capture_trigger(uint32_t frames);

/* Number of captures written so far, or 0 when unavailable. */
uint32_t rsf_capture_count(void);

#ifdef __cplusplus
} /* extern "C" */
#endif

#endif /* RSF_FRAME_CAPTURE_H */
