// SPDX-License-Identifier: GPL-3.0-only
#define VK_NO_PROTOTYPES
#include <vulkan/vulkan.h>
#include "../runtime/vulkan/streamline/sdk-fixes/ngx-vulkan-dispatch.h"
#include <cstdio>
#include <cstdlib>

namespace {
uint32_t sequence = 0;
void require(bool value, const char* message) {
    if (!value) { std::fprintf(stderr, "%s\n", message); std::exit(1); }
}
void VKAPI_CALL dependency(VkCommandBuffer command, VkPipelineStageFlags source, VkPipelineStageFlags destination,
    VkDependencyFlags flags, uint32_t memory_count, const VkMemoryBarrier* memory,
    uint32_t buffer_count, const VkBufferMemoryBarrier*, uint32_t image_count, const VkImageMemoryBarrier*) {
    require(command == reinterpret_cast<VkCommandBuffer>(1) && !flags && memory_count == 1 && !buffer_count && !image_count,
        "fill synchronization is a bounded global memory dependency on the original command buffer");
    const bool before = sequence % 3 == 0;
    require(before || sequence % 3 == 2, "dependencies surround each actual fill exactly once");
    const VkAccessFlags all_memory = VK_ACCESS_MEMORY_READ_BIT | VK_ACCESS_MEMORY_WRITE_BIT;
    const VkAccessFlags transfer = VK_ACCESS_TRANSFER_READ_BIT | VK_ACCESS_TRANSFER_WRITE_BIT;
    require(memory->srcAccessMask == (before ? all_memory : transfer) &&
        memory->dstAccessMask == (before ? transfer : all_memory), "both transfer writes and prior writes are synchronized");
    const VkPipelineStageFlags all_commands = VK_PIPELINE_STAGE_ALL_COMMANDS_BIT;
    const VkPipelineStageFlags transfer_stage = VK_PIPELINE_STAGE_TRANSFER_BIT;
    require(source == (before ? all_commands : transfer_stage) &&
        destination == (before ? transfer_stage : all_commands), "fill stages cover transfer and all command consumers");
    ++sequence;
}
void VKAPI_CALL fill(VkCommandBuffer command, VkBuffer buffer, VkDeviceSize offset, VkDeviceSize size, uint32_t data) {
    require(sequence % 3 == 1 && command == reinterpret_cast<VkCommandBuffer>(1) && buffer == reinterpret_cast<VkBuffer>(2) &&
        offset == 16 && size == 1024 && data == 0x12345678, "fill preserves its entire original command");
    ++sequence;
}
PFN_vkVoidFunction VKAPI_CALL resolve(VkDevice, const char* name) {
    if (!std::strcmp(name, "vkCmdFillBuffer")) return reinterpret_cast<PFN_vkVoidFunction>(fill);
    if (!std::strcmp(name, "vkCmdPipelineBarrier")) return reinterpret_cast<PFN_vkVoidFunction>(dependency);
    return nullptr;
}
}
int main() {
    using namespace sl::chi::ngxDispatch;
    auto owner = reinterpret_cast<VkDevice>(3);
    configure(owner, nullptr, resolve);
    const auto wrapped_fill = reinterpret_cast<PFN_vkCmdFillBuffer>(GetDeviceProcAddr(owner, "vkCmdFillBuffer"));
    require(wrapped_fill == FillBuffer, "NGX dispatch actually selects the corrected fill entry");
    require(GetDeviceProcAddr(reinterpret_cast<VkDevice>(4), "vkCmdFillBuffer") == reinterpret_cast<PFN_vkVoidFunction>(fill),
        "foreign device dispatch remains its original entry");
    for (uint32_t i = 0; i < 3; ++i) wrapped_fill(reinterpret_cast<VkCommandBuffer>(1), reinterpret_cast<VkBuffer>(2), 16, 1024, 0x12345678);
    require(sequence == 9, "successive MFG scratch fills each receive both dependencies");
    std::puts("Streamline NGX fill-buffer dependency checks passed");
}
