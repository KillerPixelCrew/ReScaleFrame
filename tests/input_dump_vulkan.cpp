// SPDX-License-Identifier: GPL-3.0-only
#include <rescaleframe/input_dump.h>
#include <rescaleframe/input_dump_vulkan.h>
#include "vulkan_interop_fixture.h"
#include <cstdio>
#include <filesystem>
#include <fstream>
#include <iterator>

namespace {
bool passed = true;
void check(bool okay, const char* what) { if (!okay) { std::fprintf(stderr, "%s\n", what); passed = false; } }
std::string read(const std::filesystem::path& path) { std::ifstream in(path, std::ios::binary); return {std::istreambuf_iterator<char>(in), {}}; }
int32_t submit(void* user, uint32_t count, const void* values, uint64_t fence) {
    const auto& device = *static_cast<rsf_fixture::Device*>(user);
    return device.function<PFN_vkQueueSubmit>("vkQueueSubmit")(device.queue, count, static_cast<const VkSubmitInfo*>(values), reinterpret_cast<VkFence>(fence));
}
}
int main(int argc, char** argv)
{
    if (argc != 2) return 2;
    SetEnvironmentVariableW(L"RSF_VULKAN_FRAME_CAPTURE_DIR", nullptr);
    rsf_fixture::Device device;
    const auto initialized = device.init();
    if (initialized) { std::printf("Vulkan capture device unavailable: %d\n", initialized); device.close(); return 77; }
    rsf_fixture::Image image;
    check(image.init(device, VK_FORMAT_R8G8B8A8_UNORM, 4, 2) == VK_SUCCESS, "Create Vulkan capture source.");
    VkCommandPool pool{}; VkCommandBuffer commands{};
    VkCommandPoolCreateInfo pool_info{}; pool_info.sType = VK_STRUCTURE_TYPE_COMMAND_POOL_CREATE_INFO; pool_info.queueFamilyIndex = device.family;
    check(device.function<PFN_vkCreateCommandPool>("vkCreateCommandPool")(device.device, &pool_info, nullptr, &pool) == VK_SUCCESS, "Create source command pool.");
    VkCommandBufferAllocateInfo allocation{}; allocation.sType = VK_STRUCTURE_TYPE_COMMAND_BUFFER_ALLOCATE_INFO;
    allocation.commandPool = pool; allocation.level = VK_COMMAND_BUFFER_LEVEL_PRIMARY; allocation.commandBufferCount = 1;
    check(device.function<PFN_vkAllocateCommandBuffers>("vkAllocateCommandBuffers")(device.device, &allocation, &commands) == VK_SUCCESS, "Allocate source commands.");
    VkCommandBufferBeginInfo begin{}; begin.sType = VK_STRUCTURE_TYPE_COMMAND_BUFFER_BEGIN_INFO;
    check(device.function<PFN_vkBeginCommandBuffer>("vkBeginCommandBuffer")(commands, &begin) == VK_SUCCESS, "Begin source clear.");
    VkImageMemoryBarrier barrier{}; barrier.sType = VK_STRUCTURE_TYPE_IMAGE_MEMORY_BARRIER;
    barrier.srcQueueFamilyIndex = barrier.dstQueueFamilyIndex = VK_QUEUE_FAMILY_IGNORED;
    barrier.oldLayout = VK_IMAGE_LAYOUT_UNDEFINED; barrier.newLayout = VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL;
    barrier.dstAccessMask = VK_ACCESS_TRANSFER_WRITE_BIT; barrier.image = image.image;
    barrier.subresourceRange = {VK_IMAGE_ASPECT_COLOR_BIT, 0, 1, 0, 1};
    const auto pipeline_barrier = device.function<PFN_vkCmdPipelineBarrier>("vkCmdPipelineBarrier");
    pipeline_barrier(commands, VK_PIPELINE_STAGE_TOP_OF_PIPE_BIT, VK_PIPELINE_STAGE_TRANSFER_BIT, 0, 0, nullptr, 0, nullptr, 1, &barrier);
    VkClearColorValue color{}; color.float32[0] = .25f; color.float32[1] = .5f; color.float32[2] = 1; color.float32[3] = 0;
    device.function<PFN_vkCmdClearColorImage>("vkCmdClearColorImage")(commands, image.image, VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL, &color, 1, &barrier.subresourceRange);
    barrier.oldLayout = VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL; barrier.newLayout = VK_IMAGE_LAYOUT_SHADER_READ_ONLY_OPTIMAL;
    barrier.srcAccessMask = VK_ACCESS_TRANSFER_WRITE_BIT; barrier.dstAccessMask = VK_ACCESS_SHADER_READ_BIT;
    pipeline_barrier(commands, VK_PIPELINE_STAGE_TRANSFER_BIT, VK_PIPELINE_STAGE_ALL_COMMANDS_BIT, 0, 0, nullptr, 0, nullptr, 1, &barrier);
    check(device.function<PFN_vkEndCommandBuffer>("vkEndCommandBuffer")(commands) == VK_SUCCESS, "End source clear.");
    VkSubmitInfo source_submit{}; source_submit.sType = VK_STRUCTURE_TYPE_SUBMIT_INFO; source_submit.commandBufferCount = 1; source_submit.pCommandBuffers = &commands;
    check(submit(&device, 1, &source_submit, 0) == VK_SUCCESS, "Submit source rendering before capture.");
    const auto root = std::filesystem::path(argv[1]) / ("core-dump-vulkan-" + std::to_string(GetCurrentProcessId()) + '-' + std::to_string(GetTickCount64()));
    rsf_input_dump_configure(root.string().c_str(), "FixtureVulkan", "fixture", nullptr, nullptr);
    rsf_input_dump_request();
    rsf_frame_record record{}; record.struct_size = sizeof(record); record.abi_version = RSF_GAME_FRAME_ABI_VERSION;
    record.session_id = 31; record.frame_id = 32; record.view_id = 33; record.render_width = record.output_width = 4; record.render_height = record.output_height = 2;
    check(rsf_input_dump_begin_record(&record, 2) != 0, "Begin an API-neutral core capture.");
    rsf_input_dump_vk_setup setup{sizeof(setup), 1, device.setup(), device.queue, device.family, submit, &device};
    rsf_input_dump_vk* capture = nullptr;
    check(rsf_input_dump_vk_create(&setup, &capture) == 0 && capture, "Create core Vulkan reader without a vendor SDK.");
    const auto input = image.input(device, VK_IMAGE_LAYOUT_SHADER_READ_ONLY_OPTIMAL);
    check(rsf_input_dump_vk_snapshot(capture, &record, &input, "final_color") == 0, "Read actual Vulkan bytes after its queue's source writes.");
    rsf_input_dump_complete(&record, 0);
    std::filesystem::path bundle;
    for (const auto& entry : std::filesystem::directory_iterator(root / "website")) bundle = entry.path();
    const auto raw = read(bundle / "final_color.bin");
    check(raw.size() == 32, "Vulkan raw output must be tightly packed.");
    for (size_t i = 0; i + 3 < raw.size(); i += 4) check(uint8_t(raw[i]) == 64 && uint8_t(raw[i + 1]) == 128 && uint8_t(raw[i + 2]) == 255 && raw[i + 3] == 0,
        "Keep Vulkan color and original alpha bytes exactly.");
    check(read(bundle / "final_color_raw.json").find("\"native_api\":3") != std::string::npos, "Preserve native Vulkan format identity separately from preview storage.");
    check(read(bundle / "manifest.json").find("\"status\":\"complete\"") != std::string::npos, "Complete the core bundle after GPU readback.");
    check(read(bundle / "final_color.png").substr(1, 3) == "PNG", "Write a browser preview from the real Vulkan readback.");
    check(rsf_input_dump_vk_destroy(capture) == 0, "Retire the reader after its actual completion fence.");
    device.function<PFN_vkDeviceWaitIdle>("vkDeviceWaitIdle")(device.device);
    device.function<PFN_vkDestroyCommandPool>("vkDestroyCommandPool")(device.device, pool, nullptr);
    image.close(device); device.close();
    std::printf("Vulkan core dump fixture: %s\n", bundle.string().c_str());
    return passed ? 0 : 1;
}
