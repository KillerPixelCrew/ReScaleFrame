// SPDX-License-Identifier: MIT
#pragma once

#include <cstddef>
#include <cstdint>
#include <type_traits>

// Private rsf-ac7 ABI. An inactive observation still carries the most recent session ID.
struct MotionCaptureInterval {
    uint32_t session, interval;
    explicit operator bool() const noexcept { return interval != UINT32_MAX; }
    bool operator==(const MotionCaptureInterval&) const = default;
};

struct MotionCaptureStamp {
    uint32_t year, month, day, hour, minute, second, process;
};
struct MotionArtifactStats {
    int64_t qpc_frequency;
    uint32_t engine_hooks, renderer_root_hooks, capture_failures;
};
using MotionWriteFile = bool(*)(const char* path, const unsigned char* data, std::size_t size);

struct MotionCaptureEffects {
    bool(*poll)(void* swapchain, bool* pressed);
    void(*stamp)(MotionCaptureStamp* stamp);
    bool(*create_directory)(const char* path);
    void(*set_sample)(bool enabled);
    MotionWriteFile write;
    MotionArtifactStats(*stats)();
};

extern "C" {
bool rsf_ac7_capture_enabled();
void rsf_ac7_capture_log(const char* message);
MotionCaptureInterval rsf_ac7_capture_interval();
MotionCaptureInterval rsf_ac7_capture_sample();
bool rsf_ac7_capture_sampled(uint32_t interval);
void rsf_ac7_capture_fail(MotionCaptureInterval observed);
uint32_t rsf_ac7_capture_failures();
bool rsf_ac7_capture_present(void* swapchain, char* prefix, uint32_t capacity,
    const MotionCaptureEffects* effects);
void rsf_ac7_capture_shutdown(const MotionCaptureEffects* effects);
}

static_assert(sizeof(MotionCaptureInterval) == 8 && offsetof(MotionCaptureInterval, interval) == 4);
static_assert(std::is_trivial_v<MotionCaptureInterval> && std::is_standard_layout_v<MotionCaptureInterval>);
static_assert(sizeof(MotionCaptureStamp) == 28 && offsetof(MotionCaptureStamp, process) == 24);
static_assert(sizeof(MotionArtifactStats) == 24 && offsetof(MotionArtifactStats, capture_failures) == 16);
static_assert(sizeof(MotionCaptureEffects) == 48 && offsetof(MotionCaptureEffects, stats) == 40);
