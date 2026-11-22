// SPDX-License-Identifier: MIT
#pragma once
#include <rescaleframe/native_sr_vulkan.h>
#include <rescaleframe/capture_names.h>
#include <windows.h>
#include <vulkan/vulkan.h>
#include <algorithm>
#include <cstring>
#include <cwchar>
#include <filesystem>
#include <fstream>
#include <iomanip>
#include <memory>
#include <vector>
#include <mutex>

// Rust (runtime/orchestrator/src/frame_capture_vk.rs) owns selection, capture
// records, Vulkan dispatch, completion proof, readback lifetime and publishing.
static_assert(sizeof(VkPhysicalDeviceMemoryProperties) == 520);
extern "C" {
struct rsf_core_capture_vk;
rsf_core_capture_vk* rsf_core_capture_vk_create(uint32_t command_completion);
int32_t rsf_core_capture_vk_initialize(rsf_core_capture_vk*, void*, rsf_vk_get_proc,
    const VkPhysicalDeviceMemoryProperties*);
int32_t rsf_core_capture_vk_ready(const rsf_core_capture_vk*);
void rsf_core_capture_vk_destroy(rsf_core_capture_vk*);
int32_t rsf_core_capture_vk_enabled(const rsf_core_capture_vk*);
uint64_t rsf_core_capture_vk_pending(const rsf_core_capture_vk*);
void rsf_core_capture_vk_provider(const rsf_core_capture_vk*, uint64_t version, uint32_t backend, uint32_t generation);
int32_t rsf_core_capture_vk_wants(const rsf_core_capture_vk*, uint64_t frame);
int32_t rsf_core_capture_vk_record(const rsf_core_capture_vk*, void* commands, const rsf_srvk_image*,
    const rsf_frame_record*, const char* role, uint32_t reset, uint32_t core_source, uint64_t* ticket, uint64_t group);
void rsf_core_capture_vk_complete_group(const rsf_core_capture_vk*, uint64_t group);
void rsf_core_capture_vk_discard_group(const rsf_core_capture_vk*, uint64_t group);
void rsf_core_capture_vk_discard_all(const rsf_core_capture_vk*);
void rsf_core_capture_vk_retire(const rsf_core_capture_vk*, void* commands);
int32_t rsf_core_capture_vk_flush(const rsf_core_capture_vk*, uint32_t ready_only, void* commands);
}

namespace rsf {
// Serial recording only. The owner drains all referencing GPU submissions before
// flush/destruction. Buffers outlive command recording, submission and execution.
class FrameCaptureVk {
    rsf_core_capture_vk* owner;
public:
    enum class Completion { shared_submission, command_retirement };
    FrameCaptureVk() noexcept : FrameCaptureVk(Completion::shared_submission) {}
    explicit FrameCaptureVk(Completion completion) noexcept
        : owner(rsf_core_capture_vk_create(completion == Completion::command_retirement)) {}
    ~FrameCaptureVk() { rsf_core_capture_vk_destroy(owner); }
    FrameCaptureVk(const FrameCaptureVk&) = delete;
    FrameCaptureVk& operator=(const FrameCaptureVk&) = delete;
    bool enabled() const { return rsf_core_capture_vk_enabled(owner) != 0; }
    bool ready() const { return rsf_core_capture_vk_ready(owner) != 0; }
    size_t pending_count() { return size_t(rsf_core_capture_vk_pending(owner)); }
    void set_provider(uint64_t version, uint32_t selected, bool fg = false) { rsf_core_capture_vk_provider(owner, version, selected, fg); }
    // Group-based owners can publish after their own fence is certified.
    bool wants(uint64_t frame) const { return rsf_core_capture_vk_wants(owner, frame) != 0; }
    bool initialize(VkDevice value, rsf_vk_get_proc proc, const VkPhysicalDeviceMemoryProperties& properties) {
        return rsf_core_capture_vk_initialize(owner, value, proc, &properties) != 0;
    }
    bool record(VkCommandBuffer commands, const rsf_srvk_image& source, const rsf_frame_record& identity,
        const char* role, uint32_t reset, bool core_source = true, uint64_t* core_ticket = nullptr,
        uint64_t submission_group = 0) noexcept {
        uint64_t ticket = 0;
        const bool recorded = rsf_core_capture_vk_record(owner, commands, &source, &identity, role, reset,
            core_source, &ticket, submission_group) != 0;
        if (core_ticket) *core_ticket = ticket;
        return recorded;
    }
    // An owner can attach exact queue-completion evidence to its recording
    // group. Keep discarded allocations alive until the owner's full drain.
    void complete_group(uint64_t group) { rsf_core_capture_vk_complete_group(owner, group); }
    void discard_group(uint64_t group) { rsf_core_capture_vk_discard_group(owner, group); }
    // Only match a retired recording when its caller has proven completion.
    // A full flush requires all recorded work to have drained.
    void retire_commands(VkCommandBuffer commands) noexcept { rsf_core_capture_vk_retire(owner, commands); }
    // Requires device loss or retirement of all referencing submissions.
    void discard_all() noexcept { rsf_core_capture_vk_discard_all(owner); }
    bool flush(bool ready_only = false, VkCommandBuffer completed_commands = VK_NULL_HANDLE) noexcept {
        return rsf_core_capture_vk_flush(owner, ready_only, completed_commands) != 0;
    }
};
}
