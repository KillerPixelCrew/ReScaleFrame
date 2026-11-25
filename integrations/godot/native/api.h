// SPDX-License-Identifier: MIT
#pragma once
#include <rescaleframe/native_vulkan_host.h>
#include <stdint.h>

// Engine module to native runtime contract. Godot objects never cross this interface. The setup,
// settings, status and Vulkan pass records are the Runtime's Vulkan host records
// (native_vulkan_host.h), which the D3D12 path shares; the layouts and versions are unchanged.
#define RSF_GODOT_SETTINGS_ABI_VERSION RSF_VKHOST_SETTINGS_ABI_VERSION
#define RSF_GODOT_ABI_VERSION RSF_VKHOST_ABI_VERSION
typedef rsf_vkhost_setup rsf_godot_setup;
typedef rsf_vkhost_settings rsf_godot_settings;
typedef rsf_vkhost_status rsf_godot_status;
typedef struct rsf_godot_engine_services {
    uint32_t struct_size, abi_version;
    int (*configure)(const rsf_godot_settings*, uint64_t viewport_id);
    void (*reset_history)(void);
} rsf_godot_engine_services;

typedef rsf_vk_function rsf_godot_vk_function;
typedef rsf_vk_get_proc rsf_godot_vk_proc;
typedef rsf_vkhost_image rsf_godot_vk_image;
/* Driver callback lease. Every image belongs to device and remains live through
   the engine's submission fence. No Godot object crosses this interface. */
typedef rsf_vkhost_packet rsf_godot_vk_packet;

#define RSF_GODOT_VK_HANDOFF_ABI_VERSION RSF_SRVK_SPLIT_ABI_VERSION
#define RSF_GODOT_VK_GRAPH_HANDOFF RSF_VKHOST_GRAPH_HANDOFF
/* Recording-only graph lease. Both sections are submitted by the engine with
   its original acquire/transfer dependencies, including on provider failure. */
typedef rsf_fsr4vk_handoff rsf_godot_vk_handoff;

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
RSF_GODOT_EXPORT int rsfg_vk_host_capabilities(uint32_t flags);
RSF_GODOT_EXPORT rsf_godot_vk_proc rsfg_vk_resolver(void);
RSF_GODOT_EXPORT void rsfg_vk_window(void* window);
RSF_GODOT_EXPORT int rsfg_vk_present_marker(uint64_t window, uint32_t begin);
/* Optional engine execution receipt. FRAME announces a source for capture; SUBMISSION and
   PRESENT bracket actual driver calls. The engine supplies the presenting window in window_key. */
RSF_GODOT_EXPORT int rsfg_vk_execution(const rsf_game_render_pass* pass, uint32_t begin);
RSF_GODOT_EXPORT int32_t rsfg_vk_native_present(uint64_t callback, void* queue, const void* info);
/* Synchronous callback lease containing only native Vulkan data. */
RSF_GODOT_EXPORT int32_t rsfg_vk_wsi_call(int32_t (*callback)(void*), void* user);
RSF_GODOT_EXPORT int32_t rsfg_vk_foreign_call(int32_t (*callback)(void*), void* user);
RSF_GODOT_EXPORT int rsfg_render_vulkan(void* command_buffer, const rsf_godot_vk_packet* packet);
RSF_GODOT_EXPORT int rsfg_render_vulkan_handoff(void* command_buffer, const rsf_godot_vk_packet* packet,
    const rsf_godot_vk_handoff* handoff, void** continuation_commands);
RSF_GODOT_EXPORT int32_t rsfg_vk_create(const rsf_fgvk_setup* setup, void** owner, uint64_t* chain);
/* Readback ownership is independent of the optional FG presenter. */
RSF_GODOT_EXPORT void rsfg_vk_capture_setup(const rsf_fgvk_setup* setup);
RSF_GODOT_EXPORT int32_t rsfg_vk_capture_release(void* queue_user);
RSF_GODOT_EXPORT int32_t rsfg_vk_images(void* owner, uint32_t* count, uint64_t* images);
RSF_GODOT_EXPORT uint32_t rsfg_vk_image_layout(void* owner);
RSF_GODOT_EXPORT int32_t rsfg_vk_acquire(void* owner, uint64_t timeout, uint64_t semaphore, uint64_t fence, uint32_t* index);
RSF_GODOT_EXPORT int32_t rsfg_vk_present(void* owner, const void* present_info);
RSF_GODOT_EXPORT void rsfg_vk_recording_begin(void* commands);
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
