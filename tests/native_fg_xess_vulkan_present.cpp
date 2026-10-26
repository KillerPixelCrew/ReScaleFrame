// SPDX-License-Identifier: GPL-3.0-only
#define VK_USE_PLATFORM_WIN32_KHR
#include <windows.h>
#include <vulkan/vulkan.h>
#include <array>
#include <cstdio>
#include "../runtime/orchestrator/src/native_fg_xess_vulkan.h"

namespace {
struct Queue { VkQueue queue; PFN_vkQueueSubmit submit; };
int32_t submit(void* data, uint32_t count, const void* submits, uint64_t fence) {
    const auto& queue = *static_cast<Queue*>(data);
    return queue.submit(queue.queue, count, static_cast<const VkSubmitInfo*>(submits), reinterpret_cast<VkFence>(fence));
}
void lock(void*) {} // Fixture is single-threaded; there is no engine queue user.
template<typename T> T proc(const rsf_srvk_setup& setup, const char* name) {
    return reinterpret_cast<T>(setup.device_proc(setup.device, name));
}
}

int RunXessPresentationFixture(const rsf_srvk_setup& graphics, uint32_t family, const wchar_t* runtime) {
    WNDCLASSW klass{}; klass.lpfnWndProc = DefWindowProcW; klass.hInstance = GetModuleHandleW(nullptr);
    klass.lpszClassName = L"RsfXeSSInteropFixture";
    if (!RegisterClassW(&klass)) return 100;
    HWND window = CreateWindowExW(0, klass.lpszClassName, L"ReScaleFrame XeSS interop fixture",
        WS_OVERLAPPEDWINDOW, CW_USEDEFAULT, CW_USEDEFAULT, 1296, 759, nullptr, nullptr, klass.hInstance, nullptr);
    if (!window) { UnregisterClassW(klass.lpszClassName, klass.hInstance); return 101; }
    ShowWindow(window, SW_SHOWNORMAL); SetForegroundWindow(window);
    VkDevice device = static_cast<VkDevice>(graphics.device);
    Queue queue{};
    proc<PFN_vkGetDeviceQueue>(graphics, "vkGetDeviceQueue")(device, family, 0, &queue.queue);
    queue.submit = proc<PFN_vkQueueSubmit>(graphics, "vkQueueSubmit");
    auto idle = proc<PFN_vkDeviceWaitIdle>(graphics, "vkDeviceWaitIdle");
    struct Guide { VkImage image{}; VkDeviceMemory memory{}; uint32_t format{}; };
    std::array<Guide, 2> guides{};
    guides[0].format = VK_FORMAT_R32_SFLOAT; guides[1].format = VK_FORMAT_R16G16_SFLOAT;
    VkSwapchainCreateInfoKHR info{VK_STRUCTURE_TYPE_SWAPCHAIN_CREATE_INFO_KHR};
    info.minImageCount = 3; info.imageFormat = VK_FORMAT_R8G8B8A8_UNORM;
    info.imageColorSpace = VK_COLOR_SPACE_SRGB_NONLINEAR_KHR;
    info.imageExtent = {1280, 720}; info.imageArrayLayers = 1;
    info.imageUsage = VK_IMAGE_USAGE_COLOR_ATTACHMENT_BIT | VK_IMAGE_USAGE_TRANSFER_DST_BIT | VK_IMAGE_USAGE_TRANSFER_SRC_BIT;
    info.imageSharingMode = VK_SHARING_MODE_EXCLUSIVE;
    info.presentMode = VK_PRESENT_MODE_FIFO_KHR;
    rsf_fgvk_setup setup{}; setup.struct_size = sizeof(setup); setup.abi_version = RSF_FGVK_ABI_VERSION;
    setup.graphics = graphics; setup.queue_family = family; setup.create_info = &info;
    for (auto& entry : setup.queues) entry = queue.queue;
    setup.graphics_submit = submit; setup.lock_queues = lock; setup.unlock_queues = lock; setup.queue_user = &queue;
    rsf_xessgvk_options options{sizeof(options), 1, window, 1, 1, reinterpret_cast<const uint16_t*>(runtime)};
    rsf_xessgvk* owner = nullptr; uint64_t chain = 0;
    int result = rsf_xessgvk_create(&setup, &options, &owner, &chain);
    const int create_result = result;
    VkCommandPool pool{}; VkCommandBuffer commands{};
    VkSemaphore acquired{}, finished{};
    VkRenderPass render_pass{};
    std::array<VkImageView, 3> views{};
    std::array<VkFramebuffer, 3> framebuffers{};
    uint32_t completed = 0;
    uint32_t interpolated_statuses = 0;
    auto destroyPool = proc<PFN_vkDestroyCommandPool>(graphics, "vkDestroyCommandPool");
    auto destroySemaphore = proc<PFN_vkDestroySemaphore>(graphics, "vkDestroySemaphore");
    if (!result) {
        VkCommandPoolCreateInfo create{VK_STRUCTURE_TYPE_COMMAND_POOL_CREATE_INFO};
        create.flags = VK_COMMAND_POOL_CREATE_RESET_COMMAND_BUFFER_BIT; create.queueFamilyIndex = family;
        result = proc<PFN_vkCreateCommandPool>(graphics, "vkCreateCommandPool")(device, &create, nullptr, &pool);
        VkCommandBufferAllocateInfo allocate{VK_STRUCTURE_TYPE_COMMAND_BUFFER_ALLOCATE_INFO};
        allocate.commandPool = pool; allocate.level = VK_COMMAND_BUFFER_LEVEL_PRIMARY; allocate.commandBufferCount = 1;
        if (!result) result = proc<PFN_vkAllocateCommandBuffers>(graphics, "vkAllocateCommandBuffers")(device, &allocate, &commands);
        VkSemaphoreCreateInfo semaphore{VK_STRUCTURE_TYPE_SEMAPHORE_CREATE_INFO};
        auto createSemaphore = proc<PFN_vkCreateSemaphore>(graphics, "vkCreateSemaphore");
        if (!result) result = createSemaphore(device, &semaphore, nullptr, &acquired);
        if (!result) result = createSemaphore(device, &semaphore, nullptr, &finished);
        auto getMemory = reinterpret_cast<PFN_vkGetPhysicalDeviceMemoryProperties>(
            graphics.instance_proc(graphics.instance, "vkGetPhysicalDeviceMemoryProperties"));
        VkPhysicalDeviceMemoryProperties memory{};
        getMemory(static_cast<VkPhysicalDevice>(graphics.physical_device), &memory);
        for (auto& guide : guides) {
            if (result) break;
            VkImageCreateInfo createImage{VK_STRUCTURE_TYPE_IMAGE_CREATE_INFO};
            createImage.imageType = VK_IMAGE_TYPE_2D; createImage.format = static_cast<VkFormat>(guide.format);
            createImage.extent = {640, 360, 1}; createImage.mipLevels = createImage.arrayLayers = 1;
            createImage.samples = VK_SAMPLE_COUNT_1_BIT; createImage.tiling = VK_IMAGE_TILING_OPTIMAL;
            createImage.usage = VK_IMAGE_USAGE_SAMPLED_BIT | VK_IMAGE_USAGE_TRANSFER_SRC_BIT | VK_IMAGE_USAGE_TRANSFER_DST_BIT;
            result = proc<PFN_vkCreateImage>(graphics, "vkCreateImage")(device, &createImage, nullptr, &guide.image);
            VkMemoryRequirements requirements{};
            if (!result) proc<PFN_vkGetImageMemoryRequirements>(graphics, "vkGetImageMemoryRequirements")(device, guide.image, &requirements);
            VkMemoryAllocateInfo allocateMemory{VK_STRUCTURE_TYPE_MEMORY_ALLOCATE_INFO}; allocateMemory.allocationSize = requirements.size;
            allocateMemory.memoryTypeIndex = memory.memoryTypeCount;
            for (uint32_t i = 0; i < memory.memoryTypeCount; ++i)
                if ((requirements.memoryTypeBits & (1u << i)) && (memory.memoryTypes[i].propertyFlags & VK_MEMORY_PROPERTY_DEVICE_LOCAL_BIT)) {
                    allocateMemory.memoryTypeIndex = i; break;
                }
            if (!result && allocateMemory.memoryTypeIndex == memory.memoryTypeCount) result = VK_ERROR_OUT_OF_DEVICE_MEMORY;
            if (!result) result = proc<PFN_vkAllocateMemory>(graphics, "vkAllocateMemory")(device, &allocateMemory, nullptr, &guide.memory);
            if (!result) result = proc<PFN_vkBindImageMemory>(graphics, "vkBindImageMemory")(device, guide.image, guide.memory, 0);
        }
    }
    std::array<uint64_t, 3> images{};
    uint32_t count = static_cast<uint32_t>(images.size());
    if (!result) result = rsf_xessgvk_images(owner, &count, images.data());
    if (!result) {
        VkAttachmentDescription attachment{}; attachment.format = VK_FORMAT_R8G8B8A8_UNORM;
        attachment.samples = VK_SAMPLE_COUNT_1_BIT; attachment.loadOp = VK_ATTACHMENT_LOAD_OP_LOAD;
        attachment.storeOp = VK_ATTACHMENT_STORE_OP_STORE;
        attachment.initialLayout = attachment.finalLayout = VK_IMAGE_LAYOUT_COLOR_ATTACHMENT_OPTIMAL;
        VkAttachmentReference color{0, VK_IMAGE_LAYOUT_COLOR_ATTACHMENT_OPTIMAL};
        VkSubpassDescription subpass{}; subpass.pipelineBindPoint = VK_PIPELINE_BIND_POINT_GRAPHICS;
        subpass.colorAttachmentCount = 1; subpass.pColorAttachments = &color;
        VkRenderPassCreateInfo create{VK_STRUCTURE_TYPE_RENDER_PASS_CREATE_INFO};
        create.attachmentCount = 1; create.pAttachments = &attachment; create.subpassCount = 1; create.pSubpasses = &subpass;
        result = proc<PFN_vkCreateRenderPass>(graphics, "vkCreateRenderPass")(device, &create, nullptr, &render_pass);
        for (uint32_t i = 0; i < images.size() && !result; ++i) {
            VkImageViewCreateInfo view{VK_STRUCTURE_TYPE_IMAGE_VIEW_CREATE_INFO};
            view.image = reinterpret_cast<VkImage>(images[i]); view.viewType = VK_IMAGE_VIEW_TYPE_2D;
            view.format = VK_FORMAT_R8G8B8A8_UNORM; view.subresourceRange = {VK_IMAGE_ASPECT_COLOR_BIT, 0, 1, 0, 1};
            result = proc<PFN_vkCreateImageView>(graphics, "vkCreateImageView")(device, &view, nullptr, &views[i]);
            VkFramebufferCreateInfo framebuffer{VK_STRUCTURE_TYPE_FRAMEBUFFER_CREATE_INFO};
            framebuffer.renderPass = render_pass; framebuffer.attachmentCount = 1; framebuffer.pAttachments = &views[i];
            framebuffer.width = 1280; framebuffer.height = 720; framebuffer.layers = 1;
            if (!result) result = proc<PFN_vkCreateFramebuffer>(graphics, "vkCreateFramebuffer")(device, &framebuffer, nullptr, &framebuffers[i]);
        }
    }
    if (!result) result = rsf_xessgvk_enable(owner, 1);
    std::array<bool, 3> initialized{};
    for (uint32_t frame = 0; !result && frame < 12; ++frame) {
        MSG message{};
        while (PeekMessageW(&message, nullptr, 0, 0, PM_REMOVE)) { TranslateMessage(&message); DispatchMessageW(&message); }
        result = idle(device);
        if (!result) result = rsf_xessgvk_sleep(owner, frame + 1);
        if (!result) result = rsf_xessgvk_latency(owner, frame + 1, 0);
        if (!result) result = rsf_xessgvk_latency(owner, frame + 1, 1);
        if (!result) result = rsf_xessgvk_latency(owner, frame + 1, 2);
        uint32_t index = 0;
        if (!result) result = rsf_xessgvk_acquire(owner, 10000000000ull, reinterpret_cast<uint64_t>(acquired), 0, &index);
        if (result) break;
        result = proc<PFN_vkResetCommandBuffer>(graphics, "vkResetCommandBuffer")(commands, 0);
        VkCommandBufferBeginInfo begin{VK_STRUCTURE_TYPE_COMMAND_BUFFER_BEGIN_INFO}; begin.flags = VK_COMMAND_BUFFER_USAGE_ONE_TIME_SUBMIT_BIT;
        if (!result) result = proc<PFN_vkBeginCommandBuffer>(graphics, "vkBeginCommandBuffer")(commands, &begin);
        if (result) break;
        VkImageMemoryBarrier barrier{VK_STRUCTURE_TYPE_IMAGE_MEMORY_BARRIER};
        barrier.srcAccessMask = initialized[index] ? VK_ACCESS_MEMORY_READ_BIT : 0;
        barrier.dstAccessMask = VK_ACCESS_TRANSFER_WRITE_BIT;
        barrier.oldLayout = initialized[index] ? VK_IMAGE_LAYOUT_SHADER_READ_ONLY_OPTIMAL : VK_IMAGE_LAYOUT_UNDEFINED;
        barrier.newLayout = VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL;
        barrier.srcQueueFamilyIndex = barrier.dstQueueFamilyIndex = VK_QUEUE_FAMILY_IGNORED;
        barrier.image = reinterpret_cast<VkImage>(images[index]);
        barrier.subresourceRange = {VK_IMAGE_ASPECT_COLOR_BIT, 0, 1, 0, 1};
        auto pipelineBarrier = proc<PFN_vkCmdPipelineBarrier>(graphics, "vkCmdPipelineBarrier");
        pipelineBarrier(commands, VK_PIPELINE_STAGE_ALL_COMMANDS_BIT, VK_PIPELINE_STAGE_TRANSFER_BIT, 0, 0, nullptr, 0, nullptr, 1, &barrier);
        VkClearColorValue color{{0.1f, 0.25f, 0.5f, 1.0f}};
        proc<PFN_vkCmdClearColorImage>(graphics, "vkCmdClearColorImage")(commands, barrier.image,
            VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL, &color, 1, &barrier.subresourceRange);
        barrier.srcAccessMask = VK_ACCESS_TRANSFER_WRITE_BIT; barrier.dstAccessMask = VK_ACCESS_SHADER_READ_BIT;
        barrier.oldLayout = VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL; barrier.newLayout = VK_IMAGE_LAYOUT_SHADER_READ_ONLY_OPTIMAL;
        pipelineBarrier(commands, VK_PIPELINE_STAGE_TRANSFER_BIT, VK_PIPELINE_STAGE_ALL_COMMANDS_BIT, 0, 0, nullptr, 0, nullptr, 1, &barrier);
        barrier.srcAccessMask = VK_ACCESS_SHADER_READ_BIT;
        barrier.dstAccessMask = VK_ACCESS_COLOR_ATTACHMENT_READ_BIT | VK_ACCESS_COLOR_ATTACHMENT_WRITE_BIT;
        barrier.oldLayout = VK_IMAGE_LAYOUT_SHADER_READ_ONLY_OPTIMAL; barrier.newLayout = VK_IMAGE_LAYOUT_COLOR_ATTACHMENT_OPTIMAL;
        pipelineBarrier(commands, VK_PIPELINE_STAGE_ALL_COMMANDS_BIT, VK_PIPELINE_STAGE_COLOR_ATTACHMENT_OUTPUT_BIT,
            0, 0, nullptr, 0, nullptr, 1, &barrier);
        VkRenderPassBeginInfo draw{VK_STRUCTURE_TYPE_RENDER_PASS_BEGIN_INFO};
        draw.renderPass = render_pass; draw.framebuffer = framebuffers[index]; draw.renderArea.extent = {1280, 720};
        proc<PFN_vkCmdBeginRenderPass>(graphics, "vkCmdBeginRenderPass")(commands, &draw, VK_SUBPASS_CONTENTS_INLINE);
        VkClearAttachment rectangle{}; rectangle.aspectMask = VK_IMAGE_ASPECT_COLOR_BIT;
        rectangle.clearValue.color.float32[0] = 0.02f; rectangle.clearValue.color.float32[1] = 0.9f;
        rectangle.clearValue.color.float32[2] = 0.9f; rectangle.clearValue.color.float32[3] = 1;
        VkClearRect region{}; region.rect.offset = {300 + static_cast<int32_t>(frame) * 8, 200};
        region.rect.extent = {80, 120}; region.layerCount = 1;
        proc<PFN_vkCmdClearAttachments>(graphics, "vkCmdClearAttachments")(commands, 1, &rectangle, 1, &region);
        proc<PFN_vkCmdEndRenderPass>(graphics, "vkCmdEndRenderPass")(commands);
        barrier.srcAccessMask = VK_ACCESS_COLOR_ATTACHMENT_WRITE_BIT; barrier.dstAccessMask = VK_ACCESS_SHADER_READ_BIT;
        barrier.oldLayout = VK_IMAGE_LAYOUT_COLOR_ATTACHMENT_OPTIMAL; barrier.newLayout = VK_IMAGE_LAYOUT_SHADER_READ_ONLY_OPTIMAL;
        pipelineBarrier(commands, VK_PIPELINE_STAGE_COLOR_ATTACHMENT_OUTPUT_BIT, VK_PIPELINE_STAGE_ALL_COMMANDS_BIT,
            0, 0, nullptr, 0, nullptr, 1, &barrier);
        if (frame == 0) for (const auto& guide : guides) {
            barrier.srcAccessMask = 0; barrier.dstAccessMask = VK_ACCESS_TRANSFER_WRITE_BIT;
            barrier.oldLayout = VK_IMAGE_LAYOUT_UNDEFINED; barrier.newLayout = VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL;
            barrier.image = guide.image;
            pipelineBarrier(commands, VK_PIPELINE_STAGE_ALL_COMMANDS_BIT, VK_PIPELINE_STAGE_TRANSFER_BIT, 0, 0, nullptr, 0, nullptr, 1, &barrier);
            // A plane translating eight output pixels right per source frame.
            // Motion is current-to-previous UV; render scale converts it to -4px.
            VkClearColorValue constant{}; constant.float32[0] = guide.format == VK_FORMAT_R32_SFLOAT ? 0.5f : -8.0f / 1280.0f;
            proc<PFN_vkCmdClearColorImage>(graphics, "vkCmdClearColorImage")(commands, guide.image,
                VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL, &constant, 1, &barrier.subresourceRange);
            barrier.srcAccessMask = VK_ACCESS_TRANSFER_WRITE_BIT; barrier.dstAccessMask = VK_ACCESS_SHADER_READ_BIT;
            barrier.oldLayout = VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL; barrier.newLayout = VK_IMAGE_LAYOUT_SHADER_READ_ONLY_OPTIMAL;
            pipelineBarrier(commands, VK_PIPELINE_STAGE_TRANSFER_BIT, VK_PIPELINE_STAGE_ALL_COMMANDS_BIT, 0, 0, nullptr, 0, nullptr, 1, &barrier);
        }
        rsf_frame_record record{}; record.struct_size = sizeof(record); record.abi_version = RSF_GAME_FRAME_ABI_VERSION;
        record.frame_id = frame + 1; record.session_id = 1; record.view_id = 1; record.resource_generation = 1;
        record.flags = frame == 0 ? RSF_FRAME_FLAG_RESET : 0; record.frame_time_ms = 16.666667f;
        record.render_width = 640; record.render_height = 360; record.output_width = 1280; record.output_height = 720;
        record.camera.struct_size = sizeof(record.camera); record.camera.abi_version = RSF_GAME_FRAME_ABI_VERSION;
        record.camera.render_width = 640; record.camera.render_height = 360;
        record.camera.output_width = 1280; record.camera.output_height = 720;
        record.camera.near_plane = 1; record.camera.far_plane = 100; record.camera.vertical_fov_radians = 1;
        for (uint32_t i = 0; i < 16; i += 5) record.camera.world_to_view[i] = record.camera.view_to_clip[i] = 1;
        rsf_fgvk_frame inputs{}; inputs.struct_size = sizeof(inputs); inputs.abi_version = RSF_FGVK_ABI_VERSION;
        inputs.record = &record; inputs.motion_scale_x = 640; inputs.motion_scale_y = 360;
        inputs.depth = {graphics.device, reinterpret_cast<uint64_t>(guides[0].image), guides[0].format, 640, 360, VK_IMAGE_LAYOUT_SHADER_READ_ONLY_OPTIMAL, 1};
        inputs.motion = {graphics.device, reinterpret_cast<uint64_t>(guides[1].image), guides[1].format, 640, 360, VK_IMAGE_LAYOUT_SHADER_READ_ONLY_OPTIMAL, 1};
        inputs.hudless = {graphics.device, images[index], VK_FORMAT_R8G8B8A8_UNORM, 1280, 720, VK_IMAGE_LAYOUT_SHADER_READ_ONLY_OPTIMAL, 1};
        result = rsf_xessgvk_prepare(owner, commands, &inputs);
        if (!result) result = rsf_xessgvk_hudless(owner, commands, &inputs);
        if (result) { proc<PFN_vkEndCommandBuffer>(graphics, "vkEndCommandBuffer")(commands); break; }
        result = proc<PFN_vkEndCommandBuffer>(graphics, "vkEndCommandBuffer")(commands);
        VkPipelineStageFlags stage = VK_PIPELINE_STAGE_TRANSFER_BIT;
        VkSubmitInfo source{VK_STRUCTURE_TYPE_SUBMIT_INFO};
        source.waitSemaphoreCount = 1; source.pWaitSemaphores = &acquired; source.pWaitDstStageMask = &stage;
        source.commandBufferCount = 1; source.pCommandBuffers = &commands;
        source.signalSemaphoreCount = 1; source.pSignalSemaphores = &finished;
        if (!result) result = queue.submit(queue.queue, 1, &source, VK_NULL_HANDLE);
        if (result) break;
        void* submitted = commands;
        rsf_xessgvk_submitted(owner, &submitted, 1);
        result = rsf_xessgvk_latency(owner, frame + 1, 3);
        if (result) break;
        initialized[index] = true;
        VkSwapchainKHR swapchain = reinterpret_cast<VkSwapchainKHR>(chain);
        VkPresentInfoKHR present{VK_STRUCTURE_TYPE_PRESENT_INFO_KHR};
        present.waitSemaphoreCount = 1; present.pWaitSemaphores = &finished;
        present.swapchainCount = 1; present.pSwapchains = &swapchain; present.pImageIndices = &index;
        result = rsf_xessgvk_present(owner, &present);
        rsf_fgvk_status status{sizeof(status), RSF_FGVK_ABI_VERSION};
        if (!rsf_xessgvk_status(owner, &status) && status.enabled && status.last_present_count > 1) ++interpolated_statuses;
        if (!result) ++completed;
    }
    int drain = owner ? rsf_xessgvk_drain(owner) : 0;
    if (!drain) {
        for (auto framebuffer : framebuffers) if (framebuffer) proc<PFN_vkDestroyFramebuffer>(graphics, "vkDestroyFramebuffer")(device, framebuffer, nullptr);
        for (auto view : views) if (view) proc<PFN_vkDestroyImageView>(graphics, "vkDestroyImageView")(device, view, nullptr);
        if (render_pass) proc<PFN_vkDestroyRenderPass>(graphics, "vkDestroyRenderPass")(device, render_pass, nullptr);
    }
    int release = drain ? -1 : rsf_xessgvk_destroy(owner);
    // No dependent owner is released if the presenter could not drain.
    if (!drain && !release) {
        idle(device);
        if (acquired) destroySemaphore(device, acquired, nullptr);
        if (finished) destroySemaphore(device, finished, nullptr);
        if (pool) destroyPool(device, pool, nullptr);
        for (const auto& guide : guides) {
            if (guide.image) proc<PFN_vkDestroyImage>(graphics, "vkDestroyImage")(device, guide.image, nullptr);
            if (guide.memory) proc<PFN_vkFreeMemory>(graphics, "vkFreeMemory")(device, guide.memory, nullptr);
        }
        DestroyWindow(window); UnregisterClassW(klass.lpszClassName, klass.hInstance);
    }
    std::printf("{\"presentation_fixture\":true,\"fg_requested\":true,\"create_result\":%d,"
        "\"completed_real_frames\":%u,\"interpolated_sdk_statuses\":%u,\"present_result\":%d,\"drain_result\":%d,\"release_result\":%d}\n",
        create_result, completed, interpolated_statuses, result, drain, release);
    return result || drain || release || completed != 12 ? 1 : 0;
}
