// SPDX-License-Identifier: MIT
#pragma once
#include <rescaleframe/game_renderer.h>
#include <rescaleframe/native_fg_vulkan.h>
#include <rescaleframe/native_sr_split_vulkan.h>
#include <rescaleframe/sharpening_settings.h>
#include <stdint.h>

/* The Runtime's Vulkan host for an engine integration that owns its Vulkan device and WSI
   (runtime/orchestrator/src/vulkan_host.rs). The host owns the session: Streamline, XeSS and
   latency SDK lifetime, the creation resolver that adds vendor device requirements, provider
   admission and Auto resolution, the FG presenter that replaces the engine's primary swapchain,
   the SR session with its retirement, the frame capture and the status. The integration reports
   its device, primary window, passes and Present calls, and presents what the host returns.
   Integrations link these entries statically with the Runtime archive; one host per process.
   Calls are serialized by the host. */
#define RSF_VKHOST_ABI_VERSION 3u
#define RSF_VKHOST_SETTINGS_ABI_VERSION 2u
/* rsf_vkhost_capabilities: the engine can split its graph recording (rsf_fsr4vk_handoff). */
#define RSF_VKHOST_GRAPH_HANDOFF 1u

typedef struct rsf_vkhost_setup {
    uint32_t struct_size, abi_version;
    const char* runtime_directory;
    const char* engine_version;
    const char* project_id;
    rsf_game_log_fn log;
    void* log_user;
} rsf_vkhost_setup;

/* A requested configuration. sharpening.struct_size zero requests the Runtime defaults.
   gameplay zero keeps FG off on every screen. */
typedef struct rsf_vkhost_settings {
    uint32_t struct_size, abi_version;
    uint32_t sr_backend, quality, dlss_preset;
    uint32_t fg_backend, fg_mode, generated_frames, latency_mode, frame_limit_us;
    uint32_t gameplay;
    rsf_sharpening_options sharpening;
} rsf_vkhost_settings;

/* The host fills the SR, FG, presentation and latency facts; window and input facts are the
   integration's to add. */
typedef struct rsf_vkhost_status {
    uint32_t struct_size, abi_version;
    uint64_t session_id;
    uint32_t requested_sr, active_sr, render_width, render_height, output_width, output_height;
    int32_t sr_result, fg_result;
    uint32_t fg_backend, fg_available, fg_active, max_generated_frames, generated_frames;
    uint32_t latency_available, latency_mode, reason, pcl_available, latency_report_available;
    uint64_t evaluated_frames, refused_frames, tagged_frames, sleep_calls, marker_calls;
    uint64_t total_presented, generated_presented;
    uint32_t presentation_statistics_valid;
    uint32_t presentation_statistics_flags;
    uint64_t source_frame_id, provider_generation, generated_callbacks;
    uint32_t window_foreground, window_visible, foreground_process_id;
    uint64_t keyboard_inputs, mouse_inputs, controller_inputs, pcl_pings;
    uint32_t latency_owner;
    uint64_t input_substitutes, order_violations, derived_boundaries;
} rsf_vkhost_status;

typedef struct rsf_vkhost_image {
    uint64_t image;
    uint32_t format, width, height;
    uint32_t copy_source;
} rsf_vkhost_image;

/* One SR (RSF_GAME_RENDER_SR) or HUD-less (RSF_GAME_RENDER_FINAL_SCENE) pass. Every image
   belongs to device and stays live through the engine's submission fence. */
typedef struct rsf_vkhost_packet {
    uint32_t struct_size, abi_version;
    void* instance;
    void* physical_device;
    void* device;
    rsf_vk_get_proc instance_proc, device_proc;
    rsf_game_render_pass pass;
    rsf_vkhost_image color, depth, motion, output;
} rsf_vkhost_packet;

#ifdef __cplusplus
extern "C" {
#endif
/* This build carries the Vulkan SR providers. */
int32_t rsf_vkhost_supported(void);
/* Before rsf_vkhost_boot: RSF_VKHOST_* engine capabilities. */
int32_t rsf_vkhost_capabilities(uint32_t flags);
int32_t rsf_vkhost_boot(const rsf_vkhost_setup* setup);
int32_t rsf_vkhost_booted(void);
/* The vkGetInstanceProcAddr the engine creates its instance and devices through. */
rsf_vk_get_proc rsf_vkhost_resolver(void);
/* The engine's primary window: the FG presenter's host window. */
void rsf_vkhost_window(void* window);
/* Admission, then the request; zero refuses and keeps the previous settings. */
int32_t rsf_vkhost_configure(const rsf_vkhost_settings* settings);
/* Writes the current settings; returns booted. */
int32_t rsf_vkhost_settings_get(rsf_vkhost_settings* settings);
/* Writes the status, also when not booted; returns booted. */
int32_t rsf_vkhost_status_get(rsf_vkhost_status* status);
/* explicit: the engine reports submission and Present itself. */
void rsf_vkhost_render_begin(uint64_t source_frame_id, uint32_t explicit_execution);
/* The engine's own vkQueuePresentKHR of an admitted frame, for the frame capture. */
void rsf_vkhost_capture_present(const void* present_info, int32_t result, uint32_t begin);
/* Presenter drain, SR retirement, capture, XeSS and SDK shutdown; zero keeps the session. */
int32_t rsf_vkhost_shutdown(void);

/* The FG presenter replacing the engine's primary swapchain (rsf_fgvk_* semantics). */
int32_t rsf_vkhost_create(const rsf_fgvk_setup* setup, void** owner, uint64_t* chain);
void rsf_vkhost_capture_setup(const rsf_fgvk_setup* setup);
int32_t rsf_vkhost_capture_release(void* queue_user);
int32_t rsf_vkhost_images(void* owner, uint32_t* count, uint64_t* images);
uint32_t rsf_vkhost_image_layout(void* owner);
/* VK_ERROR_OUT_OF_DATE_KHR when the resolved provider changed: recreate the swapchain. */
int32_t rsf_vkhost_acquire(void* owner, uint64_t timeout, uint64_t semaphore, uint64_t fence, uint32_t* index);
int32_t rsf_vkhost_present(void* owner, const void* present_info);
/* A command buffer whose previous use retired is recorded again. */
void rsf_vkhost_recording_begin(void* commands);
void rsf_vkhost_submitted(void* const* commands, uint32_t count);
int32_t rsf_vkhost_destroy(void* owner);
int32_t rsf_vkhost_drain(void* owner);

/* One pass recorded into command_buffer: FG inputs, then SR or the spatial fallback. */
int rsf_vkhost_render(void* command_buffer, const rsf_vkhost_packet* packet);
/* As rsf_vkhost_render with the engine's graph split; recording continues on
   *continuation_commands. */
int rsf_vkhost_render_handoff(void* command_buffer, const rsf_vkhost_packet* packet,
    const rsf_fsr4vk_handoff* handoff, void** continuation_commands);
#ifdef __cplusplus
}
#endif
