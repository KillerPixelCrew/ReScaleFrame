// SPDX-License-Identifier: GPL-3.0-only
#pragma once
#include <cstdint>
namespace rsf::latency {
struct AmdVulkanStatus {bool available{},enabled{};uint64_t input_updates{},present_updates{};};
bool amd_vk_capable();
void amd_vk_options(uint32_t mode,uint32_t limit_us);
void amd_vk_begin(uint64_t frame);
void amd_vk_input(uint64_t frame);
void amd_vk_present(uint64_t frame);
void amd_vk_cancel(uint64_t frame);
AmdVulkanStatus amd_vk_status();
void amd_vk_shutdown();
}
