// SPDX-License-Identifier: GPL-3.0-only
// Real private state transitions with CPU-only SDK configuration; no COM calls.
#include "../runtime/vulkan/fsr4-fg/bridge.cpp"
#include "../runtime/orchestrator/src/native_fg_fsr4_vulkan.cpp"
#include "../runtime/orchestrator/src/native_dx12_vulkan_interop.cpp"
#include <cstdio>

namespace {
int failures = 0, configured = 0;
ffxReturnCode_t configure_result = FFX_API_RETURN_OK;
#define CHECK(value) do { if (!(value)) { std::printf("line %d: %s\n", __LINE__, #value); ++failures; } } while (false)
ffxReturnCode_t fake_configure(ffxContext* context, const ffxConfigureDescHeader* header) {
    CHECK(context && header->type == FFX_API_CONFIGURE_DESC_TYPE_FRAMEGENERATION);
    const auto& config = *reinterpret_cast<const ffxConfigureDescFrameGeneration*>(header);
    CHECK(!config.frameGenerationEnabled && config.frameID == static_cast<uint64_t>(++configured));
    return configure_result;
}
}
int main() {
    rsf_fsr4fg_bridge bridge{};
    bridge.generation = reinterpret_cast<void*>(1); bridge.chain = reinterpret_cast<IDXGISwapChain4*>(2);
    bridge.configure = fake_configure; bridge.width = 640; bridge.height = 360;
    bridge.generated_result = -91;
    CHECK(configure(bridge, false, nullptr) == 0 && bridge.generated_result.load() == 0 && !bridge.enabled);
    configure_result = FFX_API_RETURN_ERROR;
    bridge.generated_result = -92;
    CHECK(configure(bridge, false, nullptr) == static_cast<int32_t>(FFX_API_RETURN_ERROR) && bridge.generated_result.load() == -92);
    bridge.terminal = -93; bridge.done = 10;
    rsf_fsr4fg_frame frame{sizeof(frame), 1}; frame.ready_value = 11; frame.done_value = 12;
    rsf_fsr4fg_present_status status{sizeof(status), 1};
    CHECK(RsfFsr4FgPresent(&bridge, &frame, &status) == -93 && status.terminal == 1 && bridge.done == 10);
    CHECK(RsfFsr4FgDrain(&bridge) == -93 && bridge.done == 10);
    CHECK(RsfFsr4FgPrepareDestroy(&bridge) == -93 && bridge.generation && bridge.chain);
    CHECK(RsfFsr4FgDestroy(&bridge) == -93 && bridge.generation && bridge.chain);
    rsf_fsr4gvk owner{}; owner.terminal = true;
    owner.device = reinterpret_cast<VkDevice>(3); owner.pool = reinterpret_cast<VkCommandPool>(4);
    CHECK(rsf_fsr4gvk_drain(&owner) == VK_NOT_READY && owner.device && owner.pool);
    CHECK(rsf_fsr4gvk_destroy(&owner) == VK_NOT_READY && owner.device && owner.pool);
    if (!failures) std::puts("FSR4 FG Off error reset and terminal owner retention passed (synthetic, no GPU).");
    return failures ? 1 : 0;
}
