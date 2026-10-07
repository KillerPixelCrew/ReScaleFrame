/* SPDX-License-Identifier: GPL-3.0-only */
#ifndef RSF_AC7_MOTION_CAPTURE_H
#define RSF_AC7_MOTION_CAPTURE_H

#include <stdint.h>
#include <rescaleframe/game_renderer.h>

#ifdef __cplusplus
extern "C" {
#endif

/* Synchronous diagnostic callback; message is borrowed for this call. */
typedef void (*rsf_ac7_capture_log_fn)(void* user, const char* message);

/* Configure before D3D11 creation, install engine observations only after decryption.
   The capture forwards engine decisions unchanged. F9 arms 60 present intervals; pipeline
   samples are taken in intervals 0, 30 and 59. Guarded post-process and widget producer scopes
   are saved to native.jsonl. GPU and CPU timelines remain separately labelled. */
/* Copies directory; log/user remain borrowed through shutdown. Empty directory disables capture. */
void rsf_ac7_motion_capture_configure(const char* directory, rsf_ac7_capture_log_fn log, void* user);
/* Returns nonzero when velocity observers install; optional root observations can refuse separately. */
int rsf_ac7_motion_capture_install(void);
/* Stop collection, detach research callbacks and remove this module's observation hooks. */
void rsf_ac7_motion_capture_shutdown(void);
/* Creation/upload callbacks copy bytecode or CPU bytes into bounded diagnostic registries.
   shader/buffer are identity keys only. stage is VS/PS/GS/HS/DS/CS = 0/1/2/3/4/5. */
void rsf_ac7_motion_capture_shader(void* shader, uint32_t stage, const void* bytes, uint32_t size);
void rsf_ac7_motion_capture_buffer(void* user, void* buffer, const void* initial,
                                  uint32_t bytes, uint32_t bind_flags);
void rsf_ac7_motion_capture_upload(void* buffer, const void* bytes, uint32_t size);
/* Thread-safe one-shot request, consumed at the next observed Present. */
void rsf_ac7_motion_capture_request(void);
/* Native controller supplies queued execution identity; capture must not detour its owner again. */
void rsf_ac7_motion_capture_native_owner(uint32_t enabled);
void rsf_ac7_motion_capture_native_pass(const rsf_game_render_pass* pass, uint32_t begin);
/* Present-owner call: poll F9/request, advance 60 intervals, and install callbacks at 0/30/59.
   Returns 1 and writes a bounded, terminated prefix for paired images at sampled intervals;
   returns 0 otherwise. prefix/capacity are required and swapchain is borrowed for this call. */
int rsf_ac7_motion_capture_present(void* swapchain, char* prefix, uint32_t capacity);

/* Native velocity-eligibility observations. Distances/radius use engine units; mobility/relevance
   are build-specific enum/bit fields. fields_valid gates interpretation of every later field. */
typedef struct rsf_ac7_velocity_facts {
    uint32_t fields_valid, accepted, visible, mobility, relevance, has_velocity_called;
    uint32_t has_velocity, camera_cut, always_velocity, history_checked, history_found;
    float radius, distance_squared, lod_factor, minimum_size;
} rsf_ac7_velocity_facts;

/* Pure interpretation of observed facts; returns immutable static text. Unknown causes stay broad. */
const char* rsf_ac7_velocity_reason(const rsf_ac7_velocity_facts* facts);

#ifdef __cplusplus
}
#endif
#endif
