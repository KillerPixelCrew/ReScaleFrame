// SPDX-License-Identifier: GPL-3.0-only
#pragma once
#include <rescaleframe/game_renderer.h>
#include <rescaleframe/native_fg_vulkan.h>
#include <stdint.h>

// Engine module to native runtime contract. Godot objects never cross this interface.
#define RSF_GODOT_ABI_VERSION 1u
typedef struct rsf_godot_setup {
    uint32_t struct_size, abi_version;
    const char* runtime_directory;
    const char* engine_version;
    const char* project_id;
    rsf_game_log_fn log;
    void* log_user;
} rsf_godot_setup;
typedef struct rsf_godot_settings {
    uint32_t struct_size, abi_version;
    uint32_t sr_backend, quality, dlss_preset;
    uint32_t fg_backend, fg_mode, generated_frames, latency_mode, frame_limit_us;
    uint32_t gameplay;
} rsf_godot_settings;
typedef struct rsf_godot_status {
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
} rsf_godot_status;
typedef struct rsf_godot_engine_services {
    uint32_t struct_size, abi_version;
    int (*configure)(const rsf_godot_settings*, uint64_t viewport_id);
    void (*reset_history)(void);
} rsf_godot_engine_services;

typedef void (*rsf_godot_vk_function)(void);
typedef rsf_godot_vk_function (*rsf_godot_vk_proc)(void* object, const char* name);
typedef struct rsf_godot_vk_image {
    uint64_t image;
    uint32_t format, width, height;
    uint32_t copy_source;
} rsf_godot_vk_image;
/* Driver callback lease. Every image belongs to device and remains live through
   the engine's submission fence. No Godot object crosses this interface. */
typedef struct rsf_godot_vk_packet {
    uint32_t struct_size, abi_version;
    void* instance;
    void* physical_device;
    void* device;
    rsf_godot_vk_proc instance_proc, device_proc;
    rsf_game_render_pass pass;
    rsf_godot_vk_image color, depth, motion, output;
} rsf_godot_vk_packet;

#if defined(RSF_GODOT_NATIVE_BUILD)
#define RSF_GODOT_EXPORT __declspec(dllexport)
#else
#define RSF_GODOT_EXPORT
#endif
#ifdef __cplusplus
extern "C" {
#endif
RSF_GODOT_EXPORT int rsfg_boot(const rsf_godot_setup* setup);
RSF_GODOT_EXPORT int rsfg_boot_vulkan(const rsf_godot_setup* setup);
RSF_GODOT_EXPORT int rsfg_vulkan_supported(void);
RSF_GODOT_EXPORT int rsfg_render_vulkan(void* command_buffer, const rsf_godot_vk_packet* packet);
RSF_GODOT_EXPORT int32_t rsfg_vk_create(const rsf_fgvk_setup* setup, void** owner, uint64_t* chain);
RSF_GODOT_EXPORT int32_t rsfg_vk_images(void* owner, uint32_t* count, uint64_t* images);
RSF_GODOT_EXPORT int32_t rsfg_vk_acquire(void* owner, uint64_t timeout, uint64_t semaphore, uint64_t fence, uint32_t* index);
RSF_GODOT_EXPORT int32_t rsfg_vk_present(void* owner, const void* present_info);
RSF_GODOT_EXPORT void rsfg_vk_submitted(void* const* commands, uint32_t count);
RSF_GODOT_EXPORT int32_t rsfg_vk_destroy(void* owner);
RSF_GODOT_EXPORT int32_t rsfg_vk_drain(void* owner);
RSF_GODOT_EXPORT int32_t rsfg_create_swapchain(void* factory, void* queue, void* window, const void* description, void** out);
RSF_GODOT_EXPORT int rsfg_configure(const rsf_godot_settings* settings);
RSF_GODOT_EXPORT int rsfg_status(rsf_godot_status* status);
RSF_GODOT_EXPORT int rsfg_settings(rsf_godot_settings* settings);
/* Synthetic diagnostic through the actual registered Windows PCL message. */
RSF_GODOT_EXPORT int rsfg_request_latency_ping(void);
RSF_GODOT_EXPORT void rsfg_cpu(const rsf_game_cpu_event* event);
RSF_GODOT_EXPORT void rsfg_render_begin(uint64_t source_frame_id);
RSF_GODOT_EXPORT int rsfg_render(void* device, void* queue, void* commands, const rsf_game_render_pass* pass);
RSF_GODOT_EXPORT void rsfg_submitted(void* queue, void* const* command_lists, uint32_t count);
RSF_GODOT_EXPORT void rsfg_present(void* swapchain);
RSF_GODOT_EXPORT int rsfg_shutdown(void);
RSF_GODOT_EXPORT int rsfg_attach_engine(const rsf_godot_engine_services* services);
RSF_GODOT_EXPORT int rsfg_configure_engine(const rsf_godot_settings* settings, uint64_t viewport_id);
RSF_GODOT_EXPORT void rsfg_reset_history(void);
#ifdef __cplusplus
}
#endif
