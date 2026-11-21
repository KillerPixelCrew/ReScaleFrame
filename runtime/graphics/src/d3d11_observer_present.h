/* SPDX-License-Identifier: MIT */
#pragma once
#include <rescaleframe/d3d11_observer.h>

struct rsf_observer11_present_ops {
    int32_t (*is_d3d11)(void* chain);
    void (*update)(void* chain);
    int32_t (*forward)(const rsf_observer_present_event*, const void* parameters, uint32_t nested);
};

extern "C" {
int32_t rsf_core_observer11_present(rsf_observer_present_event*, const void* parameters,
    const rsf_observer11_present_ops*);
int32_t rsf_core_observer11_application_present(const rsf_observer_present_event*, void (*update)(void*));
uint64_t rsf_core_observer11_present_size(void);
uint64_t rsf_core_observer11_replace_size(uint32_t width, uint32_t height);
}
