// SPDX-License-Identifier: GPL-3.0-only
#include "drivers/vulkan/rendering_device_driver_vulkan.h"

void *RenderingDeviceDriverVulkan::rescaleframe_begin(CommandBufferID id) {
    auto *commands = reinterpret_cast<CommandBufferInfo *>(id.id);
    if (!commands || commands->active_render_pass) return nullptr;
    return commands->vk_command_buffer;
}
uint64_t RenderingDeviceDriverVulkan::rescaleframe_instance_proc() {
    return reinterpret_cast<uint64_t>(vkGetInstanceProcAddr);
}
uint64_t RenderingDeviceDriverVulkan::rescaleframe_device_proc() {
    return reinterpret_cast<uint64_t>(vkGetDeviceProcAddr);
}
