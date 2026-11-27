/* SPDX-License-Identifier: MIT */
#ifndef RSF_FRAME_CAPTURE_PLATFORM_H
#define RSF_FRAME_CAPTURE_PLATFORM_H

#include <rescaleframe/frame_capture.h>

typedef struct rsf_capture_calls {
    void (*trigger)(void);
    void (*trigger_multiple)(uint32_t frames);
    uint32_t (*count)(void);
} rsf_capture_calls;

typedef int (*rsf_capture_connect)(void* library, const char* output_prefix_utf8,
                                   rsf_capture_calls* calls);

rsf_capture_result rsf_capture_initialise_impl(const char* library_path_utf8,
                                                const char* output_prefix_utf8,
                                                rsf_capture_connect connect);

#endif
