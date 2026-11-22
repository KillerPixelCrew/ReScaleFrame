// SPDX-License-Identifier: MIT
#pragma once
#include "api.h"
#include <stddef.h>
#include <stdint.h>

// Private engine module host lifecycle in the SCons Rust object (host.rs): boot admission and
// order, the ready, enabled, session and requested SR state, the configure sequence, shutdown
// order and the status-driven viewport extent. These callbacks are the Godot, module loader
// and driver leaves; Rust decides when each runs.

struct rsf_godot_boot_facts {
    uint32_t enabled_setting; // rescaleframe/enabled
    uint32_t headless;        // the null display driver
    uint32_t editor;
    uint32_t vulkan;          // the OS rendering driver is vulkan
};

struct rsf_godot_boot_callbacks {
    uint32_t (*load)();                   // settings, LoadLibraryW, required exports; zero on failure
    void (*capabilities)(uint32_t flags); // rsfg_vk_host_capabilities
    uint32_t (*start)(uint32_t vulkan);   // rsfg_boot_vulkan or rsfg_boot
    void (*rejected)();
    uint32_t (*attach)();                 // rsfg_attach_engine with the engine services
    uint64_t (*session)();
    uint32_t (*native_shutdown)();                // rsfg_status session identity
};

struct rsf_godot_configure_facts {
    uint32_t viewport_valid;
    uint32_t vulkan; // the rendering server driver is vulkan
};

struct rsf_godot_configure_callbacks {
    void *context;
    uint32_t (*is_root)(void *context);
    const rsf_godot_settings *(*parse)(void *context); // Dictionary into caller-owned settings
    void (*report_window)(void *context);              // rsfg_vk_window
    uint32_t (*configure)(void *context, const rsf_godot_settings *settings);
    void (*queue_primary)(void *context);              // rsf_godot_primary_set on the server thread
    void (*apply_scaling)(void *context, uint32_t mode, float scale); // viewport.h mode
};

struct rsf_godot_shutdown_callbacks {
    void (*release_render)(); // release_motion on the server thread, then sync
    uint32_t (*native_shutdown)();
};

struct rsf_godot_extent {
    uint32_t requested_sr, render_width, render_height, output_width, output_height;
};

struct rsf_godot_render_callbacks {
    void (*render_begin)(uint64_t source_frame_id); // rsfg_render_begin
    uint32_t (*status_extent)(rsf_godot_extent *out); // rsfg_status, zero on refusal
    void (*set_scale)(uint64_t viewport, double scale);
};

extern "C" uint32_t rsf_godot_host_ready();   // any thread
extern "C" uint32_t rsf_godot_host_active();  // ready and enabled, any thread
extern "C" uint64_t rsf_godot_host_session(); // any thread
extern "C" void rsf_godot_host_boot(const rsf_godot_boot_facts *facts, const rsf_godot_boot_callbacks *callbacks);
extern "C" uint32_t rsf_godot_host_configure(const rsf_godot_configure_facts *facts, const rsf_godot_configure_callbacks *callbacks);
extern "C" void rsf_godot_host_shutdown(const rsf_godot_shutdown_callbacks *callbacks);
// Main thread, at the engine's scene module initialization level.
extern "C" void rsf_godot_module_initialize(void (*register_class)(), void (*publish_bridge)());
extern "C" void rsf_godot_module_uninitialize(void (*shutdown)(), void (*unpublish_bridge)());
// Rendering server thread.
extern "C" void rsf_godot_host_render_begin(uint64_t source_frame_id, uint32_t gameplay, double seconds,
    const rsf_godot_render_callbacks *callbacks);
extern "C" void rsf_godot_host_render_size(uint64_t target, int32_t output_width, int32_t output_height,
    int32_t *internal, uint32_t (*status_extent)(rsf_godot_extent *out));

static_assert(RSF_GODOT_VK_GRAPH_HANDOFF == 1, "host.rs mirrors the graph handoff capability");
static_assert(offsetof(rsf_godot_settings, sr_backend) == 8 && offsetof(rsf_godot_settings, quality) == 12,
    "host.rs reads the settings prefix");
static_assert(sizeof(rsf_godot_boot_facts) == 16 && alignof(rsf_godot_boot_facts) == 4,
    "host.rs mirrors the boot facts");
static_assert(sizeof(rsf_godot_boot_callbacks) == 7 * sizeof(void *), "host.rs mirrors the boot callbacks");
static_assert(sizeof(rsf_godot_configure_facts) == 8 && alignof(rsf_godot_configure_facts) == 4,
    "host.rs mirrors the configure facts");
static_assert(sizeof(rsf_godot_configure_callbacks) == 7 * sizeof(void *), "host.rs mirrors the configure callbacks");
static_assert(sizeof(rsf_godot_shutdown_callbacks) == 2 * sizeof(void *), "host.rs mirrors the shutdown callbacks");
static_assert(sizeof(rsf_godot_extent) == 20 && alignof(rsf_godot_extent) == 4 &&
    offsetof(rsf_godot_extent, render_width) == 4 && offsetof(rsf_godot_extent, output_height) == 16,
    "host.rs mirrors the extent");
static_assert(sizeof(rsf_godot_render_callbacks) == 3 * sizeof(void *), "host.rs mirrors the render callbacks");
