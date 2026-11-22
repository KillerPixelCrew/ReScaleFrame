/* SPDX-License-Identifier: MIT */
#pragma once
#include <rescaleframe/d3d11_observer.h>

struct rsf_observer11_texture_facts {
    uint32_t width, height, format, mip_levels, array_size, sample_count, bind_flags, misc_flags;
};

/* Contain foreign C++ exceptions before returning to the Rust dispatcher. */
struct rsf_observer11_callbacks {
    void (*log)(rsf_dump_log_fn, void* user, const char*);
    void (*layout)(rsf_observer_layout_fn, void* user, void* object, const rsf_observer_layout_element*, uint32_t copied, uint32_t count);
    void (*shader)(rsf_observer_shader_fn, void* user, void* object, uint32_t stage, const void* bytes, uint32_t size);
    void (*texture)(rsf_observer_texture_fn, void* user, void* object, const rsf_observer11_texture_facts*);
    void (*buffer)(rsf_observer_buffer_fn, void* user, void* object, const void* bytes, uint32_t size, uint32_t flags);
    void (*present)(rsf_observer_present_fn, void* user, void* chain);
    void (*event)(rsf_observer_present_event_fn, void* user, const rsf_observer_present_event*);
};

extern "C" {
int32_t rsf_core_observer11_logging(void);
void rsf_core_observer11_log(const char*);
}
