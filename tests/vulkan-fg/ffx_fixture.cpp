// SPDX-License-Identifier: MIT
// The production WSI hooks borrow the native leaf from the Rust route. A hook
// inside its forwarded Vulkan call keeps the leaf. Rust unregisters
// before closing the native leaf and releasing process-wide admission.
#include "../native_ffx_vulkan_harness.h"

using namespace fake;

namespace route {
rsf_ffxgvk* owner = nullptr;
rsf_wsi_route_fixture_state fixture{};

int32_t refused_second_owner() {
    Setup setup;
    const auto table = ops();
    rsf_ffxgvk* second = nullptr; uint64_t chain = 0;
    const auto result = rsf_core_ffxgvk_create_native(&table, &setup.value, &second, &chain);
    require(!second && !chain, "a refused owner returns no handle");
    return result;
}
VkResult VKAPI_PTR present(VkQueue queue, const VkPresentInfoKHR*) {
    require(queue == id<VkQueue>(22), "the hook forwards the SDK's present queue");
    ++fixture.presents;
    if (fixture.destroy_inside) {
        // The host destroys the owner while this hook is inside its forwarded call.
        fixture.destroyed_inside = rsf_ffxgvk_destroy(owner);
        fixture.routed_inside = routed();
        fixture.created_inside = refused_second_owner();
    }
    return VK_SUBOPTIMAL_KHR;
}
VkResult VKAPI_PTR create(VkDevice, const VkSwapchainCreateInfoKHR*, const VkAllocationCallbacks*, VkSwapchainKHR* chain) {
    *chain = id<VkSwapchainKHR>(40); ++fixture.creates; return VK_SUCCESS;
}
void VKAPI_PTR destroy(VkDevice, VkSwapchainKHR chain, const VkAllocationCallbacks*) {
    require(chain == id<VkSwapchainKHR>(40), "the hook forwards the destroyed chain"); ++fixture.destroys;
}
VkResult VKAPI_PTR acquire(VkDevice, VkSwapchainKHR, uint64_t, VkSemaphore, VkFence, uint32_t* index) {
    *index = 2; ++fixture.acquires; return VK_SUCCESS;
}
VkResult VKAPI_PTR images(VkDevice, VkSwapchainKHR, uint32_t* count, VkImage*) { *count = 3; ++fixture.enumerations; return VK_INCOMPLETE; }
rsf_vk_function lookup(void*, const char* name) {
    ++fixture.lookups;
    return std::strcmp(name, "vkBindImageMemory2") == 0 ? reinterpret_cast<rsf_vk_function>(&images) : nullptr;
}
int32_t wsi(const rsf_ffx_wsi_dispatch* setup) {
    require(setup && setup->struct_size == sizeof(*setup) && setup->abi_version == 2 && setup->device == id<VkDevice>(1),
        "the dispatch uses this device and the corrected SDK ABI");
    if (setup->create) {
        require(setup->create == serialized_create && setup->destroy == serialized_destroy &&
            setup->present == serialized_present && setup->acquire == serialized_acquire && setup->images == serialized_images,
            "registration publishes the typed host hooks");
        require(!routed(), "the route publishes its leaf only after registration succeeds");
        ++fixture.registered;
        return 0;
    }
    require(!setup->destroy && !setup->present && !setup->acquire && !setup->images, "teardown clears every hook");
    require(!routed(), "the route lends no leaf during unregistration");
    require(refused_second_owner() == RSF_BACKEND_ERROR_NOT_SUPPORTED, "admission is held through unregistration");
    if (fixture.refuse_unregister) return -1;
    ++fixture.unregistered; return 0;
}

int32_t open(const rsf_fgvk_setup* setup, void* module, uint32_t width, uint32_t height, rsf_ffxgvk_native** out, void** resources) {
    const auto result = fake::open(setup, module, width, height, out, resources);
    if (result != RSF_BACKEND_OK) return result;
    auto* native = *out;
    native->native_present = present; native->native_create = create; native->native_destroy = destroy;
    native->native_acquire = acquire; native->native_images = images; native->setup.graphics.device_proc = lookup;
    native->wsi_dispatch = wsi;
    return RSF_BACKEND_OK;
}
}


extern "C" rsf_ffx_fixture_state* rsf_fixture_ffx_state() { return &state; }
extern "C" rsf_wsi_route_fixture_state* rsf_fixture_wsi_route_state() { return &route::fixture; }
extern "C" int32_t rsf_fixture_ffx_start(bool routed_fixture, rsf_ffxgvk** out, uint64_t* chain) {
    Setup setup;
    static const auto ordinary = ops();
    static const auto routed_ops = [] {
        auto table = ops();
        table.open = route::open;
        table.set_wsi = set_wsi;
        return table;
    }();
    const auto result = rsf_core_ffxgvk_create_native(routed_fixture ? &routed_ops : &ordinary, &setup.value, out, chain);
    if (routed_fixture && result == RSF_BACKEND_OK) route::owner = *out;
    return result;
}
extern "C" void* rsf_fixture_ffx_native() { return state.native; }
extern "C" void* rsf_fixture_ffx_routed() { return routed(); }
extern "C" uint64_t rsf_fixture_ffx_chain() { return handle_value(state.native->chain); }
extern "C" void* rsf_fixture_ffx_gpu() { return state.native->gpu.get(); }
extern "C" const rsf_vkfg_platform* rsf_fixture_ffx_platform() { return &rsf::vkfg::platform; }
extern "C" int32_t rsf_fixture_ffx_present(rsf_ffxgvk* owner, uint64_t chain, const rsf_frame_record* frame, bool explicit_source) {
    const auto swapchain = id<VkSwapchainKHR>(chain);
    VkPresentInfoKHR info{VK_STRUCTURE_TYPE_PRESENT_INFO_KHR};
    info.swapchainCount = 1;
    info.pSwapchains = &swapchain;
    return explicit_source ? rsf_ffxgvk_present_source(owner, &info, frame) : rsf_ffxgvk_present(owner, &info);
}
extern "C" int32_t rsf_fixture_wsi_create(uint64_t device, uint64_t* out) {
    VkSwapchainKHR chain{};
    const auto result = serialized_create(id<VkDevice>(device), nullptr, nullptr, &chain);
    if (result == VK_SUCCESS) *out = handle_value(chain);
    return result;
}
extern "C" void rsf_fixture_wsi_destroy(uint64_t device, uint64_t chain) {
    serialized_destroy(id<VkDevice>(device), id<VkSwapchainKHR>(chain), nullptr);
}
extern "C" int32_t rsf_fixture_wsi_acquire(uint64_t device, uint64_t chain, uint32_t* out) {
    return serialized_acquire(id<VkDevice>(device), id<VkSwapchainKHR>(chain), 0, VK_NULL_HANDLE, VK_NULL_HANDLE, out);
}
extern "C" int32_t rsf_fixture_wsi_images(uint64_t device, uint64_t chain, uint32_t* count) {
    return serialized_images(id<VkDevice>(device), id<VkSwapchainKHR>(chain), count, nullptr);
}
extern "C" int32_t rsf_fixture_wsi_present(uint64_t queue) {
    const VkPresentInfoKHR info{VK_STRUCTURE_TYPE_PRESENT_INFO_KHR};
    return serialized_present(id<VkQueue>(queue), &info);
}
extern "C" int32_t rsf_fixture_wsi_device_proc(uint64_t device) {
    const auto proc = device_proc(id<VkDevice>(device), "vkBindImageMemory2KHR");
    if (!proc) return 0;
    return proc == reinterpret_cast<PFN_vkVoidFunction>(&route::images) ? 1 : -1;
}
extern "C" int32_t rsf_fixture_wsi_graphics_submit(uint64_t fence) {
    return graphics_submit(0, nullptr, id<VkFence>(fence));
}
int main(int argc, char** argv) {
    if (argc != 2) return 1;
    SetEnvironmentVariableW(L"RSF_VULKAN_FRAME_CAPTURE_DIR", nullptr);
    SetEnvironmentVariableW(L"RSF_VULKAN_TIMING_FILE", nullptr);
    return rsf_fixture_ffx_run(uint32_t(std::strtoul(argv[1], nullptr, 10)));
}
