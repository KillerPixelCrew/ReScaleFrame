// SPDX-License-Identifier: MIT
// The production WSI hooks borrow the native leaf from the Rust route. A hook
// inside its forwarded Vulkan call keeps the leaf, and the native close
// unregisters while Rust still holds the process-wide admission.
#include "native_ffx_vulkan_harness.h"

using namespace fake;

namespace route {
rsf_ffxgvk* owner = nullptr;
uint32_t presents = 0, creates = 0, destroys = 0, acquires = 0, enumerations = 0, lookups = 0, unregistered = 0;
bool destroy_inside = false;
int32_t destroyed_inside = RSF_BACKEND_OK, created_inside = RSF_BACKEND_OK;
rsf_ffxgvk_native* routed_inside = nullptr;

int32_t refused_second_owner() {
    Setup setup;
    const auto table = ops();
    rsf_ffxgvk* second = nullptr; uint64_t chain = 0;
    const auto result = rsf_core_ffxgvk_create(&table, &setup.value, &setup.swapchain, &second, &chain);
    require(!second && !chain, "a refused owner returns no handle");
    return result;
}
VkResult VKAPI_PTR present(VkQueue queue, const VkPresentInfoKHR*) {
    require(queue == id<VkQueue>(22), "the hook forwards the SDK's present queue");
    ++presents;
    if (destroy_inside) {
        // The host destroys the owner while this hook is inside its forwarded call.
        destroyed_inside = rsf_ffxgvk_destroy(owner);
        routed_inside = routed();
        created_inside = refused_second_owner();
    }
    return VK_SUBOPTIMAL_KHR;
}
VkResult VKAPI_PTR create(VkDevice, const VkSwapchainCreateInfoKHR*, const VkAllocationCallbacks*, VkSwapchainKHR* chain) {
    *chain = id<VkSwapchainKHR>(40); ++creates; return VK_SUCCESS;
}
void VKAPI_PTR destroy(VkDevice, VkSwapchainKHR chain, const VkAllocationCallbacks*) {
    require(chain == id<VkSwapchainKHR>(40), "the hook forwards the destroyed chain"); ++destroys;
}
VkResult VKAPI_PTR acquire(VkDevice, VkSwapchainKHR, uint64_t, VkSemaphore, VkFence, uint32_t* index) {
    *index = 2; ++acquires; return VK_SUCCESS;
}
VkResult VKAPI_PTR images(VkDevice, VkSwapchainKHR, uint32_t* count, VkImage*) { *count = 3; ++enumerations; return VK_INCOMPLETE; }
rsf_vk_function lookup(void*, const char* name) {
    ++lookups;
    return std::strcmp(name, "vkBindImageMemory2") == 0 ? reinterpret_cast<rsf_vk_function>(&images) : nullptr;
}
int32_t wsi(const rsf_ffx_wsi_dispatch* setup) {
    require(setup && setup->struct_size == sizeof(*setup) && setup->abi_version == 2 && setup->device == id<VkDevice>(1) &&
        !setup->create && !setup->destroy && !setup->present && !setup->acquire && !setup->images, "close clears this device's dispatch");
    require(!routed(), "the route lends no leaf while the native close unregisters");
    require(refused_second_owner() == RSF_BACKEND_ERROR_NOT_SUPPORTED, "admission is held through unregistration");
    ++unregistered; return 0;
}
}

int main() {
    Setup setup;
    auto table = ops();
    route::owner = start(table, setup);
    auto* native = state.native;
    require(routed() == native, "Rust publishes the opened leaf to the WSI route");
    native->native_present = route::present; native->native_create = route::create; native->native_destroy = route::destroy;
    native->native_acquire = route::acquire; native->native_images = route::images; native->setup.graphics.device_proc = route::lookup;
    native->wsi_dispatch = route::wsi; native->wsi_registered = true;

    const auto device = id<VkDevice>(1), foreign = id<VkDevice>(2);
    VkSwapchainKHR chain = VK_NULL_HANDLE; uint32_t index = 0, count = 0;
    require(serialized_create(foreign, nullptr, nullptr, &chain) == VK_ERROR_INITIALIZATION_FAILED &&
        serialized_acquire(foreign, chain, 0, VK_NULL_HANDLE, VK_NULL_HANDLE, &index) == VK_ERROR_INITIALIZATION_FAILED &&
        serialized_images(foreign, chain, &count, nullptr) == VK_ERROR_INITIALIZATION_FAILED && !device_proc(foreign, "vkBindImageMemory2KHR"),
        "a foreign device is not routed");
    serialized_destroy(foreign, id<VkSwapchainKHR>(40), nullptr);
    require(!route::creates && !route::destroys && !route::acquires && !route::enumerations && !route::lookups, "no foreign call was forwarded");
    require(serialized_create(device, nullptr, nullptr, &chain) == VK_SUCCESS && chain == id<VkSwapchainKHR>(40) &&
        serialized_acquire(device, chain, 0, VK_NULL_HANDLE, VK_NULL_HANDLE, &index) == VK_SUCCESS && index == 2 &&
        serialized_images(device, chain, &count, nullptr) == VK_INCOMPLETE && count == 3,
        "device hooks forward their Vulkan results unchanged");
    require(device_proc(device, "vkBindImageMemory2KHR") == reinterpret_cast<PFN_vkVoidFunction>(&route::images) && route::lookups == 2,
        "the backend lookup falls back to the core alias");
    const auto submits = state.submits;
    require(graphics_submit(0, nullptr, id<VkFence>(77)) == VK_SUCCESS && state.submits == submits + 1, "the game-queue submit reaches the host");

    VkPresentInfoKHR info{VK_STRUCTURE_TYPE_PRESENT_INFO_KHR};
    require(serialized_present(id<VkQueue>(22), &info) == VK_SUBOPTIMAL_KHR && route::presents == 1, "present forwards its result");

    route::destroy_inside = true;
    require(serialized_present(id<VkQueue>(22), &info) == VK_SUBOPTIMAL_KHR && route::presents == 2, "the borrowed present still returns its result");
    require(route::destroyed_inside == RSF_BACKEND_ERROR_NOT_READY && !route::routed_inside &&
        route::created_inside == RSF_BACKEND_ERROR_NOT_SUPPORTED && !route::unregistered,
        "a hook inside its callback keeps the leaf, its registration and the admission");
    require(state.effects_destroyed == 1 && state.chains_destroyed == 1 && state.swapchains_destroyed == 1,
        "the SDK contexts were released before the route refused the close");
    route::destroy_inside = false;
    require(serialized_present(id<VkQueue>(22), &info) == VK_ERROR_INITIALIZATION_FAILED &&
        graphics_submit(0, nullptr, id<VkFence>(77)) == VK_ERROR_DEVICE_LOST && route::presents == 2,
        "a closing route forwards no new callback");

    require(rsf_ffxgvk_destroy(route::owner) == RSF_BACKEND_OK && route::unregistered == 1 && !routed(),
        "the retry closes the leaf once, after its last borrow");
    require(state.effects_destroyed == 1 && state.chains_destroyed == 1 && state.swapchains_destroyed == 1,
        "the retry repeats no SDK release");
    require(serialized_present(id<VkQueue>(22), &info) == VK_ERROR_INITIALIZATION_FAILED && !device_proc(device, "vkBindImageMemory2KHR"),
        "a late hook after retirement touches no freed leaf");

    auto* successor = start(table, setup);
    require(routed() == state.native && rsf_ffxgvk_destroy(successor) == RSF_BACKEND_OK && !routed(), "the released route admits a successor");
    std::puts("PASS: FFX WSI hooks borrow the Rust route; a live borrow defers the native close and admission");
    return 0;
}
