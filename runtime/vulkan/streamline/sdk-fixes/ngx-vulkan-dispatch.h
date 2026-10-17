#pragma once
#include <mutex>
#include <unordered_map>
#include <vector>
#include <cstring>

// NGX receives these dispatch functions at initialization. They preserve the
// loader/layer chain, while fixing the SDK's D3D-style initial-state assumption
// and ordering its private transfer commands after its compute transitions.
namespace sl::chi::ngxDispatch {
inline VkDevice device{};
inline PFN_vkGetDeviceProcAddr nextDevice{};
inline PFN_vkGetInstanceProcAddr nextInstance{};
inline PFN_vkCreateImage createImage{};
inline PFN_vkDestroyImage destroyImage{};
inline PFN_vkCmdPipelineBarrier barrier{};
inline PFN_vkCmdPipelineBarrier2 barrier2{};
inline PFN_vkCmdCopyImage copyImage{};
inline PFN_vkCmdClearColorImage clearImage{};
struct FreshImage { uint32_t mips, layers; VkImageAspectFlags aspects; std::vector<VkImageAspectFlags> initialized; };
inline std::mutex freshMutex;
inline std::unordered_map<VkImage, FreshImage> fresh;

inline VkImageAspectFlags aspects(VkFormat format) {
    switch (format) {
    case VK_FORMAT_D16_UNORM: case VK_FORMAT_X8_D24_UNORM_PACK32: case VK_FORMAT_D32_SFLOAT:
        return VK_IMAGE_ASPECT_DEPTH_BIT;
    case VK_FORMAT_S8_UINT: return VK_IMAGE_ASPECT_STENCIL_BIT;
    case VK_FORMAT_D16_UNORM_S8_UINT: case VK_FORMAT_D24_UNORM_S8_UINT: case VK_FORMAT_D32_SFLOAT_S8_UINT:
        return VK_IMAGE_ASPECT_DEPTH_BIT | VK_IMAGE_ASPECT_STENCIL_BIT;
    default: return VK_IMAGE_ASPECT_COLOR_BIT;
    }
}
inline VkResult VKAPI_CALL CreateImage(VkDevice dev, const VkImageCreateInfo* info,
    const VkAllocationCallbacks* allocator, VkImage* image) {
    VkResult result = createImage(dev, info, allocator, image);
    if (result == VK_SUCCESS && info->initialLayout == VK_IMAGE_LAYOUT_UNDEFINED) {
        std::lock_guard lock(freshMutex);
        fresh[*image] = {info->mipLevels, info->arrayLayers, aspects(info->format),
            std::vector<VkImageAspectFlags>(size_t(info->mipLevels) * info->arrayLayers)};
    }
    return result;
}
inline void VKAPI_CALL DestroyImage(VkDevice dev, VkImage image, const VkAllocationCallbacks* allocator) {
    { std::lock_guard lock(freshMutex); fresh.erase(image); }
    destroyImage(dev, image, allocator);
}
template<class T> inline std::vector<T> initializedBarriers(uint32_t count, const T* images) {
    std::vector<T> result;
    std::lock_guard lock(freshMutex);
    for (uint32_t i = 0; i < count; ++i) {
        auto found = fresh.find(images[i].image);
        if (found == fresh.end()) { result.push_back(images[i]); continue; }
        auto& state = found->second;
        const auto& range = images[i].subresourceRange;
        uint32_t mips = range.levelCount == VK_REMAINING_MIP_LEVELS ? state.mips - range.baseMipLevel : range.levelCount;
        uint32_t layers = range.layerCount == VK_REMAINING_ARRAY_LAYERS ? state.layers - range.baseArrayLayer : range.layerCount;
        if (range.baseMipLevel >= state.mips || mips > state.mips - range.baseMipLevel ||
            range.baseArrayLayer >= state.layers || layers > state.layers - range.baseArrayLayer) {
            result.push_back(images[i]); continue;
        }
        for (uint32_t layer = range.baseArrayLayer; layer < range.baseArrayLayer + layers; ++layer)
            for (uint32_t mip = range.baseMipLevel; mip < range.baseMipLevel + mips; ++mip) {
                auto& used = state.initialized[size_t(layer) * state.mips + mip];
                VkImageAspectFlags newAspects = range.aspectMask & ~used;
                VkImageAspectFlags oldAspects = range.aspectMask & used;
                for (VkImageAspectFlags subset : {newAspects, oldAspects}) {
                    if (!subset) continue;
                    T part = images[i];
                    part.subresourceRange = {subset, mip, 1, layer, 1};
                    if (subset == newAspects) { part.oldLayout = VK_IMAGE_LAYOUT_UNDEFINED; part.srcAccessMask = 0; }
                    result.push_back(part);
                }
                used |= range.aspectMask;
            }
        bool complete = true;
        for (auto used : state.initialized) complete &= (used & state.aspects) == state.aspects;
        if (complete) fresh.erase(found);
    }
    return result;
}
inline void VKAPI_CALL Barrier(VkCommandBuffer cmd, VkPipelineStageFlags source, VkPipelineStageFlags destination,
    VkDependencyFlags flags, uint32_t memoryCount, const VkMemoryBarrier* memory,
    uint32_t bufferCount, const VkBufferMemoryBarrier* buffers, uint32_t imageCount, const VkImageMemoryBarrier* images) {
    auto initialized = initializedBarriers(imageCount, images);
    barrier(cmd, source, destination, flags, memoryCount, memory, bufferCount, buffers,
        static_cast<uint32_t>(initialized.size()), initialized.data());
}
inline void VKAPI_CALL Barrier2(VkCommandBuffer cmd, const VkDependencyInfo* info) {
    auto initialized = initializedBarriers(info->imageMemoryBarrierCount, info->pImageMemoryBarriers);
    auto patched = *info;
    patched.imageMemoryBarrierCount = static_cast<uint32_t>(initialized.size());
    patched.pImageMemoryBarriers = initialized.data();
    barrier2(cmd, &patched);
}
inline void transferDependency(VkCommandBuffer cmd, bool before) {
    VkMemoryBarrier dependency{VK_STRUCTURE_TYPE_MEMORY_BARRIER, nullptr,
        static_cast<VkAccessFlags>(before ? VK_ACCESS_MEMORY_READ_BIT | VK_ACCESS_MEMORY_WRITE_BIT : VK_ACCESS_TRANSFER_READ_BIT | VK_ACCESS_TRANSFER_WRITE_BIT),
        static_cast<VkAccessFlags>(before ? VK_ACCESS_TRANSFER_READ_BIT | VK_ACCESS_TRANSFER_WRITE_BIT : VK_ACCESS_MEMORY_READ_BIT | VK_ACCESS_MEMORY_WRITE_BIT)};
    barrier(cmd, before ? VK_PIPELINE_STAGE_ALL_COMMANDS_BIT : VK_PIPELINE_STAGE_TRANSFER_BIT,
        before ? VK_PIPELINE_STAGE_TRANSFER_BIT : VK_PIPELINE_STAGE_ALL_COMMANDS_BIT,
        0, 1, &dependency, 0, nullptr, 0, nullptr);
}
inline void initializeCopy(VkCommandBuffer cmd, VkImage image, VkImageLayout layout, const VkImageSubresourceRange& range) {
    { std::lock_guard lock(freshMutex); if (!fresh.count(image)) return; }
    VkImageMemoryBarrier initial{VK_STRUCTURE_TYPE_IMAGE_MEMORY_BARRIER};
    initial.srcAccessMask = VK_ACCESS_MEMORY_READ_BIT | VK_ACCESS_MEMORY_WRITE_BIT;
    initial.dstAccessMask = VK_ACCESS_TRANSFER_READ_BIT | VK_ACCESS_TRANSFER_WRITE_BIT;
    initial.oldLayout = initial.newLayout = layout;
    initial.srcQueueFamilyIndex = initial.dstQueueFamilyIndex = VK_QUEUE_FAMILY_IGNORED;
    initial.image = image; initial.subresourceRange = range;
    Barrier(cmd, VK_PIPELINE_STAGE_ALL_COMMANDS_BIT, VK_PIPELINE_STAGE_TRANSFER_BIT, 0, 0, nullptr, 0, nullptr, 1, &initial);
}
inline void VKAPI_CALL CopyImage(VkCommandBuffer cmd, VkImage source, VkImageLayout sourceLayout,
    VkImage destination, VkImageLayout destinationLayout, uint32_t count, const VkImageCopy* regions) {
    for (uint32_t i = 0; i < count; ++i) {
        const auto& s = regions[i].srcSubresource; const auto& d = regions[i].dstSubresource;
        initializeCopy(cmd, source, sourceLayout, {s.aspectMask, s.mipLevel, 1, s.baseArrayLayer, s.layerCount});
        initializeCopy(cmd, destination, destinationLayout, {d.aspectMask, d.mipLevel, 1, d.baseArrayLayer, d.layerCount});
    }
    transferDependency(cmd, true);
    copyImage(cmd, source, sourceLayout, destination, destinationLayout, count, regions);
    transferDependency(cmd, false);
}
inline void VKAPI_CALL ClearColor(VkCommandBuffer cmd, VkImage image, VkImageLayout layout,
    const VkClearColorValue* color, uint32_t count, const VkImageSubresourceRange* ranges) {
    for (uint32_t i = 0; i < count; ++i) initializeCopy(cmd, image, layout, ranges[i]);
    transferDependency(cmd, true);
    clearImage(cmd, image, layout, color, count, ranges);
    transferDependency(cmd, false);
}
inline PFN_vkVoidFunction wrapped(const char* name) {
    if (!std::strcmp(name, "vkCreateImage")) return (PFN_vkVoidFunction)CreateImage;
    if (!std::strcmp(name, "vkDestroyImage")) return (PFN_vkVoidFunction)DestroyImage;
    if (!std::strcmp(name, "vkCmdPipelineBarrier")) return (PFN_vkVoidFunction)Barrier;
    if (!std::strcmp(name, "vkCmdPipelineBarrier2") || !std::strcmp(name, "vkCmdPipelineBarrier2KHR")) return (PFN_vkVoidFunction)Barrier2;
    if (!std::strcmp(name, "vkCmdCopyImage")) return (PFN_vkVoidFunction)CopyImage;
    if (!std::strcmp(name, "vkCmdClearColorImage")) return (PFN_vkVoidFunction)ClearColor;
    return nullptr;
}
inline PFN_vkVoidFunction VKAPI_CALL GetDeviceProcAddr(VkDevice dev, const char* name) {
    auto address = nextDevice(dev, name);
    if (dev != device) return address;
    auto replacement = wrapped(name);
    return address && replacement ? replacement : address;
}
inline PFN_vkVoidFunction VKAPI_CALL GetInstanceProcAddr(VkInstance instance, const char* name) {
    if (!std::strcmp(name, "vkGetDeviceProcAddr")) return (PFN_vkVoidFunction)GetDeviceProcAddr;
    auto address = nextInstance(instance, name);
    auto replacement = wrapped(name);
    return address && replacement ? replacement : address;
}
inline void configure(VkDevice dev, PFN_vkGetInstanceProcAddr instanceProc, PFN_vkGetDeviceProcAddr deviceProc) {
    device = dev; nextInstance = instanceProc; nextDevice = deviceProc;
    createImage = (PFN_vkCreateImage)nextDevice(dev, "vkCreateImage");
    destroyImage = (PFN_vkDestroyImage)nextDevice(dev, "vkDestroyImage");
    barrier = (PFN_vkCmdPipelineBarrier)nextDevice(dev, "vkCmdPipelineBarrier");
    barrier2 = (PFN_vkCmdPipelineBarrier2)nextDevice(dev, "vkCmdPipelineBarrier2");
    copyImage = (PFN_vkCmdCopyImage)nextDevice(dev, "vkCmdCopyImage");
    clearImage = (PFN_vkCmdClearColorImage)nextDevice(dev, "vkCmdClearColorImage");
}
}
