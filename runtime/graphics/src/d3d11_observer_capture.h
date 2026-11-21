/* SPDX-License-Identifier: MIT */
#pragma once
#include <rescaleframe/d3d11_observer.h>
#include <rescaleframe/motion_decode.h>

typedef void* (*rsf_observer11_immediate)(void* device);

struct rsf_observer11_dump_ops {
    rsf_observer11_immediate immediate;
    int32_t (*texture)(void* device, void* context, void* texture, const rsf_texture_dump_options*);
    int32_t (*read_buffer)(void* device, void* context, void* buffer, void* destination, uint32_t bytes);
    int32_t (*create_decode)(void* device, void* source, rsf_dump_log_fn, void* log_user, rsf_motion_decode**);
    int32_t (*decode)(rsf_motion_decode*, void* context, void* source, const rsf_motion_decode_params*);
    void* (*decoded_texture)(rsf_motion_decode*);
    void (*destroy_decode)(rsf_motion_decode*);
};

extern "C" {
int32_t rsf_core_observer11_needs_presenting_device(void);
/* Consumes the presenting device reference and transfers the replaced one, if any. */
void* rsf_core_observer11_adopt_device(void* device);
int32_t rsf_core_observer11_acquire_device(void** device, void** context, rsf_observer11_immediate);
int32_t rsf_core_observer11_status(rsf_observer_status*);
int32_t rsf_core_observer11_request_dump(const char* prefix, uint32_t view);
void rsf_core_observer11_dump(const rsf_observer11_dump_ops*);
int32_t rsf_core_observer11_write_buffer_sizes(const char* path);
}
