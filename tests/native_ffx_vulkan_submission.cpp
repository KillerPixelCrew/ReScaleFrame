// SPDX-License-Identifier: MIT
// Source recording, exact submission identity, completion evidence and teardown
// through the retained rsf_ffxgvk entry points.
#include "native_ffx_vulkan_harness.h"

using namespace fake;

int main() {
    Setup setup;
    auto table = ops();
    auto* self = start(table, setup);
    rsf_ffxgvk* second = nullptr; uint64_t second_chain = 0;
    require(rsf_core_ffxgvk_create(&table, &setup.value, &setup.swapchain, &second, &second_chain) == RSF_BACKEND_ERROR_NOT_SUPPORTED &&
        !second && !second_chain, "one FFX owner holds the process-wide WSI route");
    require(rsf_ffxgvk_enable(self, 1) == RSF_BACKEND_OK, "FG enabled");

    auto* gpu = &state.native->gpu;
    rsf_vkfg_image cached{};
    require(rsf_core_vkfg_ensure_image(&rsf::vkfg::platform, gpu, &cached, 640, 360, VK_FORMAT_R8G8B8A8_UNORM, 0),
        "initial private image is fully bound");
    const auto initial = cached.image;
    state.reject_bind = true;
    require(!rsf_core_vkfg_ensure_image(&rsf::vkfg::platform, gpu, &cached, 1280, 720, VK_FORMAT_R8G8B8A8_UNORM, 0) &&
        cached.image == initial && cached.width == 640 && state.destroyed_images == 1 && state.freed_memories == 1,
        "failed private image replacement rolls back without touching the committed cache");
    state.reject_bind = false;
    rsf_core_vkfg_release_image(&rsf::vkfg::platform, gpu, &cached);
    state.destroyed_images = state.freed_memories = 0;

    uint64_t image_handles[8] = {99, 99, 99, 99, 99, 99, 99, 99};
    uint32_t image_count = 8;
    require(rsf_ffxgvk_images(self, &image_count, image_handles) == VK_SUCCESS && image_count == 3 &&
        image_handles[0] == 1000 && image_handles[2] == 1002 && image_handles[3] == 99,
        "oversized image capacity reports only initialized SDK handles");
    image_count = 2;
    require(rsf_ffxgvk_images(self, &image_count, image_handles) == VK_INCOMPLETE && image_count == 2,
        "short image capacity preserves Vulkan incomplete semantics");
    image_count = 0;
    require(rsf_ffxgvk_images(self, &image_count, nullptr) == VK_SUCCESS && image_count == 3, "count-only image enumeration preserved");

    VkSwapchainKHR chain = state.native->chain, foreign_chain = id<VkSwapchainKHR>(77);
    VkPresentInfoKHR info{VK_STRUCTURE_TYPE_PRESENT_INFO_KHR}; info.swapchainCount = 1; info.pSwapchains = &chain;
    VkPresentInfoKHR foreign = info; foreign.pSwapchains = &foreign_chain;
    require(rsf_ffxgvk_present(self, &foreign) == VK_ERROR_INITIALIZATION_FAILED && !state.present_calls,
        "a foreign chain never reaches the FFX presenter");

    void* a = id<void*>(10); void* b = id<void*>(11); void* c = id<void*>(12);
    auto one = record(7, 1); complete(self, one, a, a);
    void* duplicate[] = {a, a, nullptr}; rsf_ffxgvk_submitted(self, duplicate, 3);
    require(state.submits == 1 && state.resets == 1, "one command buffer with both roles gets exactly one completion fence");
    rsf_ffxgvk_submitted(self, duplicate, 3);
    require(state.submits == 1 && state.resets == 1, "repeated notification never rearms an accepted old slot");
    auto two = record(7, 2); auto input = frame(two);
    require(rsf_ffxgvk_prepare(self, a, &input) == RSF_BACKEND_OK, "reused command address records another source");
    rsf_ffxgvk_submitted(self, &a, 1);
    require(state.submits == 2 && state.resets == 2, "prepare-only source is fenced without rearming the older accepted source");
    auto three = record(7, 3); complete(self, three, b, b);
    auto four = record(7, 4, 2); complete(self, four, b, b);
    rsf_ffxgvk_submitted(self, &b, 1);
    require(state.submits == 3, "only the newest recording on a reused command buffer is fenced");
    require(rsf_ffxgvk_present_source(self, &info, &three) == VK_SUCCESS && !state.enabled,
        "an abandoned recording cannot generate");
    require(rsf_ffxgvk_present(self, &info) == VK_SUCCESS && state.enabled && state.generated == 1 && state.generated_frame == 4,
        "newest submitted source is presentation eligible and leased to the SDK callback");
    require(state.submits == 4, "the generated source receives a consumer fence");

    auto five = record(7, 5); input = frame(five); state.reject_prepare = true;
    require(rsf_ffxgvk_prepare(self, c, &input) == RSF_BACKEND_ERROR_FEATURE_FAILED,
        "SDK recording failure remains distinct from accepted FG preparation");
    state.reject_prepare = false;
    require(rsf_ffxgvk_drain(self) == RSF_BACKEND_ERROR_NOT_READY && rsf_ffxgvk_destroy(self) == RSF_BACKEND_ERROR_NOT_READY &&
        !state.drains && !state.effects_destroyed, "failed SDK recording retains its owner until the host submits the partial work");
    auto before = state.submits; rsf_ffxgvk_submitted(self, &c, 1);
    require(state.submits == before + 1, "partially recorded SDK failure still gets submission completion");
    require(rsf_ffxgvk_present(self, &info) == VK_SUCCESS && !state.enabled, "failed prepare does not replay the previous source");

    auto six = record(7, 6); input = frame(six);
    require(rsf_ffxgvk_prepare(self, a, &input) == RSF_BACKEND_OK, "split prepare accepted");
    rsf_ffxgvk_submitted(self, &a, 1); before = state.submits; const auto earlier_waits = state.waits;
    require(rsf_ffxgvk_hudless(self, b, &input) == RSF_BACKEND_OK, "split HUD-less accepted");
    require(rsf_ffxgvk_drain(self) == RSF_BACKEND_ERROR_NOT_READY,
        "submitted guides do not permit retirement of a later unsubmitted HUD-less copy");
    rsf_ffxgvk_submitted(self, &b, 1);
    require(state.submits == before + 1 && state.waits == earlier_waits + 1, "later role waits prior fence before establishing a new completion point");
    rsf_ffxgvk_submitted(self, &b, 1); require(state.submits == before + 1, "split duplicate submission is idempotent");
    require(rsf_ffxgvk_present(self, &info) == VK_SUCCESS && state.enabled, "split submitted roles can generate");

    auto seven = record(7, 7); input = frame(seven);
    require(rsf_ffxgvk_prepare(self, a, &input) == RSF_BACKEND_OK, "completed ring slot can be reused"); rsf_ffxgvk_submitted(self, &a, 1);
    require(rsf_ffxgvk_hudless(self, b, &input) == RSF_BACKEND_OK, "later role recorded"); before = state.resets; state.reject_wait = true;
    rsf_ffxgvk_submitted(self, &b, 1); state.reject_wait = false;
    require(state.resets == before, "positive fence timeout preserves unknown completion without reset");
    require(rsf_ffxgvk_present(self, &info) == VK_SUCCESS && !state.enabled, "unknown completion refuses FG");

    auto new_session = record(99, 2, 8, 3); input = frame(new_session);
    require(rsf_ffxgvk_prepare(self, c, &input) == RSF_BACKEND_OK, "new session can restart original source IDs");
    for (uint32_t kind = 0; kind < 3; ++kind) {
        auto wrong_source = new_session;
        if (kind == 0) ++wrong_source.session_id;
        if (kind == 1) ++wrong_source.view_id;
        if (kind == 2) ++wrong_source.resource_generation;
        auto wrong = frame(wrong_source); const auto previous_copies = state.copies;
        require(rsf_ffxgvk_hudless(self, c, &wrong) == RSF_BACKEND_ERROR_INVALID_ARGUMENT && state.copies == previous_copies,
            "same numeric frame never joins another session, view or resource generation");
    }
    require(rsf_ffxgvk_hudless(self, c, &input) == RSF_BACKEND_OK, "matching identity completes new session"); rsf_ffxgvk_submitted(self, &c, 1);
    rsf_fgvk_status status{sizeof(status), RSF_FGVK_ABI_VERSION};
    require(rsf_ffxgvk_present(self, &info) == VK_SUCCESS && state.enabled && rsf_ffxgvk_get_status(self, &status) == RSF_BACKEND_OK &&
        status.enabled && status.source_frame == 2, "presentation admission uses the local recording order, not old session frame IDs");
    require(rsf_ffxgvk_present(self, &info) == VK_SUCCESS && !state.enabled, "same recording cannot present twice");

    auto older = record(99, 3, 8, 3), newer = record(99, 4, 8, 3);
    complete(self, older, a, a); rsf_ffxgvk_submitted(self, &a, 1);
    complete(self, newer, b, b); rsf_ffxgvk_submitted(self, &b, 1);
    require(rsf_ffxgvk_present_source(self, &info, &older) == VK_SUCCESS && state.enabled && state.generated_frame == 3 &&
        rsf_ffxgvk_get_status(self, &status) == RSF_BACKEND_OK && status.source_frame == 3,
        "older output selects its own submitted guides without rewinding the producer");
    require(rsf_ffxgvk_present_source(self, &info, &newer) == VK_SUCCESS && state.enabled && state.generated_frame == 4,
        "newer prepared source remains available after the older Present");
    auto invalid_source = newer; invalid_source.struct_size = 4;
    require(rsf_ffxgvk_present_source(self, &info, &invalid_source) == VK_ERROR_INITIALIZATION_FAILED, "short Present identity refused");
    invalid_source = newer; ++invalid_source.abi_version;
    require(rsf_ffxgvk_present_source(self, &info, &invalid_source) == VK_ERROR_INITIALIZATION_FAILED, "foreign Present identity ABI refused");
    auto foreign_source = record(99, 5, 8, 3); complete(self, foreign_source, a, a); rsf_ffxgvk_submitted(self, &a, 1);
    ++foreign_source.session_id;
    require(rsf_ffxgvk_present_source(self, &info, &foreign_source) == VK_SUCCESS && !state.enabled &&
        rsf_ffxgvk_get_status(self, &status) == RSF_BACKEND_OK && !status.source_frame,
        "foreign identity retains ordinary output without selecting latest guides");
    auto forbidden_source = record(99, 6, 8, 3, RSF_FRAME_FLAG_NO_FG);
    complete(self, forbidden_source, b, b); rsf_ffxgvk_submitted(self, &b, 1);
    require(rsf_ffxgvk_present_source(self, &info, &forbidden_source) == VK_SUCCESS && !state.enabled,
        "explicit source preserves NO_FG admission refusal");
    require(rsf_ffxgvk_present_source(self, &info, nullptr) == VK_SUCCESS && !state.enabled &&
        rsf_ffxgvk_get_status(self, &status) == RSF_BACKEND_OK && !status.source_frame,
        "unassociated output never guesses a prepared source");

    // presentInterpolated submits interpolation before it returns an earlier
    // asynchronous present result. That source still needs its consumer fence.
    auto late = record(99, 8, 8, 3); complete(self, late, c, c); rsf_ffxgvk_submitted(self, &c, 1);
    before = state.submits; state.present_result = VK_ERROR_OUT_OF_DATE_KHR;
    require(rsf_ffxgvk_present(self, &info) == VK_ERROR_OUT_OF_DATE_KHR && state.enabled && state.submits == before + 1 &&
        rsf_ffxgvk_get_status(self, &status) == RSF_BACKEND_OK && status.result == RSF_BACKEND_ERROR_FEATURE_FAILED && status.enabled,
        "an error result after interpolation keeps the HUD-less consumer fence");
    state.present_result = VK_SUCCESS;
    const auto waits_before_reuse = state.waits;
    auto reuse = record(99, 14, 8, 3); input = frame(reuse);
    require(rsf_ffxgvk_prepare(self, a, &input) == RSF_BACKEND_OK && state.waits == waits_before_reuse + 2,
        "slot reuse waits for both the source and the consumer completion");
    rsf_ffxgvk_submitted(self, &a, 1);
    require(state.sdk_records >= 8, "the scenarios exercised real SDK prepare callsites");

    // Teardown keeps every resource until each completion step succeeds.
    state.reject_drain = true;
    require(rsf_ffxgvk_destroy(self) == RSF_BACKEND_ERROR_NOT_READY && !state.effects_destroyed && !state.chains_destroyed,
        "an unproven drain retains the owner and SDK contexts");
    state.reject_drain = false; state.reject_effect_destroy = true;
    const auto drains = state.drains;
    require(rsf_ffxgvk_destroy(self) == RSF_BACKEND_ERROR_FEATURE_FAILED && state.drains == drains + 1 && !state.chains_destroyed,
        "a refused effect destruction retains the chain and swapchain context");
    image_count = 0;
    auto later = record(99, 15, 8, 3); input = frame(later);
    require(rsf_ffxgvk_present(self, &info) == VK_ERROR_INITIALIZATION_FAILED &&
        rsf_ffxgvk_images(self, &image_count, nullptr) == VK_ERROR_INITIALIZATION_FAILED &&
        rsf_ffxgvk_prepare(self, a, &input) == RSF_BACKEND_ERROR_INVALID_ARGUMENT,
        "a partially retired owner refuses new recording and presentation");
    state.reject_effect_destroy = false; state.reject_swapchain_destroy = true;
    require(rsf_ffxgvk_destroy(self) == RSF_BACKEND_ERROR_FEATURE_FAILED && state.effects_destroyed == 1 && state.chains_destroyed == 1 &&
        owner.load() == state.native, "the chain goes before a refused swapchain context, and the WSI route stays registered");
    state.reject_swapchain_destroy = false;
    const auto fences_before = state.destroyed_fences;
    require(rsf_ffxgvk_destroy(self) == RSF_BACKEND_OK && state.effects_destroyed == 1 && state.chains_destroyed == 1 &&
        state.swapchains_destroyed == 1 && state.destroyed_images == 6 && state.destroyed_fences == fences_before + 12 && !owner.load(),
        "retry completes teardown without repeating released steps and releases the WSI route last");
    std::puts("PASS: FFX source recording, reused command identity, submission completion and retained teardown");
}
