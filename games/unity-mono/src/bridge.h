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

// Returns a leased packet to Rust after all callbacks using the packet have finished.
struct rsf_unity_taken_packet {
    rsf_unity_packet packet{};
    uint32_t slot = 0;
    const bool taken;
    explicit rsf_unity_taken_packet(const void* address)
        : taken(rsf_unity_frames_take_packet(address, &packet, &slot) != 0) {}
    rsf_unity_taken_packet(const rsf_unity_taken_packet&) = delete;
    rsf_unity_taken_packet& operator=(const rsf_unity_taken_packet&) = delete;
    ~rsf_unity_taken_packet() { if (taken) rsf_unity_frames_end_packet(slot); }
};

bool rsf_unity_bridge_prepare(const rsf_game_host_services& host, rsf_unity_native_api* api) noexcept;
void rsf_unity_bridge_activate(bool enabled) noexcept;
void rsf_unity_bridge_release() noexcept;
uint32_t rsf_unity_bridge_managed_state() noexcept;
