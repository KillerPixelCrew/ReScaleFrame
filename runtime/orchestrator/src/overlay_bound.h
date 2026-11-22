// SPDX-License-Identifier: MIT
#pragma once
#include <rescaleframe/overlay_api.h>

// Embedded D3D12 consumers share the service's settings and panel. The caller
// applies the persisted settings at its drained boundary and owns the source queue.
rsf_result rsf_overlay_configure_bound(const rsf_overlay_service_setup*);
rsf_result rsf_overlay_present_bound(void* device, void* queue, void* chain, void* backbuffer, void* input_window,
    rsf_game_log_fn, void*);
rsf_result rsf_overlay_notify_bound(uint32_t reason);
