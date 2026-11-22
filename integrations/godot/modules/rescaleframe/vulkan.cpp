// SPDX-License-Identifier: MIT
#include "drivers/vulkan/rendering_device_driver_vulkan.h"
#include "rsf_godot_engine.h"
#include "api.h"
#include "frame.h"
#include "host.h"
#include "core/os/os.h"
#include "core/string/print_string.h"

namespace RSFGodot {
struct MixedPresentCalls {
    int32_t (*fan_out)(void *context);
    int32_t (*present)(void *context, uint32_t index);
};
static_assert(VK_ERROR_FEATURE_NOT_PRESENT == -8);
extern "C" int32_t rsf_godot_present_mixed(const MixedPresentCalls *calls, void *context,
        uint32_t count, uint32_t has_extensions, uint32_t submitted);
}

namespace {
thread_local RSFGodot::VulkanExecutionScope *execution_scope = nullptr;
}

RSFGodot::VulkanExecutionScope::VulkanExecutionScope(uint64_t queue_id, uint64_t fence_id, bool eligible) :
        previous(execution_scope), queue(queue_id), fence(fence_id) {
    execution_scope = this;
    if (eligible && fence && vk_execution_supported()) {
        rsf_godot_render_frame frame{};
        rsf_godot_render_frame_read(&frame);
        session = rsf_godot_host_session();
        source = frame.source_frame_id;
    }
}
RSFGodot::VulkanExecutionScope::~VulkanExecutionScope() { execution_scope = previous; }

void RSFGodot::VulkanExecutionScope::submit(uint64_t queue, uint64_t fence, bool begin) {
    auto *scope = execution_scope;
    if (!scope || !scope->session || !scope->source || scope->queue != queue || scope->fence != fence) return;
    scope->submitted = !begin;
    vk_execution(RSF_GAME_RENDER_SUBMISSION, scope->session, scope->source, 0, begin);
}
bool RSFGodot::VulkanExecutionScope::present(uint64_t window, bool begin) {
    if (!vk_execution_supported()) return vk_present_marker(window, begin);
    const auto *scope = execution_scope;
    return scope && scope->submitted && vk_execution(RSF_GAME_RENDER_PRESENT, scope->session, scope->source, window, begin);
}

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
bool RenderingDeviceDriverVulkan::rescaleframe_create(CommandQueueID queue_id, void *opaque_chain, void *info, const void *allocation_callbacks) {
    auto *queue = reinterpret_cast<CommandQueue *>(queue_id.id);
    auto *chain = static_cast<SwapChain *>(opaque_chain);
    const auto *surface = reinterpret_cast<const RenderingContextDriverVulkan::Surface *>(chain->surface);
    if (!surface || !surface->rescaleframe_primary) return false;
    if (!RSFGodot::vulkan_enabled() || queue->queue_index != 0) return false;
    chain->rescaleframe_driver = this; chain->rescaleframe_family = queue->queue_family;
    rsf_fgvk_setup setup{}; setup.struct_size = sizeof(setup); setup.abi_version = RSF_FGVK_ABI_VERSION;
    auto &graphics = setup.graphics;
    graphics.struct_size = sizeof(graphics); graphics.abi_version = RSF_SRVK_ABI_VERSION;
    graphics.instance = context_driver->instance_get(); graphics.physical_device = physical_device; graphics.device = vk_device;
    graphics.instance_proc = reinterpret_cast<rsf_vk_get_proc>(vkGetInstanceProcAddr);
    graphics.device_proc = reinterpret_cast<rsf_vk_get_proc>(vkGetDeviceProcAddr); graphics.depth_inverted = 1;
    auto &description = *static_cast<VkSwapchainCreateInfoKHR *>(info);
    VkSurfaceCapabilitiesKHR capabilities{};
    if (vkGetPhysicalDeviceSurfaceCapabilitiesKHR(physical_device, description.surface, &capabilities) == VK_SUCCESS &&
            (capabilities.supportedUsageFlags & VK_IMAGE_USAGE_TRANSFER_SRC_BIT)) description.imageUsage |= VK_IMAGE_USAGE_TRANSFER_SRC_BIT;
    graphics.output_width = description.imageExtent.width; graphics.output_height = description.imageExtent.height;
    for (uint32_t i = 0; i < 4; ++i) setup.queues[i] = queue_families[queue->queue_family][i < uint32_t(queue_families[queue->queue_family].size()) ? i : 0].queue;
    setup.queue_family = queue->queue_family; setup.create_info = info; setup.allocator = allocation_callbacks;
    setup.graphics_submit = rescaleframe_submit; setup.lock_queues = rescaleframe_lock_queues;
    setup.unlock_queues = rescaleframe_unlock_queues; setup.queue_user = chain;
    RSFGodot::vk_capture_setup(&setup);
    if (OS::get_singleton()->get_environment("RSF_GODOT_NATIVE_WSI") == "1" || !rescaleframe_timeline || queue_families[queue->queue_family].size() < 4) return false;
    uint64_t handle = 0;
    const bool created = RSFGodot::vk_create(&setup, &chain->rescaleframe_owner, &handle);
    if (!created) {
        if (chain->rescaleframe_owner) chain->vk_swapchain = reinterpret_cast<VkSwapchainKHR>(handle);
        return false;
    }
    chain->vk_swapchain = reinterpret_cast<VkSwapchainKHR>(handle); return true;
}
bool RenderingDeviceDriverVulkan::rescaleframe_release(void *opaque_chain) {
    auto *chain = static_cast<SwapChain *>(opaque_chain);
    if (!RSFGodot::vk_capture_release(chain)) return false;
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
    if (chain->rescaleframe_owner) return VkResult(RSFGodot::vk_images(chain->rescaleframe_owner, count, reinterpret_cast<uint64_t *>(images)));
    struct Lease { PFN_vkGetSwapchainImagesKHR call; VkDevice device; VkSwapchainKHR chain; uint32_t *count; VkImage *images; };
    Lease lease{device_functions.GetSwapchainImagesKHR, vk_device, chain->vk_swapchain, count, images};
    return VkResult(RSFGodot::vk_wsi_call([](void *user) -> int32_t {
        const auto &value = *static_cast<Lease *>(user); return value.call(value.device, value.chain, value.count, value.images);
    }, &lease));
}
VkResult RenderingDeviceDriverVulkan::rescaleframe_acquire(void *opaque_chain, uint64_t timeout, VkSemaphore semaphore, VkFence fence, uint32_t *index) {
    auto *chain = static_cast<SwapChain *>(opaque_chain);
    if (chain->rescaleframe_owner) return VkResult(RSFGodot::vk_acquire(chain->rescaleframe_owner, timeout, reinterpret_cast<uint64_t>(semaphore), reinterpret_cast<uint64_t>(fence), index));
    struct Lease { PFN_vkAcquireNextImageKHR call; VkDevice device; VkSwapchainKHR chain; uint64_t timeout; VkSemaphore semaphore; VkFence fence; uint32_t *index; };
    Lease lease{device_functions.AcquireNextImageKHR, vk_device, chain->vk_swapchain, timeout, semaphore, fence, index};
    return VkResult(RSFGodot::vk_wsi_call([](void *user) -> int32_t {
        const auto &value = *static_cast<Lease *>(user); return value.call(value.device, value.chain, value.timeout, value.semaphore, value.fence, value.index);
    }, &lease));
}
VkResult RenderingDeviceDriverVulkan::rescaleframe_present(CommandQueueID queue_id, const VkPresentInfoKHR *info, VectorView<SwapChainID> chains, bool submitted) {
    struct PresentScope {
        uint64_t window = 0;
        void admit(RenderingContextDriver::SurfaceID id) {
            if (window) return;
            const auto *surface = reinterpret_cast<const RenderingContextDriverVulkan::Surface *>(id);
            if (surface && RSFGodot::VulkanExecutionScope::present(surface->rescaleframe_window, true))
                window = surface->rescaleframe_window;
        }
        ~PresentScope() { if (window) RSFGodot::VulkanExecutionScope::present(window, false); }
    };
    auto *queue = reinterpret_cast<CommandQueue *>(queue_id.id);
    if (chains.size() == 1) {
        auto *chain = reinterpret_cast<SwapChain *>(chains[0].id);
        if (chain->rescaleframe_owner) {
            PresentScope scope;
            scope.admit(chain->surface);
            return VkResult(RSFGodot::vk_present(chain->rescaleframe_owner, info));
        }
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
        struct Context {
            VkQueue graphics;
            const VkPresentInfoKHR &info;
            const VectorView<SwapChainID> &chains;
            uint64_t native_present;
        } context{graphics, *info, chains, reinterpret_cast<uint64_t>(device_functions.QueuePresentKHR)};
        static constexpr RSFGodot::MixedPresentCalls calls{
            [](void *opaque) -> int32_t {
                const auto &batch = *static_cast<const Context *>(opaque);
                LocalVector<VkPipelineStageFlags> stages;
                stages.resize(batch.info.waitSemaphoreCount);
                for (uint32_t i = 0; i < stages.size(); ++i) stages[i] = VK_PIPELINE_STAGE_ALL_COMMANDS_BIT;
                LocalVector<VkSemaphore> signals;
                for (uint32_t i = 0; i < batch.chains.size(); ++i) {
                    const auto *chain = reinterpret_cast<SwapChain *>(batch.chains[i].id);
                    signals.push_back(chain->present_semaphores[chain->image_index]);
                }
                VkSubmitInfo submit{};
                submit.sType = VK_STRUCTURE_TYPE_SUBMIT_INFO;
                submit.waitSemaphoreCount = batch.info.waitSemaphoreCount;
                submit.pWaitSemaphores = batch.info.pWaitSemaphores;
                submit.pWaitDstStageMask = stages.ptr();
                submit.signalSemaphoreCount = signals.size();
                submit.pSignalSemaphores = signals.ptr();
                return vkQueueSubmit(batch.graphics, 1, &submit, VK_NULL_HANDLE);
            },
            [](void *opaque, uint32_t index) -> int32_t {
                const auto &batch = *static_cast<const Context *>(opaque);
                const auto *chain = reinterpret_cast<SwapChain *>(batch.chains[index].id);
                VkPresentInfoKHR one = batch.info;
                one.swapchainCount = one.waitSemaphoreCount = 1;
                one.pSwapchains = &chain->vk_swapchain;
                one.pImageIndices = &chain->image_index;
                one.pWaitSemaphores = &chain->present_semaphores[chain->image_index];
                one.pResults = batch.info.pResults ? &batch.info.pResults[index] : nullptr;
                PresentScope scope;
                scope.admit(chain->surface);
                const auto item = chain->rescaleframe_owner ? VkResult(RSFGodot::vk_present(chain->rescaleframe_owner, &one)) :
                    VkResult(RSFGodot::vk_native_present(batch.native_present, batch.graphics, &one));
                if (one.pResults) *one.pResults = item;
                return item;
            },
        };
        return VkResult(RSFGodot::rsf_godot_present_mixed(&calls, &context, chains.size(), info->pNext != nullptr, submitted));
    }
    PresentScope scope;
    for (uint32_t i = 0; i < chains.size(); ++i) {
        const auto *chain = reinterpret_cast<const SwapChain *>(chains[i].id);
        scope.admit(chain->surface);
    }
    return VkResult(RSFGodot::vk_native_present(reinterpret_cast<uint64_t>(device_functions.QueuePresentKHR), graphics, info));
}

void *RenderingDeviceDriverVulkan::rescaleframe_begin(CommandBufferID id) {
    auto *commands = reinterpret_cast<CommandBufferInfo *>(id.id);
    if (!commands || commands->active_render_pass) return nullptr;
    return commands->vk_command_buffer;
}
bool RenderingDeviceDriverVulkan::rescaleframe_graph_info(CommandBufferID id, uint32_t &family) {
    const auto *commands = reinterpret_cast<const CommandBufferInfo *>(id.id);
    if (!rescaleframe_timeline || !commands || commands->active_render_pass || commands->rescaleframe_timeline) return false;
    family = commands->rescaleframe_family;
    return true;
}
void RenderingDeviceDriverVulkan::rescaleframe_graph_split(CommandBufferID current, CommandBufferID continuation,
        uint64_t timeline, uint64_t previous_done, uint64_t ready, uint64_t done) {
    auto *before = reinterpret_cast<CommandBufferInfo *>(current.id);
    auto *after = reinterpret_cast<CommandBufferInfo *>(continuation.id);
    // A second split in a continuation needs another distinct external timeline.
    // Refuse rather than overwrite a dependency which the engine must submit.
    CRASH_COND_MSG(before->rescaleframe_timeline || after->rescaleframe_timeline ||
            before->rescaleframe_family != after->rescaleframe_family,
            "ReScaleFrame graph split has incompatible command-buffer dependencies.");
    const VkResult result = vkEndCommandBuffer(before->vk_command_buffer);
    CRASH_COND_MSG(result != VK_SUCCESS, "ReScaleFrame could not end its source graph command buffer.");
    before->rescaleframe_timeline = reinterpret_cast<VkSemaphore>(timeline);
    before->rescaleframe_wait = previous_done;
    before->rescaleframe_signal = ready;
    after->rescaleframe_timeline = reinterpret_cast<VkSemaphore>(timeline);
    after->rescaleframe_wait = done;
    after->rescaleframe_signal = 0;
}
uint64_t RenderingDeviceDriverVulkan::rescaleframe_instance_proc() {
    return reinterpret_cast<uint64_t>(vkGetInstanceProcAddr);
}
uint64_t RenderingDeviceDriverVulkan::rescaleframe_device_proc() {
    return reinterpret_cast<uint64_t>(vkGetDeviceProcAddr);
}
