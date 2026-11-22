// SPDX-License-Identifier: MIT
#pragma once
#include <stdbool.h>
#include <rescaleframe/unity_bridge.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef struct rsf_uvk_engine_instance {
    uint64_t instance, device, physical_device, queue;
    uint32_t queue_family;
} rsf_uvk_engine_instance;

/* Values copied from the Unity borrow. Layout and aspect use Vulkan values. */
typedef struct rsf_uvk_engine_image {
    uint64_t image;
    int32_t layout;
    uint32_t aspect;
} rsf_uvk_engine_image;

/* Calls into the Unity interface or other foreign callbacks catch exceptions before returning
   to Rust. A status of zero indicates a refused call, or a caught exception where applicable. */
bool rsf_uvk_foreign_available(void);
bool rsf_uvk_foreign_instance(rsf_uvk_engine_instance*);
bool rsf_uvk_foreign_texture(void*, int32_t layout, uint32_t access, rsf_uvk_engine_image*);
bool rsf_uvk_foreign_queue(int event_id, void* event);
void rsf_uvk_foreign_log(const rsf_game_host_services*, const char*);
bool rsf_uvk_foreign_pass(const rsf_game_host_services*, void*, const rsf_game_render_pass*, uint32_t phase);
bool rsf_uvk_foreign_present(uint64_t queue, const void* info, int32_t* result);

bool rsf_uvk_bridge_render(int event_id, int base_event, const rsf_unity_packet*, const rsf_game_host_services*, bool reconfigure);
void rsf_uvk_bridge_execute(const void* event);
int32_t rsf_uvk_bridge_present(uint64_t queue, const void* info);
void rsf_uvk_bridge_retirement_fault(void);
bool rsf_uvk_bridge_graphics(void** device, void** queue);
bool rsf_uvk_bridge_create_window(void);
bool rsf_uvk_bridge_update_window(uint32_t width, uint32_t height);
bool rsf_uvk_bridge_owns_window(void*);
struct rsf_unity_vulkan_device_handles;
bool rsf_uvk_foreign_create_device(const rsf_game_host_services*, struct rsf_unity_vulkan_device_handles*);
bool rsf_uvk_bridge_create_device(const rsf_game_host_services*, const rsf_uvk_engine_instance*,
    struct rsf_unity_vulkan_device_handles*,
    void (*instance_lookup)(void), void (*device_lookup)(void));

#ifdef __cplusplus
}
#endif
