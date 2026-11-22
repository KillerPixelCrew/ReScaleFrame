// SPDX-License-Identifier: MIT
#pragma once
#include <stdint.h>

// Engine sources inserted by eng/patch-godot.py include this through rsf_godot_engine.h, so it
// names the api.h records without including their SDK headers.
struct rsf_godot_setup;
struct rsf_godot_status;
struct rsf_godot_settings;
struct rsf_godot_engine_services;
struct rsf_godot_vk_packet;
struct rsf_godot_vk_handoff;
struct rsf_game_render_pass;

// Private native host table in the SCons Rust object (native.rs). Rust loads the library,
// resolves each rsfg_* export once and gates every forwarder on the host state. The engine
// supplies only its Godot facts: the UTF-16 library path and the Vulkan driver query.

#ifdef __cplusplus
extern "C" {
#endif
// Main thread, once during boot. A nonzero error is the Windows load error.
uint32_t rsf_godot_native_load(const uint16_t *path, uint32_t (*vulkan_driver)(void), uint32_t *error);
uint32_t rsf_godot_native_boot(uint32_t vulkan, const struct rsf_godot_setup *setup);
void rsf_godot_native_capabilities(uint32_t flags);
uint32_t rsf_godot_native_attach(const struct rsf_godot_engine_services *services);
uint32_t rsf_godot_native_status(struct rsf_godot_status *status);
uint32_t rsf_godot_native_shutdown(void);
uint32_t rsf_godot_native_configure(const struct rsf_godot_settings *settings);
void rsf_godot_native_vk_window(void *window);

// Main loop thread: frame.rs's report leaf.
void rsf_godot_native_report_cpu(uint32_t stage, uint64_t source_frame_id, uint32_t render_expected,
    uint32_t input_kind, uint32_t unframed_input);
// Rendering server thread: host.rs's render-begin leaf.
void rsf_godot_native_render_begin(uint64_t source_frame_id);

// Any thread.
uint32_t rsf_godot_native_vulkan_enabled(void);
uint32_t rsf_godot_native_vk_execution_supported(void);
uint32_t rsf_godot_native_vk_execution(uint32_t role, uint64_t session, uint64_t source, uint64_t window, uint32_t begin);
uint32_t rsf_godot_native_vk_present_marker(uint64_t window, uint32_t begin);
uint64_t rsf_godot_native_vk_resolver(void);

// Driver sites.
uint32_t rsf_godot_native_create_swapchain(void *factory, void *queue, void *window, const void *description, void **out);
void rsf_godot_native_submitted(void *queue, void *const *lists, uint32_t count);
void rsf_godot_native_present(void *swapchain);
int32_t rsf_godot_native_vk_native_present(uint64_t callback, void *queue, const void *info);
int32_t rsf_godot_native_vk_wsi_call(int32_t (*callback)(void *), void *user);
int32_t rsf_godot_native_vk_foreign_call(int32_t (*callback)(void *), void *user);
uint32_t rsf_godot_native_vk_create(const void *setup, void **owner, uint64_t *chain);
void rsf_godot_native_vk_capture_setup(const void *setup);
uint32_t rsf_godot_native_vk_capture_release(void *queue_user);
int32_t rsf_godot_native_vk_images(void *owner, uint32_t *count, uint64_t *images);
uint32_t rsf_godot_native_vk_image_layout(void *owner); // PRESENT_SRC without a proxy or host
int32_t rsf_godot_native_vk_acquire(void *owner, uint64_t timeout, uint64_t semaphore, uint64_t fence, uint32_t *index);
int32_t rsf_godot_native_vk_present(void *owner, const void *info);
void rsf_godot_native_vk_recording_begin(void *commands);
void rsf_godot_native_vk_submitted(void *const *commands, uint32_t count);
uint32_t rsf_godot_native_vk_destroy(void *owner);
uint32_t rsf_godot_native_vk_drain(void *owner);

// packet.rs's driver routing.
int rsf_godot_native_render(void *device, void *queue, void *commands, const struct rsf_game_render_pass *pass);
int rsf_godot_native_render_vulkan(void *commands, const struct rsf_godot_vk_packet *packet);
int rsf_godot_native_render_vulkan_handoff(void *commands, const struct rsf_godot_vk_packet *packet,
    const struct rsf_godot_vk_handoff *handoff, void **continuation);
#ifdef __cplusplus
}
#endif
