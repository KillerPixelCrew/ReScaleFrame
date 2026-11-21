/* SPDX-License-Identifier: MIT */
#pragma once
#include <rescaleframe/d3d11_observer.h>
#include <stddef.h>

struct rsf_observer11_capture_filter {
    uint32_t format, minimum_width, capacity, constant_min, constant_max;
};

struct rsf_observer11_capture_status {
    uint32_t have_device, textures_matched, textures_created, constant_matches, distinct_buffer_sizes;
    uint32_t dumps_completed, textures_written, constant_bytes_written;
};

struct rsf_observer11_dump_buffer {
    uint32_t bytes;
    void* buffer;
};

/* All records and interfaces are borrowed for the synchronous dump call. */
struct rsf_observer11_dump {
    void* device;
    void* context;
    const char* prefix;
    void* const* textures;
    size_t texture_count;
    const rsf_observer11_dump_buffer* buffers;
    size_t buffer_count;
    uint32_t view;
};

struct rsf_observer11_dump_result { uint32_t textures, constant_bytes; };
typedef void* (*rsf_observer11_immediate)(void* device);
typedef void (*rsf_observer11_write_dump)(const rsf_observer11_dump*, rsf_observer11_dump_result*);

extern "C" {
void rsf_core_observer11_capture_configure(const rsf_observer11_capture_filter*);
void rsf_core_observer11_texture(void* device, void* texture, uint32_t width, uint32_t format, uint32_t bindings);
void rsf_core_observer11_buffer(void* device, void* buffer, uint32_t bytes, uint32_t bindings);
int32_t rsf_core_observer11_needs_presenting_device(void);
/* Consumes the presenting device reference and transfers the replaced one, if any. */
void* rsf_core_observer11_adopt_device(void* device);
int32_t rsf_core_observer11_acquire_device(void** device, void** context, rsf_observer11_immediate);
void rsf_core_observer11_capture_status(rsf_observer11_capture_status*);
void rsf_core_observer11_request_dump(const char* prefix, uint32_t view);
void rsf_core_observer11_dump(rsf_observer11_immediate, rsf_observer11_write_dump);
int32_t rsf_core_observer11_write_buffer_sizes(const char* path);
}
