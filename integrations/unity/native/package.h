// SPDX-License-Identifier: GPL-3.0-only
#pragma once
#include <stdint.h>
#include <rescaleframe/unity_bridge.h>
#define RSF_UNITY_PACKAGE_ABI_VERSION 1u
typedef struct rsf_unity_package_settings {
    uint32_t struct_size, abi_version;
    uint32_t sr_backend, quality, dlss_preset;
    uint32_t fg_backend, fg_mode, generated_frames, latency_mode, frame_limit_us;
    uint32_t gameplay;
} rsf_unity_package_settings;
typedef struct rsf_unity_package_status {
    uint32_t struct_size, abi_version;
    uint32_t requested_sr, active_sr, render_width, render_height, output_width, output_height;
    int32_t sr_result, fg_result;
    uint32_t fg_backend, fg_available, fg_active, max_generated_frames, generated_frames;
    uint32_t latency_available, latency_mode, reason;
    uint64_t evaluated_frames, refused_frames, tagged_frames;
    uint64_t sleep_calls, marker_calls;
    uint32_t pcl_available, latency_report_available;
} rsf_unity_package_status;
void rsf_unity_package_bridge_error(int32_t result, const char* message) noexcept;
bool rsf_unity_package_graphics(void** device, void** queue) noexcept;
bool rsf_unity_package_is_editor() noexcept;
#define RSF_UNITY_PREVIEW_SOURCE_COMMON 0x40000000u
typedef struct rsf_unity_frame_counters {
    uint32_t struct_size, abi_version, valid, active;
    uint64_t total_presented, generated_presented, provider_generation;
} rsf_unity_frame_counters;
