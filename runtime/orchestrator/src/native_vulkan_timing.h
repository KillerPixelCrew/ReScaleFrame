// SPDX-License-Identifier: MIT
#pragma once
#include <cstdint>

struct rsf_vk_timing;
extern "C" {
rsf_vk_timing* rsf_core_vk_timing_open();
void rsf_core_vk_timing_close(rsf_vk_timing*);
int64_t rsf_core_vk_timing_begin(const rsf_vk_timing*);
void rsf_core_vk_timing_end(const rsf_vk_timing*, const char* stage, uint64_t frame, int64_t start);
}

namespace rsf {
inline const rsf_vk_timing* vk_timing() noexcept {
    // Retire the Rust file owner when this DLL unloads.
    struct Owner {
        rsf_vk_timing* handle = rsf_core_vk_timing_open();
        ~Owner() { rsf_core_vk_timing_close(handle); }
    };
    static const Owner owner;
    return owner.handle;
}

inline int64_t vk_timing_begin() noexcept {
    return rsf_core_vk_timing_begin(vk_timing());
}

inline void vk_timing_end(const char* stage, uint64_t frame, int64_t start) noexcept {
    if (start) rsf_core_vk_timing_end(vk_timing(), stage, frame, start);
}

class VulkanTimingScope {
    const char* stage;
    uint64_t frame;
    int64_t start;
public:
    VulkanTimingScope(const char* value, uint64_t identity) noexcept
        : stage(value), frame(identity), start(value ? vk_timing_begin() : 0) {}
    ~VulkanTimingScope() { vk_timing_end(stage, frame, start); }
    VulkanTimingScope(const VulkanTimingScope&) = delete;
    VulkanTimingScope& operator=(const VulkanTimingScope&) = delete;
};

template<class Call> decltype(auto) vk_timed_call(const char* stage, uint64_t frame, Call&& call) {
    const VulkanTimingScope scope(stage, frame);
    return call();
}
}
