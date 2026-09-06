/* SPDX-License-Identifier: GPL-3.0-only */
/* In-application RenderDoc capture, driven from inside the game process.

   RenderDoc's own launcher cannot easily reach a Steam- and Proton-started game, but any module
   already loaded in the process can load renderdoc.dll and drive its API. Skyrim Community
   Shaders uses the same approach; the vendor-extension handling below follows their finding.

   This captures. It does not analyse, and a D3D11 capture can only be replayed on Windows. */

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

/* Load renderdoc.dll from an explicit path and take its API. Must run before the graphics device
   is created, which is why the carrier DLL loads as a static import.

   `output_prefix_utf8` becomes the capture file path template, so captures land beside the other
   research output rather than in the game directory. */
rsf_capture_result rsf_capture_initialise(const char* library_path_utf8,
                                          const char* output_prefix_utf8);

/* Queue a capture of the next `frames` presented frames. */
rsf_capture_result rsf_capture_trigger(uint32_t frames);

/* Number of captures written so far, or 0 when unavailable. */
uint32_t rsf_capture_count(void);

#ifdef __cplusplus
} /* extern "C" */
#endif

#endif /* RSF_FRAME_CAPTURE_H */
