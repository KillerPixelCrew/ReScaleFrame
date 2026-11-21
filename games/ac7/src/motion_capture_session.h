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

struct MotionCaptureEffects {
    bool(*poll)(void* swapchain, bool* pressed);
    bool(*prepare)(uint32_t session);
    void(*armed)(MotionCaptureInterval interval);
    void(*set_sample)(uint32_t interval, char* prefix, uint32_t capacity);
    void(*finish)();
};

extern "C" {
void rsf_ac7_capture_enable(bool enabled);
bool rsf_ac7_capture_enabled();
void rsf_ac7_capture_request();
MotionCaptureInterval rsf_ac7_capture_interval();
MotionCaptureInterval rsf_ac7_capture_sample();
bool rsf_ac7_capture_sampled(uint32_t interval);
bool rsf_ac7_capture_current(uint32_t session);
void rsf_ac7_capture_fail(MotionCaptureInterval observed);
uint32_t rsf_ac7_capture_failures();
bool rsf_ac7_capture_due_checkpoint(MotionCaptureInterval observed);
bool rsf_ac7_capture_present(void* swapchain, char* prefix, uint32_t capacity,
    const MotionCaptureEffects* effects);
void rsf_ac7_capture_shutdown(const MotionCaptureEffects* effects);
}

static_assert(sizeof(MotionCaptureInterval) == 8 && offsetof(MotionCaptureInterval, interval) == 4);
static_assert(std::is_trivial_v<MotionCaptureInterval> && std::is_standard_layout_v<MotionCaptureInterval>);
static_assert(sizeof(MotionCaptureEffects) == 40 && offsetof(MotionCaptureEffects, finish) == 32);
