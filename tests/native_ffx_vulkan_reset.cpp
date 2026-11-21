// SPDX-License-Identifier: MIT
// Presenter activation, camera reset delivery and source identity through the
// retained rsf_ffxgvk entry points. No vendor DLL, Vulkan device or GPU work.
#include "native_ffx_vulkan_harness.h"

using namespace fake;

int main() {
    Setup setup;
    auto table = ops();
    auto* self = start(table, setup);
    VkSwapchainKHR chain = state.native->chain;
    VkPresentInfoKHR info{VK_STRUCTURE_TYPE_PRESENT_INFO_KHR}; info.swapchainCount = 1; info.pSwapchains = &chain;
    void* a = id<void*>(10); void* b = id<void*>(11);
    state.check_reset = true;

    require(rsf_ffxgvk_enable(self, 1) == RSF_BACKEND_OK, "explicit FG enable is accepted");
    for (uint64_t frame = 1; frame <= 4; ++frame) {
        // Frame 1 starts a producer, frame 3 carries an engine camera reset.
        const auto source = record(7, frame, 0, 1, frame == 3 ? RSF_FRAME_FLAG_RESET : 0);
        complete(self, source, a, a); rsf_ffxgvk_submitted(self, &a, 1);
        state.expected_reset = frame % 2 == 1;
        require(rsf_ffxgvk_present(self, &info) == VK_SUCCESS && state.enabled && state.generated_frame == frame,
            "submitted reset and ordinary frames keep the presenter active");
    }
    require(state.configurations == 4 && state.activations == 1 && state.generated == 4 && state.submits == 8,
        "camera resets update per-frame SDK inputs without presenter reconfiguration");

    require(rsf_ffxgvk_enable(self, 0) == RSF_BACKEND_OK, "explicit FG disable is accepted");
    const auto disabled = record(7, 5); complete(self, disabled, a, a); rsf_ffxgvk_submitted(self, &a, 1);
    require(rsf_ffxgvk_present(self, &info) == VK_SUCCESS && !state.enabled && state.activations == 2 && state.generated == 4,
        "explicit disable still retires presenter activation");
    rsf_fgvk_status status{sizeof(status), RSF_FGVK_ABI_VERSION};
    require(rsf_ffxgvk_get_status(self, &status) == RSF_BACKEND_OK && !status.enabled && status.generated_callbacks == 4 &&
        status.prepared == 4 && status.tagged == 4, "status counts only enabled preparation and SDK generation");

    require(rsf_ffxgvk_enable(self, 1) == RSF_BACKEND_OK, "FG can resume");
    const auto unknown = record(7, 6, 0, 1, RSF_FRAME_FLAG_RESET);
    auto input = frame(unknown);
    require(rsf_ffxgvk_prepare(self, a, &input) == RSF_BACKEND_OK, "split prepare accepted"); rsf_ffxgvk_submitted(self, &a, 1);
    require(rsf_ffxgvk_hudless(self, b, &input) == RSF_BACKEND_OK, "split HUD-less accepted");
    state.reject_wait = true; rsf_ffxgvk_submitted(self, &b, 1); state.reject_wait = false;
    require(rsf_ffxgvk_present(self, &info) == VK_SUCCESS && !state.enabled && state.generated == 4,
        "unknown GPU completion never admits an input lease");

    // The ledger marks a producer change as a reset; the callback sees the
    // leased record's flags for the exact source each Present names.
    const auto changed = record(7, 7, 2), steady = record(7, 8, 2);
    complete(self, changed, a, a); rsf_ffxgvk_submitted(self, &a, 1);
    complete(self, steady, b, b); rsf_ffxgvk_submitted(self, &b, 1);
    state.expected_reset = true;
    require(rsf_ffxgvk_present_source(self, &info, &changed) == VK_SUCCESS && state.enabled && state.generated_frame == 7,
        "an older explicit source carries its view-change reset into generation");
    state.expected_reset = false;
    require(rsf_ffxgvk_present_source(self, &info, &steady) == VK_SUCCESS && state.enabled && state.generated_frame == 8,
        "the next explicit source generates without a reset");
    const auto next = record(7, 9, 2); complete(self, next, a, a); rsf_ffxgvk_submitted(self, &a, 1);
    auto other_view = next; ++other_view.view_id;
    const auto generated = state.generated;
    require(rsf_ffxgvk_present_source(self, &info, &other_view) == VK_SUCCESS && !state.enabled && state.generated == generated &&
        rsf_ffxgvk_get_status(self, &status) == RSF_BACKEND_OK && !status.source_frame,
        "a mismatched view identity never leases the stored source");
    require(rsf_ffxgvk_present_source(self, &info, &next) == VK_SUCCESS && state.enabled && state.generated_frame == 9 &&
        rsf_ffxgvk_get_status(self, &status) == RSF_BACKEND_OK && status.source_frame == 9,
        "the matching identity still generates after a refused one");

    const auto refused = record(7, 10, 2); complete(self, refused, a, a); rsf_ffxgvk_submitted(self, &a, 1);
    state.reject_generate = true;
    require(rsf_ffxgvk_present_source(self, &info, &refused) == VK_SUCCESS &&
        rsf_ffxgvk_get_status(self, &status) == RSF_BACKEND_OK && status.generated_callbacks == generated + 1,
        "a failed SDK dispatch does not increment the generation count");

    require(rsf_ffxgvk_destroy(self) == RSF_BACKEND_OK && !state.enabled && !routed(),
        "teardown disables generation before releasing the presenter");
    std::puts("PASS: FFX camera resets preserve presenter activation, SDK reset dispatch and exact source identity");
    return 0;
}
