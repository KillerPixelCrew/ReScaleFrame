/* SPDX-License-Identifier: MIT */
#pragma once
/* Internal boundary between the Unity package's Vulkan bridge (integrations/unity/native/
   vulkan_bridge.cpp) and its Rust state (vulkan_state.rs). The GPU handle records are
   generated in vulkan_handles.rs. The frame and presentation records use the existing
   packet bindings and layout assertions.

   Rust owns the shared device's readiness and its single creator, the bridge's permanent fault,
   the render events queued to Unity and their packets, the present interceptions and surface
   updates in progress, the player window frame the present renders under, the DXGI swap chain
   and its resize, the checked close and release, and the presentation: Unity's intercepted
   swapchains with their images and present-ready semaphores, the presentation window's identity
   and the presentation lease. The command slot rotation and the shared D3D12 fence values are the
   package bridge's submission ring (bridge_interop.h); a process has one Unity renderer, so the
   D3D11 and Vulkan bridges never use it at once. Rust owns the shared graphics device record,
   Vulkan dispatch, command pool, per-slot handles and imported images. vulkan_platform.rs
   executes the graphics operations. Rust also owns the HWND path and the image descriptors
   copied into each admitted event. vulkan_bridge.cpp calls the Unity SDK and guards foreign
   callbacks. */
#include "bridge_state.h"

/* Render event kinds, by offset from the bridge's first event ID. */
#define RSF_UNITY_VULKAN_EVENT_SR 0u
#define RSF_UNITY_VULKAN_EVENT_WINDOW 1u
#define RSF_UNITY_VULKAN_EVENT_HUDLESS 2u
/* rsf_unity_vulkan_initialize results. */
#define RSF_UNITY_VULKAN_DEVICE_BUSY 0u
#define RSF_UNITY_VULKAN_DEVICE_FAILED 1u
#define RSF_UNITY_VULKAN_DEVICE_READY 2u

#ifdef __cplusplus
extern "C" {
#endif
/* The frame and its process-lived host callbacks are copied together at admission. */
typedef struct rsf_unity_vulkan_frame {
    rsf_unity_packet packet;
    rsf_game_host_services services;
} rsf_unity_vulkan_frame;
/* Stores the player window frame. Zero when the bridge faulted or is closing. */
uint32_t rsf_unity_vulkan_set_window(const rsf_unity_vulkan_frame* frame, uint64_t instance, void* device, uint64_t queue, void* parent);

/* Copies frame into a free event slot of this kind. Returns the copy's address, which Unity hands
   back to the queue callback as identity only, and its slot; NULL when
   faulted, closing, of an unknown kind or full. */
void* rsf_unity_vulkan_admit_event(const rsf_unity_vulkan_frame* frame, uint32_t kind, uint32_t reconfigure,
    uint32_t* slot);
/* Copies out the queued event at address and marks it running; it stays outstanding until
   rsf_unity_vulkan_finish_event. Zero when address names no queued event; a faulted bridge frees
   the event and returns zero. */
uint32_t rsf_unity_vulkan_start_event(const void* address, rsf_unity_vulkan_frame* frame, uint32_t* kind,
    uint32_t* reconfigure, uint32_t* slot);
void rsf_unity_vulkan_finish_event(uint32_t slot);

/* A present interception or surface update in progress. Zero, and not counted, while closing. */
uint32_t rsf_unity_vulkan_enter(void);
void rsf_unity_vulkan_leave(void);

/* Transferred COM references and imported semaphore, plus borrowed Unity Vulkan device facts. */
typedef struct rsf_unity_vulkan_device_handles {
    void* device;
    void* queue;
    void* fence;
    uint64_t vk_device, vk_physical_device, vk_queue, vk_semaphore;
    uint32_t vk_queue_family;
    void* dispatch; /* Owned by the shared device, borrowed with this record. */
} rsf_unity_vulkan_device_handles;

/* A view borrowed under the presentation lease. The pool owns both VkCommandBuffers. */
typedef struct rsf_unity_vulkan_command_handles {
    void* allocator;
    void* commands;
    uint64_t vk[2];
    uint64_t complete;
} rsf_unity_vulkan_command_handles;
bool rsf_unity_vulkan_command_slot(uint32_t index, rsf_unity_vulkan_command_handles* view);

/* Rust owns these handles. A successful prepare lends a view under the caller's presentation
   lease. Replacing an image first drains all its GPU users. */
typedef struct rsf_unity_vulkan_image_handles {
    void* resource;
    uint64_t image, memory;
} rsf_unity_vulkan_image_handles;
/* Every create leaf transfers successful handles as it goes, including partial results on
   failure. Rust copies this table and destroys those handles using the same device view. */
typedef struct rsf_unity_vulkan_device_ops {
    bool (*create)(const rsf_game_host_services*, rsf_unity_vulkan_device_handles*);
    void (*destroy)(const rsf_unity_vulkan_device_handles*);
    uint64_t (*create_pool)(const rsf_unity_vulkan_device_handles*);
    void (*destroy_pool)(const rsf_unity_vulkan_device_handles*, uint64_t pool);
    bool (*create_slot)(const rsf_unity_vulkan_device_handles*, uint64_t pool, rsf_unity_vulkan_command_handles*);
    void (*destroy_slot)(const rsf_unity_vulkan_device_handles*, const rsf_unity_vulkan_command_handles*);
    bool (*create_image)(const rsf_unity_vulkan_device_handles*, uint32_t width, uint32_t height,
        uint32_t format, uint32_t dx_format, rsf_unity_vulkan_image_handles*);
    void (*destroy_image)(const rsf_unity_vulkan_device_handles*, const rsf_unity_vulkan_image_handles*);
    bool (*fence_ready)(const rsf_unity_vulkan_device_handles*, uint64_t fence);
    uint64_t (*shared_completed)(const rsf_unity_vulkan_device_handles*);
    bool (*drain_images)(const rsf_unity_vulkan_device_handles*);
    void (*report_created)(const rsf_game_host_services*);
    uint64_t (*ring_begin)(void);
    void (*ring_retired)(uint64_t epoch);
} rsf_unity_vulkan_device_ops;
/* The caller holds an entry or running event. READY lends a view under that hold. Creation is
   exclusive. A failed creation rolls back before making a new claim possible. */
uint32_t rsf_unity_vulkan_initialize(const rsf_game_host_services*, const rsf_unity_vulkan_device_ops*,
    rsf_unity_vulkan_device_handles* view);
/* Borrows an already published view under an entry. The caller must retain admission through use
   or take its own COM references. */
bool rsf_unity_vulkan_device_view(rsf_unity_vulkan_device_handles* view);
bool rsf_unity_vulkan_prepare_image(uint32_t index, uint32_t width, uint32_t height,
    uint32_t format, uint32_t dx_format, rsf_unity_vulkan_image_handles* view);
/* Stops the bridge until release. Non-zero for the first fault, which the caller reports. */
uint32_t rsf_unity_vulkan_fault(void);
uint32_t rsf_unity_vulkan_faulted(void);

/* Serialized by the package lifecycle. Stop new entries while already queued events and entries
   finish. Once idle, try_close keeps admission closed and polls both fence sets without waiting.
   A successful close holds the presentation lease until finish_retirement. A retry keeps the
   bridge closed, without holding the lease between attempts. Commit destroys images, commands,
   the DXGI chain and shared graphics, in that order. Only the next package open resumes entries. */
void rsf_unity_vulkan_stop_admission(void);
bool rsf_unity_vulkan_try_close(void);
void rsf_unity_vulkan_finish_retirement(bool commit);
void rsf_unity_vulkan_open(void);

/* Create transfers one IDXGISwapChain3 reference on success. The other DXGI leaves borrow it
   until destroy releases it. Drain uses the published shared device and the submission ring. */
typedef struct rsf_unity_vulkan_surface_ops {
    void* (*create)(const rsf_unity_vulkan_device_handles*, void* window, uint32_t width, uint32_t height);
    void (*extent)(void* chain, uint32_t* width, uint32_t* height);
    bool (*drain)(const rsf_unity_vulkan_device_handles*);
    bool (*resize)(void* chain, uint32_t width, uint32_t height);
    void (*destroy)(void* chain);
} rsf_unity_vulkan_surface_ops;
/* Borrows the owned chain after successful creation or resize (or when its extent already
   matches). Keep both admission and the presentation lease through all uses. Null leaves the
   existing owner, if any, in place. */
void* rsf_unity_vulkan_configure_surface(uint32_t width, uint32_t height, const rsf_unity_vulkan_surface_ops*);
uint32_t rsf_unity_vulkan_surface_ready(void);

/* Presentation: Unity's intercepted swapchains, the presentation window and the presentation lease
   outlive release. Vulkan handles are passed as uint64_t identity. The lease is exclusive: a
   present interception holds it from lookup through the forwarded vkQueuePresentKHR, swapchain
   retirement through vkDeviceWaitIdle and semaphore destruction, and the window thread while it
   creates the DXGI swap chain. */

/* Keeps the first presentation window. Non-zero when window is the kept one. */
uint32_t rsf_unity_vulkan_set_surface_window(void* window);
void* rsf_unity_vulkan_surface_window(void);

typedef struct rsf_unity_vulkan_present_source {
    uint64_t image;         /* VkImage at the presented index */
    uint64_t present_ready; /* VkSemaphore the forwarded present waits on */
    uint32_t width;
    uint32_t height;
} rsf_unity_vulkan_present_source;
/* Waits for and takes the lease, which the caller always ends with
   rsf_unity_vulkan_presentation_end. Returns 1 and consumes the matching receipt, 2 for the
   native player target without a receipt, or 0 for another target. */
uint32_t rsf_unity_vulkan_present_begin(uint64_t instance, void* device, uint64_t queue, uint64_t chain,
    uint32_t index, void* parent, rsf_unity_vulkan_present_source* source, rsf_unity_vulkan_frame* frame);
/* Takes the lease without waiting for a surface update or queue callback. */
uint32_t rsf_unity_vulkan_presentation_try_begin(void);
void rsf_unity_vulkan_presentation_end(void);
#ifdef __cplusplus
}
#endif
