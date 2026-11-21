// SPDX-License-Identifier: MIT
#pragma once
#include <rescaleframe/unity_bridge.h>
#include "bridge_state.h"

struct rsf_unity_frame_callback {
    const bool entered = rsf_unity_frames_callback_enter() != 0;
    rsf_unity_frame_callback() = default;
    rsf_unity_frame_callback(const rsf_unity_frame_callback&) = delete;
    rsf_unity_frame_callback& operator=(const rsf_unity_frame_callback&) = delete;
    ~rsf_unity_frame_callback() { if (entered) rsf_unity_frames_callback_leave(); }
};

bool rsf_unity_bridge_prepare(const rsf_game_host_services& host, rsf_unity_native_api* api) noexcept;
void rsf_unity_bridge_activate(bool enabled) noexcept;
bool rsf_unity_bridge_drained() noexcept;
void rsf_unity_bridge_release() noexcept;
uint32_t rsf_unity_bridge_managed_state() noexcept;
