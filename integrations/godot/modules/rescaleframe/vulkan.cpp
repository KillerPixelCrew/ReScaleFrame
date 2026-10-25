// SPDX-License-Identifier: GPL-3.0-only
#include "drivers/vulkan/rendering_device_driver_vulkan.h"
#include "rsf_godot_engine.h"
#include "api.h"
#include "core/os/os.h"
#include "core/string/print_string.h"

int32_t RenderingDeviceDriverVulkan::rescaleframe_submit(void *user, uint32_t count, const void *submits, uint64_t fence) {
    auto *chain = static_cast<SwapChain *>(user);
    auto *driver = chain->rescaleframe_driver;
    auto &queue = driver->queue_families[chain->rescaleframe_family][0];
    MutexLock lock(queue.submit_mutex);
    return vkQueueSubmit(queue.queue, count, static_cast<const VkSubmitInfo *>(submits), reinterpret_cast<VkFence>(fence));
}
void RenderingDeviceDriverVulkan::rescaleframe_lock_queues(void *user) {
    auto *driver = static_cast<SwapChain *>(user)->rescaleframe_driver;
    for (auto &family : driver->queue_families) for (auto &queue : family) queue.submit_mutex.lock();
}
void RenderingDeviceDriverVulkan::rescaleframe_unlock_queues(void *user) {
    auto *driver = static_cast<SwapChain *>(user)->rescaleframe_driver;
    for (uint32_t i = driver->queue_families.size(); i > 0; --i) {
        auto &family = driver->queue_families[i - 1];
        for (uint32_t j = family.size(); j > 0; --j) family[j - 1].submit_mutex.unlock();
    }
}
bool RenderingDeviceDriverVulkan::rescaleframe_create(CommandQueueID queue_id, void *opaque_chain, const void *info, const void *allocation_callbacks) {
    auto *queue = reinterpret_cast<CommandQueue *>(queue_id.id);
    auto *chain = static_cast<SwapChain *>(opaque_chain);
    if (!RSFGodot::vulkan_enabled() || !rescaleframe_timeline || queue->queue_index != 0 || queue_families[queue->queue_family].size() < 4) return false;
    chain->rescaleframe_driver = this; chain->rescaleframe_family = queue->queue_family;
    rsf_fgvk_setup setup{}; setup.struct_size = sizeof(setup); setup.abi_version = RSF_FGVK_ABI_VERSION;
    auto &graphics = setup.graphics;
    graphics.struct_size = sizeof(graphics); graphics.abi_version = RSF_SRVK_ABI_VERSION;
    graphics.instance = context_driver->instance_get(); graphics.physical_device = physical_device; graphics.device = vk_device;
    graphics.instance_proc = reinterpret_cast<rsf_vk_get_proc>(vkGetInstanceProcAddr);
    graphics.device_proc = reinterpret_cast<rsf_vk_get_proc>(vkGetDeviceProcAddr); graphics.depth_inverted = 1;
    for (uint32_t i = 0; i < 4; ++i) setup.queues[i] = queue_families[queue->queue_family][i].queue;
    setup.queue_family = queue->queue_family; setup.create_info = info; setup.allocator = allocation_callbacks;
    setup.graphics_submit = rescaleframe_submit; setup.lock_queues = rescaleframe_lock_queues;
    setup.unlock_queues = rescaleframe_unlock_queues; setup.queue_user = chain;
    uint64_t handle = 0;
    if (!RSFGodot::vk_create(&setup, &chain->rescaleframe_owner, &handle)) return false;
    chain->vk_swapchain = reinterpret_cast<VkSwapchainKHR>(handle); return true;
}
bool RenderingDeviceDriverVulkan::rescaleframe_release(void *opaque_chain) {
    auto *chain = static_cast<SwapChain *>(opaque_chain);
    if (!chain->rescaleframe_owner) return true;
    if (!RSFGodot::vk_destroy(chain->rescaleframe_owner)) return false;
    chain->rescaleframe_owner = nullptr; chain->vk_swapchain = VK_NULL_HANDLE; return true;
}
bool RenderingDeviceDriverVulkan::rescaleframe_drain(void *opaque_chain) {
    auto *chain = static_cast<SwapChain *>(opaque_chain);
    return !chain->rescaleframe_owner || RSFGodot::vk_drain(chain->rescaleframe_owner);
}
VkResult RenderingDeviceDriverVulkan::rescaleframe_images(void *opaque_chain, uint32_t *count, VkImage *images) {
    auto *chain = static_cast<SwapChain *>(opaque_chain);
    return chain->rescaleframe_owner ? VkResult(RSFGodot::vk_images(chain->rescaleframe_owner, count, reinterpret_cast<uint64_t *>(images))) :
        device_functions.GetSwapchainImagesKHR(vk_device, chain->vk_swapchain, count, images);
}
VkResult RenderingDeviceDriverVulkan::rescaleframe_acquire(void *opaque_chain, uint64_t timeout, VkSemaphore semaphore, VkFence fence, uint32_t *index) {
    auto *chain = static_cast<SwapChain *>(opaque_chain);
    return chain->rescaleframe_owner ? VkResult(RSFGodot::vk_acquire(chain->rescaleframe_owner, timeout, reinterpret_cast<uint64_t>(semaphore), reinterpret_cast<uint64_t>(fence), index)) :
        device_functions.AcquireNextImageKHR(vk_device, chain->vk_swapchain, timeout, semaphore, fence, index);
}
VkResult RenderingDeviceDriverVulkan::rescaleframe_present(CommandQueueID queue_id, const VkPresentInfoKHR *info, VectorView<SwapChainID> chains, bool submitted) {
    auto *queue = reinterpret_cast<CommandQueue *>(queue_id.id);
    if (chains.size() == 1) {
        auto *chain = reinterpret_cast<SwapChain *>(chains[0].id);
        if (chain->rescaleframe_owner) return VkResult(RSFGodot::vk_present(chain->rescaleframe_owner, info));
    }
    bool has_proxy = false;
    for (uint32_t i = 0; i < chains.size(); ++i) has_proxy |= reinterpret_cast<SwapChain *>(chains[i].id)->rescaleframe_owner != nullptr;
    auto graphics = queue_families[queue->queue_family][queue->queue_index].queue;
    if (has_proxy) {
        if (OS::get_singleton()->has_environment("RSF_GODOT_WSI_TRACE")) {
            static uint32_t diagnostics = 0;
            if (diagnostics++ < 3) {
                print_line(vformat("RSF WSI mixed: submitted=%d chains=%d waits=%d", int(submitted), chains.size(), info->waitSemaphoreCount));
                for (uint32_t i = 0; i < info->waitSemaphoreCount; ++i) print_line(vformat("RSF WSI wait=%d", uint64_t(info->pWaitSemaphores[i])));
                for (uint32_t i = 0; i < chains.size(); ++i) {
                    const auto *chain = reinterpret_cast<SwapChain *>(chains[i].id);
                    print_line(vformat("RSF WSI chain=%d proxy=%d image=%d semaphore=%d", uint64_t(chain->vk_swapchain), int(chain->rescaleframe_owner != nullptr),
                        chain->image_index, uint64_t(chain->present_semaphores[chain->image_index])));
                }
            }
        }
        if (info->pNext) return VK_ERROR_FEATURE_NOT_PRESENT;
        if (!submitted) {
            // Consume the shared wait list once and fan out one completion
            // semaphore per chain. A binary semaphore cannot be waited twice.
            LocalVector<VkPipelineStageFlags> stages;
            stages.resize(info->waitSemaphoreCount);
            for (uint32_t i = 0; i < stages.size(); ++i) stages[i] = VK_PIPELINE_STAGE_ALL_COMMANDS_BIT;
            LocalVector<VkSemaphore> signals;
            for (uint32_t i = 0; i < chains.size(); ++i) {
                const auto *chain = reinterpret_cast<SwapChain *>(chains[i].id);
                signals.push_back(chain->present_semaphores[chain->image_index]);
            }
            VkSubmitInfo submit{}; submit.sType = VK_STRUCTURE_TYPE_SUBMIT_INFO;
            submit.waitSemaphoreCount = info->waitSemaphoreCount; submit.pWaitSemaphores = info->pWaitSemaphores; submit.pWaitDstStageMask = stages.ptr();
            submit.signalSemaphoreCount = signals.size(); submit.pSignalSemaphores = signals.ptr();
            VkResult result = vkQueueSubmit(graphics, 1, &submit, VK_NULL_HANDLE);
            if (result != VK_SUCCESS) return result;
        }
        VkResult result = VK_SUCCESS;
        for (uint32_t i = 0; i < chains.size(); ++i) {
            const auto *chain = reinterpret_cast<SwapChain *>(chains[i].id);
            VkPresentInfoKHR one = *info;
            one.swapchainCount = one.waitSemaphoreCount = 1;
            one.pSwapchains = &chain->vk_swapchain; one.pImageIndices = &chain->image_index;
            one.pWaitSemaphores = &chain->present_semaphores[chain->image_index]; one.pResults = info->pResults ? &info->pResults[i] : nullptr;
            VkResult item = chain->rescaleframe_owner ? VkResult(RSFGodot::vk_present(chain->rescaleframe_owner, &one)) : device_functions.QueuePresentKHR(graphics, &one);
            if (one.pResults) *one.pResults = item;
            if (item != VK_SUCCESS) result = item;
        }
        return result;
    }
    return device_functions.QueuePresentKHR(graphics, info);
}

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
