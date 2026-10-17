#include <vulkan/vulkan.h>
#include "ngx-vulkan-dispatch.h"
#include <cassert>
#include <cstdio>
using namespace sl::chi::ngxDispatch;
int main() {
    const VkImage image = reinterpret_cast<VkImage>(uintptr_t{1});
    constexpr VkImageAspectFlags both = VK_IMAGE_ASPECT_DEPTH_BIT | VK_IMAGE_ASPECT_STENCIL_BIT;
    fresh[image] = {2, 2, both, std::vector<VkImageAspectFlags>(4)};
    VkImageMemoryBarrier requested{VK_STRUCTURE_TYPE_IMAGE_MEMORY_BARRIER};
    requested.image = image;
    requested.oldLayout = VK_IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL;
    requested.newLayout = VK_IMAGE_LAYOUT_GENERAL;
    requested.subresourceRange = {both, 0, VK_REMAINING_MIP_LEVELS, 0, VK_REMAINING_ARRAY_LAYERS};
    auto initialized = initializedBarriers(1, &requested);
    assert(initialized.size() == 4);
    for (const auto& barrier : initialized) {
        assert(barrier.oldLayout == VK_IMAGE_LAYOUT_UNDEFINED);
        // Combined aspects must stay combined without separateDepthStencilLayouts.
        assert(barrier.subresourceRange.aspectMask == both);
        assert(barrier.subresourceRange.levelCount == 1 && barrier.subresourceRange.layerCount == 1);
    }
    assert(fresh.empty());
    requested.oldLayout = requested.newLayout;
    auto reused = initializedBarriers(1, &requested);
    assert(reused.size() == 1 && reused[0].oldLayout == VK_IMAGE_LAYOUT_GENERAL);
    fresh[image] = {2, 1, VK_IMAGE_ASPECT_COLOR_BIT, std::vector<VkImageAspectFlags>(2)};
    requested.subresourceRange = {VK_IMAGE_ASPECT_COLOR_BIT, 1, 1, 0, 1};
    auto partial = initializedBarriers(1, &requested);
    assert(partial.size() == 1 && partial[0].oldLayout == VK_IMAGE_LAYOUT_UNDEFINED);
    assert(fresh.count(image));
    auto sameMip = initializedBarriers(1, &requested);
    assert(sameMip[0].oldLayout == requested.oldLayout);
    requested.subresourceRange.baseMipLevel = 0;
    auto remaining = initializedBarriers(1, &requested);
    assert(remaining[0].oldLayout == VK_IMAGE_LAYOUT_UNDEFINED && fresh.empty());
    std::puts("SDK first-use mip/layer/aspect and reuse contracts passed.");
}
