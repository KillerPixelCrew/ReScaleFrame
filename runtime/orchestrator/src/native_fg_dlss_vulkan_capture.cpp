// SPDX-License-Identifier: MIT
#include "native_fg_dlss_vulkan_capture.h"
#include "native_fg_dlss_vulkan_capture_ledger.h"
#include <cstring>

#if defined(RSF_HAVE_STREAMLINE_VK)
#define VK_NO_PROTOTYPES
#include <windows.h>
#include <vulkan/vulkan.h>
#include "../../backends/common/frame_capture_vk.h"
#include <memory>
#include <cstddef>
#include <filesystem>
#include <fstream>
#include <string>
#include <vector>

extern "C" {
struct rsf_vk_capture_commands;
rsf_vk_capture_commands* rsf_vk_capture_commands_open(const rsf_srvk_setup*);
void rsf_vk_capture_commands_close(rsf_vk_capture_commands*);
int32_t rsf_vk_capture_create_pool(const rsf_vk_capture_commands*, uint32_t family, uint64_t*);
void rsf_vk_capture_destroy_pool(const rsf_vk_capture_commands*, uint64_t pool);
int32_t rsf_vk_capture_allocate(const rsf_vk_capture_commands*, uint64_t pool, void**);
int32_t rsf_vk_capture_reset_pool(const rsf_vk_capture_commands*, uint64_t pool);
int32_t rsf_vk_capture_begin(const rsf_vk_capture_commands*, void* command);
int32_t rsf_vk_capture_end(const rsf_vk_capture_commands*, void* command);
int32_t rsf_vk_capture_create_fence(const rsf_vk_capture_commands*, uint64_t*);
void rsf_vk_capture_destroy_fence(const rsf_vk_capture_commands*, uint64_t fence);
int32_t rsf_vk_capture_reset_fence(const rsf_vk_capture_commands*, uint64_t fence);
int32_t rsf_vk_capture_wait_fence(const rsf_vk_capture_commands*, uint64_t fence, uint64_t timeout);
}

namespace rsf {
namespace {
// Typed Vulkan, WSI, Streamline registration and file leaves. The Rust owner
// in dlss_vulkan_capture.rs decides admission, command and fence lifetime,
// submission order and retirement.
using CapturePacket = rsf_sl_present_capture_packet;
static_assert(sizeof(CapturePacket) == 96 && offsetof(CapturePacket, source_frame) == 48 &&
    offsetof(CapturePacket, queue_family) == 68 && offsetof(CapturePacket, native_wsi_call) == 88);
static_assert(sizeof(rsf_dlss_capture_platform) == 24 * sizeof(void*));
static_assert(sizeof(rsf_dlss_capture_target) == 40 && alignof(rsf_dlss_capture_target) == 8 &&
    offsetof(rsf_dlss_capture_target, index) == 8 && offsetof(rsf_dlss_capture_target, height) == 32);
static_assert(sizeof(VkImage) == sizeof(uint64_t) && alignof(VkImage) == alignof(uint64_t));
static_assert(VK_IMAGE_USAGE_TRANSFER_SRC_BIT == 1 && VK_FORMAT_R8G8B8A8_UNORM == 37 &&
    VK_FORMAT_B8G8R8A8_UNORM == 44 && VK_IMAGE_LAYOUT_PRESENT_SRC_KHR == 1000001002 &&
    VK_ERROR_FORMAT_NOT_SUPPORTED == -11);

struct CaptureVk {
    rsf_fgvk_setup setup{};
    rsf_sl_set_present_capture registration = nullptr;
    std::unique_ptr<rsf_core_capture_vk, decltype(&rsf_core_capture_vk_destroy)> capture{
        rsf_core_capture_vk_create(RSF_CAPTURE_VK_SHARED_SUBMISSION), rsf_core_capture_vk_destroy};
    std::filesystem::path directory;
    rsf_vk_capture_commands* commands = nullptr;
    ~CaptureVk() { rsf_vk_capture_commands_close(commands); }
};
CaptureVk& capture_vk(void* native) { return *static_cast<CaptureVk*>(native); }
const VkPresentInfoKHR& present_info(const CapturePacket& packet) { return *static_cast<const VkPresentInfoKHR*>(packet.present_info); }

// Both calls bypass the SDK proxy under its LL2 WSI lease.
using WsiCall = int32_t (*)(int32_t (*)(void*), void*);
int32_t wsi_present(const CapturePacket& packet, const VkPresentInfoKHR* info) {
    struct Call { PFN_vkQueuePresentKHR next; VkQueue queue; const VkPresentInfoKHR* info; };
    Call call{reinterpret_cast<PFN_vkQueuePresentKHR>(packet.native_present), static_cast<VkQueue>(packet.queue), info};
    return reinterpret_cast<WsiCall>(packet.native_wsi_call)([](void* user) -> int32_t {
        const auto& item = *static_cast<Call*>(user); return item.next(item.queue, item.info); }, &call);
}
int32_t wsi_images(const CapturePacket* packet, uint64_t chain, uint32_t* count, uint64_t* output) noexcept try {
    struct Call { PFN_vkGetSwapchainImagesKHR next; VkDevice device; VkSwapchainKHR chain; uint32_t* count; VkImage* images; };
    Call call{reinterpret_cast<PFN_vkGetSwapchainImagesKHR>(packet->native_images), static_cast<VkDevice>(packet->device),
        reinterpret_cast<VkSwapchainKHR>(chain), count, reinterpret_cast<VkImage*>(output)};
    return reinterpret_cast<WsiCall>(packet->native_wsi_call)([](void* user) -> int32_t {
        const auto& item = *static_cast<Call*>(user); return item.next(item.device, item.chain, item.count, item.images); }, &call);
} catch (...) { return VK_ERROR_INITIALIZATION_FAILED; }

rsf_backend_result open_capture(const rsf_fgvk_setup* setup, void** out) noexcept try {
    *out = nullptr;
    auto next = std::make_unique<CaptureVk>();
    if (!rsf_core_capture_vk_enabled(next->capture.get())) return RSF_BACKEND_OK;
    next->setup = *setup;
    wchar_t directory[32768]{};
    const auto length = GetEnvironmentVariableW(L"RSF_VULKAN_FRAME_CAPTURE_DIR", directory, 32768);
    if (!length || length >= 32768 || !setup->graphics.runtime_directory_utf8) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    next->directory = directory;
    std::u8string runtime_directory(std::strlen(setup->graphics.runtime_directory_utf8), u8'\0');
    std::memcpy(runtime_directory.data(), setup->graphics.runtime_directory_utf8, runtime_directory.size());
    const auto common = std::filesystem::path(runtime_directory) / L"sl.common.dll";
    const auto module = GetModuleHandleW(common.c_str());
    if (!module) return RSF_BACKEND_ERROR_NOT_READY;
    next->registration = reinterpret_cast<rsf_sl_set_present_capture>(GetProcAddress(module, "rsfSlSetPresentCapture"));
    if (!next->registration) return RSF_BACKEND_ERROR_MISSING_ENTRY_POINT;
    const auto& graphics = setup->graphics;
    next->commands = rsf_vk_capture_commands_open(&graphics);
    if (!next->commands) return RSF_BACKEND_ERROR_MISSING_ENTRY_POINT;
    const auto properties = reinterpret_cast<PFN_vkGetPhysicalDeviceMemoryProperties>(graphics.instance_proc(graphics.instance, "vkGetPhysicalDeviceMemoryProperties"));
    if (!properties) return RSF_BACKEND_ERROR_MISSING_ENTRY_POINT;
    VkPhysicalDeviceMemoryProperties memory{}; properties(static_cast<VkPhysicalDevice>(graphics.physical_device), &memory);
    rsf_core_capture_vk_provider(next->capture.get(), 0, 1, 1);
    if (!rsf_core_capture_vk_initialize(next->capture.get(), graphics.device, graphics.device_proc, &memory))
        return RSF_BACKEND_ERROR_INIT_FAILED;
    *out = next.release();
    return RSF_BACKEND_OK;
} catch (...) { return RSF_BACKEND_ERROR_INIT_FAILED; }

int32_t set_callback(void* native, rsf_sl_present_capture_callback callback, void* user) noexcept {
    const auto& self = capture_vk(native);
    const rsf_sl_present_capture_registration registration{sizeof(registration), RSF_SL_PRESENT_CAPTURE_ABI_VERSION,
        self.setup.graphics.device, callback, user};
    return self.registration(&registration);
}

void target(const CapturePacket* packet, rsf_dlss_capture_target* out) noexcept {
    const auto& info = present_info(*packet);
    const auto& chain = *static_cast<const VkSwapchainCreateInfoKHR*>(packet->swapchain_info);
    *out = {0, 0, 0, chain.imageUsage, chain.imageArrayLayers, uint32_t(chain.imageFormat),
        chain.imageExtent.width, chain.imageExtent.height};
    if (info.swapchainCount == 1 && info.pSwapchains && info.pImageIndices) {
        out->chain = reinterpret_cast<uint64_t>(info.pSwapchains[0]);
        out->index = info.pImageIndices[0];
        out->selected = 1;
    }
}

uint32_t record(void* native, void* commands, const rsf_srvk_image* image, const rsf_frame_record* identity, const char* role) noexcept try {
    return rsf_core_capture_vk_record(capture_vk(native).capture.get(), commands, image,
        identity, role, identity->flags & RSF_FRAME_FLAG_RESET, 0, nullptr, reinterpret_cast<uint64_t>(commands));
} catch (...) { return 0; }

int32_t submit(const CapturePacket* packet, void* commands, uint64_t fence) noexcept try {
    const auto& info = present_info(*packet);
    std::vector<VkPipelineStageFlags> stages(info.waitSemaphoreCount, VK_PIPELINE_STAGE_ALL_COMMANDS_BIT);
    const auto buffer = static_cast<VkCommandBuffer>(commands);
    VkSubmitInfo submission{}; submission.sType = VK_STRUCTURE_TYPE_SUBMIT_INFO;
    submission.waitSemaphoreCount = info.waitSemaphoreCount; submission.pWaitSemaphores = info.pWaitSemaphores;
    submission.pWaitDstStageMask = stages.data(); submission.commandBufferCount = 1; submission.pCommandBuffers = &buffer;
    return reinterpret_cast<PFN_vkQueueSubmit>(packet->native_submit)(static_cast<VkQueue>(packet->queue), 1, &submission,
        reinterpret_cast<VkFence>(fence));
} catch (...) { return VK_ERROR_OUT_OF_HOST_MEMORY; }

int32_t present(const CapturePacket* packet, uint32_t waits) noexcept {
    const auto& info = present_info(*packet);
    if (waits) return wsi_present(*packet, &info);
    VkPresentInfoKHR completed = info; completed.waitSemaphoreCount = 0; completed.pWaitSemaphores = nullptr;
    return wsi_present(*packet, &completed);
}

void failure(void* native, uint64_t source, const char* reason, int32_t result) noexcept try {
    std::ofstream(capture_vk(native).directory / "dlss-present-capture-errors.log", std::ios::app) << source << ' ' << reason << ' ' << result << '\n';
} catch (...) {}

void append(void* native, const char* file, const uint8_t* line, size_t length) noexcept try {
    std::ofstream stream(capture_vk(native).directory / file, std::ios::app);
    stream.write(reinterpret_cast<const char*>(line), static_cast<std::streamsize>(length));
} catch (...) {}

const rsf_dlss_capture_platform capture_platform{
    open_capture,
    [](void* native) { delete static_cast<CaptureVk*>(native); },
    set_callback,
    [](void* native, uint32_t family, uint64_t* out) { return rsf_vk_capture_create_pool(capture_vk(native).commands, family, out); },
    [](void* native, uint64_t pool) { rsf_vk_capture_destroy_pool(capture_vk(native).commands, pool); },
    [](void* native, uint64_t pool, void** out) { return rsf_vk_capture_allocate(capture_vk(native).commands, pool, out); },
    [](void* native, uint64_t pool) { return rsf_vk_capture_reset_pool(capture_vk(native).commands, pool); },
    [](void* native, void* command) { return rsf_vk_capture_begin(capture_vk(native).commands, command); },
    [](void* native, void* command) { return rsf_vk_capture_end(capture_vk(native).commands, command); },
    [](void* native, uint64_t* out) { return rsf_vk_capture_create_fence(capture_vk(native).commands, out); },
    [](void* native, uint64_t fence) { rsf_vk_capture_destroy_fence(capture_vk(native).commands, fence); },
    [](void* native, uint64_t fence) { return rsf_vk_capture_reset_fence(capture_vk(native).commands, fence); },
    [](void* native, uint64_t fence, uint64_t timeout) { return rsf_vk_capture_wait_fence(capture_vk(native).commands, fence, timeout); },
    [](void* native, void* commands) { rsf_core_capture_vk_complete_group(capture_vk(native).capture.get(), reinterpret_cast<uint64_t>(commands)); },
    [](void* native, uint64_t frame) -> uint32_t { return rsf_core_capture_vk_wants(capture_vk(native).capture.get(), frame); },
    [](void* native) noexcept -> uint64_t {
        try { return rsf_core_capture_vk_pending(capture_vk(native).capture.get()); } catch (...) { return UINT64_MAX; }
    },
    target, wsi_images, record, submit, present,
    [](void* native) -> uint32_t { return rsf_core_capture_vk_flush(capture_vk(native).capture.get(), 0, nullptr); },
    failure, append,
};
}

DlssPresentCapture::~DlssPresentCapture() { rsf_core_dlss_capture_close(owner); }
rsf_backend_result DlssPresentCapture::initialize(const rsf_fgvk_setup& setup) {
    return rsf_core_dlss_capture_open(&capture_platform, &setup, &owner);
}
void DlssPresentCapture::remember(const rsf_frame_record& record, uint32_t count, const rsf_dlssgvk_status& sdk, bool configured) {
    rsf_core_dlss_capture_remember(owner, &record, count, &sdk, configured ? 1u : 0u);
}
void DlssPresentCapture::flush() { rsf_core_dlss_capture_flush(owner); }
rsf_backend_result DlssPresentCapture::detach() { return rsf_core_dlss_capture_detach(owner); }
}
#else
namespace rsf {
DlssPresentCapture::~DlssPresentCapture() = default;
rsf_backend_result DlssPresentCapture::initialize(const rsf_fgvk_setup&) { return RSF_BACKEND_OK; }
void DlssPresentCapture::remember(const rsf_frame_record&, uint32_t, const rsf_dlssgvk_status&, bool) {}
void DlssPresentCapture::flush() {}
rsf_backend_result DlssPresentCapture::detach() { return RSF_BACKEND_OK; }
}
#endif
