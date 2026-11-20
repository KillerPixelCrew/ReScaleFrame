// SPDX-License-Identifier: MIT
#pragma once
#include <rescaleframe/native_sr_vulkan.h>
namespace rsf::latency {
struct AmdVulkanStatus {uint32_t available,enabled;uint64_t input_updates,present_updates;};
extern "C" {
rsf_backend_result rsf_core_amd_vk_bind(void*, rsf_vk_function,
    void (*invoke)(void*, rsf_vk_function, uint32_t, uint64_t, uint32_t, uint32_t));
void rsf_core_amd_vk_device_destroyed(void*);
AmdVulkanStatus rsf_core_amd_vk_status();
void rsf_core_amd_vk_options(uint32_t mode,uint32_t limit_us);
void rsf_core_amd_vk_begin(uint64_t session,uint64_t frame);
void rsf_core_amd_vk_cancel(uint64_t session,uint64_t frame);
void rsf_core_amd_vk_frame(uint64_t session,uint64_t frame,uint32_t stage);
rsf_backend_result rsf_core_amd_vk_shutdown_begin();
void rsf_core_amd_vk_shutdown_end(uint32_t committed);
}

class AmdVulkanShutdown {
    bool active_=rsf_core_amd_vk_shutdown_begin()==RSF_BACKEND_OK;
    bool committed_{};
public:
    AmdVulkanShutdown() = default;
    ~AmdVulkanShutdown() { if (active_) rsf_core_amd_vk_shutdown_end(committed_); }
    explicit operator bool() const { return active_; }
    void commit() { committed_=true; }
    AmdVulkanShutdown(const AmdVulkanShutdown&) = delete;
    AmdVulkanShutdown& operator=(const AmdVulkanShutdown&) = delete;
};

inline bool amd_vk_capable() { return rsf_core_amd_vk_status().available != 0; }
inline void amd_vk_input(uint64_t session,uint64_t frame) { rsf_core_amd_vk_frame(session,frame,0); }
inline void amd_vk_present(uint64_t session,uint64_t frame) { rsf_core_amd_vk_frame(session,frame,1); }
}
