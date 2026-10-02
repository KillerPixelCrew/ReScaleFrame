/* SPDX-License-Identifier: GPL-3.0-only */
#ifndef RSF_AC7_MOTION_CAPTURE_H
#define RSF_AC7_MOTION_CAPTURE_H

#include <stdint.h>
#include <rescaleframe/game_renderer.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef void (*rsf_ac7_capture_log_fn)(void* user, const char* message);

/* Configure before D3D11 creation, install engine observations only after decryption.
   The capture forwards engine decisions unchanged. F9 arms 60 present intervals; pipeline
   samples are taken in intervals 0, 30 and 59. Guarded post-process and widget producer scopes
   are saved to native.jsonl. GPU and CPU timelines remain separately labelled. */
void rsf_ac7_motion_capture_configure(const char* directory, rsf_ac7_capture_log_fn log, void* user);
int rsf_ac7_motion_capture_install(void);
void rsf_ac7_motion_capture_shutdown(void);
void rsf_ac7_motion_capture_shader(void* shader, uint32_t stage, const void* bytes, uint32_t size);
void rsf_ac7_motion_capture_buffer(void* user, void* buffer, const void* initial,
                                  uint32_t bytes, uint32_t bind_flags);
void rsf_ac7_motion_capture_upload(void* buffer, const void* bytes, uint32_t size);
void rsf_ac7_motion_capture_request(void);
/* Native controller supplies queued execution identity; capture must not detour its owner again. */
void rsf_ac7_motion_capture_native_owner(uint32_t enabled);
void rsf_ac7_motion_capture_native_pass(const rsf_game_render_pass* pass, uint32_t begin);
/* Returns a sample prefix when paired backend images should be requested for the next evaluation. */
int rsf_ac7_motion_capture_present(void* swapchain, char* prefix, uint32_t capacity);

typedef struct rsf_ac7_velocity_facts {
    uint32_t fields_valid, accepted, visible, mobility, relevance, has_velocity_called;
    uint32_t has_velocity, camera_cut, always_velocity, history_checked, history_found;
    float radius, distance_squared, lod_factor, minimum_size;
} rsf_ac7_velocity_facts;

/* Pure interpretation of observed native facts. Unknown reasons stay unknown. */
const char* rsf_ac7_velocity_reason(const rsf_ac7_velocity_facts* facts);

#ifdef __cplusplus
}
#endif
#endif
