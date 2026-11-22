/* SPDX-License-Identifier: MIT */
#pragma once
#include <rescaleframe/d3d11_observer.h>

struct rsf_observer11_present_ops {
    int32_t (*describe)(void* chain, uint32_t* width, uint32_t* height);
    int32_t (*device)(void* chain, void** owned);
    int32_t (*forward)(const rsf_observer_present_event*, const void* parameters, void* target);
};
