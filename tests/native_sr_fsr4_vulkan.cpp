// SPDX-License-Identifier: GPL-3.0-only
#include "vulkan_interop_fixture.h"
#include "../runtime/orchestrator/src/native_sr_fsr4_vulkan.h"
#include <cstdio>
#include <cwchar>
#include <string>

namespace {
struct Handoff {
    const rsf_fixture::Device* device{};
    VkCommandBuffer continuation{};
    VkSemaphore timeline{};
    uint64_t previous{}, ready{}, done{};
    uint32_t splits{};
};
int32_t split(void* user, void* commands, uint64_t timeline, uint64_t previous,
    uint64_t ready, uint64_t done, void** continuation) {
    auto& state = *static_cast<Handoff*>(user);
    int result = state.device->function<PFN_vkEndCommandBuffer>("vkEndCommandBuffer")(static_cast<VkCommandBuffer>(commands));
    VkCommandBufferBeginInfo begin{VK_STRUCTURE_TYPE_COMMAND_BUFFER_BEGIN_INFO}; begin.flags = VK_COMMAND_BUFFER_USAGE_ONE_TIME_SUBMIT_BIT;
    if (!result) result = state.device->function<PFN_vkBeginCommandBuffer>("vkBeginCommandBuffer")(state.continuation, &begin);
    if (result) return result;
    state.timeline = reinterpret_cast<VkSemaphore>(timeline); state.previous = previous;
    state.ready = ready; state.done = done; ++state.splits;
    *continuation = state.continuation;
    return 0; // Deliberately no queue submission until evaluate has returned.
}
void transition(const rsf_fixture::Device& device, VkCommandBuffer commands, VkImage image,
    VkImageLayout before, VkImageLayout after) {
    VkImageMemoryBarrier barrier{VK_STRUCTURE_TYPE_IMAGE_MEMORY_BARRIER};
    barrier.oldLayout = before; barrier.newLayout = after;
    barrier.srcAccessMask = before == VK_IMAGE_LAYOUT_UNDEFINED ? 0 : VK_ACCESS_MEMORY_READ_BIT | VK_ACCESS_MEMORY_WRITE_BIT;
    barrier.dstAccessMask = VK_ACCESS_MEMORY_READ_BIT | VK_ACCESS_MEMORY_WRITE_BIT;
    barrier.srcQueueFamilyIndex = barrier.dstQueueFamilyIndex = VK_QUEUE_FAMILY_IGNORED;
    barrier.image = image; barrier.subresourceRange = {VK_IMAGE_ASPECT_COLOR_BIT, 0, 1, 0, 1};
    device.function<PFN_vkCmdPipelineBarrier>("vkCmdPipelineBarrier")(commands,
        VK_PIPELINE_STAGE_ALL_COMMANDS_BIT, VK_PIPELINE_STAGE_ALL_COMMANDS_BIT, 0, 0, nullptr, 0, nullptr, 1, &barrier);
}
}

int wmain(int argc, wchar_t** argv) {
    rsf_fsr4_bridge* rejected = reinterpret_cast<rsf_fsr4_bridge*>(1);
    rsf_fsr4_bridge_create invalid{sizeof(invalid) - 1, 1};
    if (RsfFsr4BridgeCreate(&invalid, &rejected) != RSF_FSR4_BRIDGE_INVALID_ARGUMENT || rejected) return 1;
    invalid.struct_size = sizeof(invalid); invalid.abi_version = 2;
    if (RsfFsr4BridgeCreate(&invalid, &rejected) != RSF_FSR4_BRIDGE_ABI_MISMATCH || rejected) return 2;
    invalid.abi_version = 1; invalid.runtime_directory = reinterpret_cast<const uint16_t*>(L"relative");
    if (RsfFsr4BridgeCreate(&invalid, &rejected) != RSF_FSR4_BRIDGE_INVALID_ARGUMENT || rejected) return 3;
    if (argc == 1) { std::puts("{\"contract_checks\":\"passed\",\"device_test\":false}"); return 0; }
    if (argc != 3 || std::wcscmp(argv[1], L"--device")) return 4;
    rsf_fixture::Device device;
    int result = device.init();
    if (result) { std::printf("{\"device_create\":%d}\n", result); device.close(); return 5; }
    int length = WideCharToMultiByte(CP_UTF8, 0, argv[2], -1, nullptr, 0, nullptr, nullptr);
    std::string directory(static_cast<size_t>(length), '\0');
    WideCharToMultiByte(CP_UTF8, 0, argv[2], -1, directory.data(), length, nullptr, nullptr);
    auto graphics = device.setup(); graphics.backend = 4; graphics.quality = 1;
    graphics.output_width = 640; graphics.output_height = 360;
    graphics.depth_inverted = 1; graphics.depth_infinite = 1;
    graphics.runtime_directory_utf8 = directory.c_str();
    Handoff state{&device};
    rsf_fsr4vk_handoff handoff{sizeof(handoff), 1, 1, device.family, &state, split};
    rsf_fsr4vk* owner = nullptr;
    result = rsf_fsr4vk_create(&graphics, &handoff, &owner);
    const int create_result = result;
    uint32_t width = 0, height = 0;
    rsf_fsr4_bridge_info info{sizeof(info), 1};
    if (!result) result = rsf_fsr4vk_plan(owner, &width, &height);
    if (!result) result = rsf_fsr4vk_info(owner, &info);
    std::array<rsf_fixture::Image, 4> images{};
    const uint32_t formats[] = {VK_FORMAT_R16G16B16A16_SFLOAT, VK_FORMAT_R32_SFLOAT, VK_FORMAT_R16G16_SFLOAT, VK_FORMAT_R16G16B16A16_SFLOAT};
    for (uint32_t i = 0; i < images.size() && !result; ++i)
        result = images[i].init(device, formats[i], i == 3 ? 640 : width, i == 3 ? 360 : height);
    VkCommandPool pool{}; std::array<VkCommandBuffer, 2> commands{};
    if (!result) {
        VkCommandPoolCreateInfo create{VK_STRUCTURE_TYPE_COMMAND_POOL_CREATE_INFO};
        create.flags = VK_COMMAND_POOL_CREATE_RESET_COMMAND_BUFFER_BIT; create.queueFamilyIndex = device.family;
        result = device.function<PFN_vkCreateCommandPool>("vkCreateCommandPool")(device.device, &create, nullptr, &pool);
        VkCommandBufferAllocateInfo allocate{VK_STRUCTURE_TYPE_COMMAND_BUFFER_ALLOCATE_INFO};
        allocate.commandPool = pool; allocate.level = VK_COMMAND_BUFFER_LEVEL_PRIMARY; allocate.commandBufferCount = 2;
        if (!result) result = device.function<PFN_vkAllocateCommandBuffers>("vkAllocateCommandBuffers")(device.device, &allocate, commands.data());
        state.continuation = commands[1];
    }
    uint32_t completed = 0, duplicate_refused = 0;
    for (uint32_t frame = 1; frame <= 6 && !result; ++frame) {
        result = device.function<PFN_vkDeviceWaitIdle>("vkDeviceWaitIdle")(device.device);
        for (auto command : commands) if (!result) result = device.function<PFN_vkResetCommandBuffer>("vkResetCommandBuffer")(command, 0);
        VkCommandBufferBeginInfo begin{VK_STRUCTURE_TYPE_COMMAND_BUFFER_BEGIN_INFO}; begin.flags = VK_COMMAND_BUFFER_USAGE_ONE_TIME_SUBMIT_BIT;
        if (!result) result = device.function<PFN_vkBeginCommandBuffer>("vkBeginCommandBuffer")(commands[0], &begin);
        if (result) break;
        if (frame == 1) {
            for (uint32_t i = 0; i < images.size(); ++i) {
                transition(device, commands[0], images[i].image, VK_IMAGE_LAYOUT_UNDEFINED, VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL);
                if (i == 3) continue;
                VkClearColorValue clear{};
                if (i == 0) { clear.float32[0] = 0.2f; clear.float32[1] = 0.4f; clear.float32[2] = 0.8f; clear.float32[3] = 1; }
                if (i == 1) clear.float32[0] = 0.75f;
                VkImageSubresourceRange range{VK_IMAGE_ASPECT_COLOR_BIT, 0, 1, 0, 1};
                device.function<PFN_vkCmdClearColorImage>("vkCmdClearColorImage")(commands[0], images[i].image,
                    VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL, &clear, 1, &range);
                transition(device, commands[0], images[i].image, VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL, VK_IMAGE_LAYOUT_SHADER_READ_ONLY_OPTIMAL);
            }
        }
        rsf_frame_record record{}; record.struct_size = sizeof(record); record.abi_version = RSF_GAME_FRAME_ABI_VERSION;
        record.frame_id = frame; record.session_id = 1; record.view_id = 1; record.resource_generation = 1;
        record.render_width = width; record.render_height = height; record.output_width = 640; record.output_height = 360;
        record.frame_time_ms = 16.666667f; record.flags = frame == 1 ? RSF_FRAME_FLAG_RESET : 0;
        record.camera.struct_size = sizeof(record.camera); record.camera.abi_version = RSF_GAME_FRAME_ABI_VERSION;
        record.camera.render_width = width; record.camera.render_height = height;
        record.camera.output_width = 640; record.camera.output_height = 360;
        record.camera.near_plane = 0.1f; record.camera.far_plane = 0; record.camera.vertical_fov_radians = 1;
        record.camera.depth_inverted = 1;
        rsf_srvk_frame input{}; input.struct_size = sizeof(input); input.abi_version = RSF_SRVK_ABI_VERSION; input.record = &record;
        input.color = images[0].input(device, VK_IMAGE_LAYOUT_SHADER_READ_ONLY_OPTIMAL);
        input.depth = images[1].input(device, VK_IMAGE_LAYOUT_SHADER_READ_ONLY_OPTIMAL);
        input.motion = images[2].input(device, VK_IMAGE_LAYOUT_SHADER_READ_ONLY_OPTIMAL);
        input.output = images[3].input(device, VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL);
        input.motion_scale_x = static_cast<float>(width); input.motion_scale_y = static_cast<float>(height); input.pre_exposure = 1;
        void* continuation = nullptr;
        const int provider_result = rsf_fsr4vk_evaluate(owner, commands[0], &input, &continuation);
        if (!continuation || continuation == commands[0]) { result = provider_result ? provider_result : -1; break; }
        void* duplicate = nullptr;
        if (rsf_fsr4vk_evaluate(owner, commands[0], &input, &duplicate) == RSF_FSR4_BRIDGE_INVALID_ARGUMENT) ++duplicate_refused;
        if (provider_result) {
            VkClearColorValue fallback{}; fallback.float32[3] = 1;
            VkImageSubresourceRange range{VK_IMAGE_ASPECT_COLOR_BIT, 0, 1, 0, 1};
            device.function<PFN_vkCmdClearColorImage>("vkCmdClearColorImage")(static_cast<VkCommandBuffer>(continuation),
                images[3].image, VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL, &fallback, 1, &range);
        }
        result = device.function<PFN_vkEndCommandBuffer>("vkEndCommandBuffer")(static_cast<VkCommandBuffer>(continuation));
        VkPipelineStageFlags stage = VK_PIPELINE_STAGE_ALL_COMMANDS_BIT;
        std::array<VkSubmitInfo, 2> submits{};
        std::array<VkTimelineSemaphoreSubmitInfo, 2> timelines{};
        for (uint32_t i = 0; i < 2; ++i) { submits[i].sType = VK_STRUCTURE_TYPE_SUBMIT_INFO;
            timelines[i].sType = VK_STRUCTURE_TYPE_TIMELINE_SEMAPHORE_SUBMIT_INFO;
            submits[i].pNext = &timelines[i]; submits[i].commandBufferCount = 1; submits[i].pCommandBuffers = &commands[i]; }
        submits[0].waitSemaphoreCount = state.previous ? 1 : 0; submits[0].pWaitSemaphores = &state.timeline; submits[0].pWaitDstStageMask = &stage;
        timelines[0].waitSemaphoreValueCount = submits[0].waitSemaphoreCount; timelines[0].pWaitSemaphoreValues = &state.previous;
        submits[0].signalSemaphoreCount = 1; submits[0].pSignalSemaphores = &state.timeline;
        timelines[0].signalSemaphoreValueCount = 1; timelines[0].pSignalSemaphoreValues = &state.ready;
        submits[1].waitSemaphoreCount = 1; submits[1].pWaitSemaphores = &state.timeline; submits[1].pWaitDstStageMask = &stage;
        timelines[1].waitSemaphoreValueCount = 1; timelines[1].pWaitSemaphoreValues = &state.done;
        if (!result) result = device.function<PFN_vkQueueSubmit>("vkQueueSubmit")(device.queue, 2, submits.data(), VK_NULL_HANDLE);
        if (!result) result = device.function<PFN_vkDeviceWaitIdle>("vkDeviceWaitIdle")(device.device);
        if (!result && !provider_result) ++completed;
        if (!result) result = provider_result;
    }
    int release = rsf_fsr4vk_destroy(owner);
    std::printf("{\"contract_checks\":\"passed\",\"device_test\":true,\"create_result\":%d,\"evaluate_result\":%d,"
        "\"completed_frames\":%u,\"deferred_splits\":%u,\"duplicate_frames_refused\":%u,\"render_width\":%u,\"render_height\":%u,"
        "\"provider_version\":%llu,\"vendor_id\":%u,\"int8_compatibility\":%u,\"release_result\":%d}\n",
        create_result, result, completed, state.splits, duplicate_refused, width, height,
        static_cast<unsigned long long>(info.provider_version), info.vendor_id, info.int8_compatibility, release);
    if (!release) {
        for (auto& image : images) image.close(device);
        if (pool) device.function<PFN_vkDestroyCommandPool>("vkDestroyCommandPool")(device.device, pool, nullptr);
        device.close();
    }
    return result || release || completed != 6 || duplicate_refused != 6 ? 1 : 0;
}
