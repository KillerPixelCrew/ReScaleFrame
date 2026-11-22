// SPDX-License-Identifier: MIT
#pragma once
#include <stddef.h>
#include <stdint.h>

/* Streamline 2.14.1 value records. bridge.cpp checks these against the pinned
   C++ declarations and copies into constructed objects at the SDK call. */
typedef struct rsf_slvk_sdk_header {
    void* next;
    uint32_t type_lo;
    uint16_t type_mid, type_hi;
    uint8_t type_tail[8];
    size_t version;
} rsf_slvk_sdk_header;

typedef void (*rsf_slvk_sdk_log)(uint32_t, const char*);

typedef struct rsf_slvk_sdk_preferences {
    rsf_slvk_sdk_header base;
    uint8_t show_console;
    uint32_t log_level;
    const uint16_t** plugin_paths;
    uint32_t num_plugin_paths;
    const uint16_t* logs_path;
    void (*allocate)();
    void (*release)();
    rsf_slvk_sdk_log log;
    uint64_t flags;
    const uint32_t* features;
    uint32_t num_features, application_id, engine;
    const char* engine_version;
    const char* project;
    uint32_t render_api;
} rsf_slvk_sdk_preferences;

typedef struct rsf_slvk_sdk_constants {
    rsf_slvk_sdk_header base;
    float view_to_clip[16], clip_to_view[16], clip_to_lens[16];
    float clip_to_previous[16], previous_to_clip[16];
    float jitter[2], motion_scale[2], pinhole[2];
    float position[3], up[3], right[3], forward[3];
    float near_plane, far_plane, fov, aspect, invalid_motion;
    uint8_t depth_inverted, motion_includes_camera, motion_3d, reset;
    uint8_t orthographic, motion_dilated, motion_jittered;
    float min_relative_depth_separation;
} rsf_slvk_sdk_constants;

typedef struct rsf_slvk_legacy_camera {
    const float* view_to_clip;
    const float* clip_to_view;
    const float* clip_to_previous;
    const float* previous_to_clip;
    float near_plane, far_plane, fov, aspect;
    float jitter[2], motion_scale[2];
    float position[3], up[3], right[3], forward[3];
    uint32_t reset;
} rsf_slvk_legacy_camera;

typedef struct rsf_slvk_legacy_image {
    uint64_t image, view;
    uint32_t layout, format, width, height, usage;
} rsf_slvk_legacy_image;

typedef struct rsf_slvk_sdk_resource {
    rsf_slvk_sdk_header base;
    uint8_t type;
    void* image;
    void* memory;
    void* view;
    uint32_t layout, width, height, format, mips, layers;
    uint64_t address;
    uint32_t flags, usage;
    uint16_t internal_flags, reserved;
} rsf_slvk_sdk_resource;

typedef struct rsf_slvk_sdk_tag {
    rsf_slvk_sdk_header base;
    void* resource;
    uint32_t type, lifecycle;
    uint32_t top, left, width, height;
} rsf_slvk_sdk_tag;

typedef struct rsf_slvk_sdk_tag_batch {
    rsf_slvk_sdk_resource resources[4];
    rsf_slvk_sdk_tag tags[4];
} rsf_slvk_sdk_tag_batch;

#ifdef __cplusplus
extern "C" {
#endif
uint32_t rsf_slvk_pack_legacy_camera(const rsf_slvk_legacy_camera*, rsf_slvk_sdk_constants*);
void rsf_slvk_pack_legacy_tags(const rsf_slvk_legacy_image* const*, rsf_slvk_sdk_tag_batch*);
#ifdef __cplusplus
}
#endif
