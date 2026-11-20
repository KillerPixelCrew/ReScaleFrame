// SPDX-License-Identifier: MIT
// Real private state transitions with CPU-only SDK configuration; no COM calls.
#include "../runtime/vulkan/fsr4-fg/bridge.cpp"
#include "../runtime/orchestrator/src/native_fg_fsr4_vulkan_platform.h"
#include <vulkan/vulkan.h>
#include <vector>
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
void terminal_bridge() {
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
}

namespace {
constexpr uint64_t completion_timeline = 40, adapter = 41;
constexpr int injected_error = -93;
struct Calls {
    uint64_t next_handle = 100;
    uint64_t provider_luid = adapter;
    uint64_t last_done = 0;
    uint64_t presented = 0;
    void* depth = nullptr;
    void* hudless = nullptr;
    uint32_t starts = 0, images_destroyed = 0, pools_destroyed = 0, releases = 0, idles = 0;
    uint32_t cleared = 0, enables = 0;
    bool fail_marker = false, fail_start = false, fail_importer_destroy = false;
    std::vector<uint64_t> waits, markers, marked_complete, discarded;
    std::vector<uint32_t> initialized_copies;
} calls;

template<class T = void>
T* fake(uint64_t id) { return reinterpret_cast<T*>(id); }

const rsf_vkfg_platform images{
    [](void*) -> uint32_t { return 1; },
    [](void*, uint32_t, uint32_t, uint32_t, uint64_t* image) -> int32_t { *image = calls.next_handle++; return 0; },
    [](void*, uint64_t, uint64_t* bytes, uint32_t* types) { *bytes = 1; *types = 1; },
    [](void*, uint64_t, uint32_t, uint64_t* memory) -> int32_t { *memory = calls.next_handle++; return 0; },
    [](void*, uint64_t image, uint64_t memory) -> int32_t { CHECK(image && memory); return 0; },
    nullptr, nullptr,
    [](void*, uint64_t image) { CHECK(image); ++calls.images_destroyed; },
    [](void*, uint64_t memory) { CHECK(memory); },
};
const rsf_fsr4gvk_ops platform{
    [](const char*, uint16_t* out, int32_t) -> int32_t { out[0] = 'X'; return 2; },
    [](uint64_t, const uint16_t*, auto*) -> int32_t { return 0; },
    [](const auto* create, rsf_fsr4fg_bridge** bridge) -> int32_t {
        CHECK(create->adapter_luid == adapter && create->generated_count == 1);
        *bridge = fake<rsf_fsr4fg_bridge>(1);
        return 0;
    },
    [](const auto*, rsf_fsr4fg_info* info) -> int32_t {
        info->adapter_luid = calls.provider_luid; info->provider_version = 700; info->max_generated = 1;
        return 0;
    },
    [](auto*, void*) -> int32_t { ++calls.starts; return calls.fail_start ? injected_error : 0; },
    [](auto*, const rsf_fsr4fg_frame* frame, rsf_fsr4fg_present_status* status) -> int32_t {
        CHECK(frame->done_value == calls.last_done + 2 && frame->ready_value == calls.last_done + 1);
        CHECK(frame->color);
        calls.last_done = frame->done_value;
        calls.presented = frame->record ? frame->record->frame_id : 0;
        calls.enables += frame->enabled;
        calls.depth = frame->depth;
        calls.hudless = frame->hudless;
        if (frame->enabled) {
            CHECK(frame->record && frame->motion_scale_x == 320 && frame->motion_scale_y == 180);
            CHECK(frame->depth && frame->motion && frame->hudless && frame->hudless != frame->color);
        } else {
            CHECK(frame->hudless == frame->color && frame->motion_scale_x == 1);
        }
        status->frames_presented = 1;
        status->fg_enabled = frame->enabled;
        status->generated_callbacks = 0;
        return 0;
    },
    [](auto*) -> int32_t { return 0; },
    [](auto*) -> int32_t { return 0; },
    [](auto*) -> int32_t { ++calls.releases; return 0; },
    [](const auto*, auto*, uint64_t luid, uint32_t enabled, rsf_dx12vk_interop** out) -> int32_t {
        CHECK(luid == adapter && enabled == 1);
        *out = fake<rsf_dx12vk_interop>(2);
        return 0;
    },
    {
        [](auto*, uint32_t format, uint32_t width, uint32_t height, rsf_dx12vk_shared_image* image) -> int32_t {
            const auto id = calls.next_handle++;
            *image = {id, fake(id), format, width, height};
            return 0;
        },
        [](const auto*) -> uint64_t { return completion_timeline; },
        [](auto*, uint32_t drained) -> int32_t { CHECK(drained == 1); return calls.fail_importer_destroy ? injected_error : 0; },
    },
    {
        [](const auto*, uint64_t* luid) -> int32_t { *luid = adapter; return 0; },
        [](const auto*, uint32_t, rsf_fsr4gvk_platform** out) -> int32_t { *out = fake<rsf_fsr4gvk_platform>(3); return 0; },
        [](auto*) {},
        [](auto*, uint64_t* pool) -> int32_t { *pool = calls.next_handle++; return 0; },
        [](auto*, uint64_t) { ++calls.pools_destroyed; },
        &images,
        [](auto*, uint64_t, void** list) -> int32_t { *list = fake(calls.next_handle++); return 0; },
        [](auto*, void*) -> int32_t { return 0; },
        [](auto*, void*) -> int32_t { return 0; },
        [](auto*, void*, const auto*, const auto*, uint32_t initialized) { calls.initialized_copies.push_back(initialized); },
        [](auto*, void*, const auto*) { ++calls.cleared; },
        [](auto*, uint64_t, uint64_t) -> int32_t { return 0; },
        [](auto*, void*, const rsf_fsr4gvk_present_info* info, uint64_t timeline, uint64_t previous, uint64_t ready) -> int32_t {
            CHECK(timeline == completion_timeline && previous == calls.last_done && ready == previous + 1);
            CHECK(info->wait_count == 1 && info->waits[0] == 88);
            return 0;
        },
        [](auto*, uint64_t timeline, uint64_t value, uint64_t timeout) -> int32_t {
            CHECK(timeline == completion_timeline && timeout != 0 && value <= calls.last_done);
            return 0;
        },
        [](auto*, uint64_t* fence) -> int32_t { *fence = calls.next_handle++; return 0; },
        [](auto*, uint64_t fence) -> int32_t {
            calls.markers.push_back(fence);
            return calls.fail_marker ? injected_error : 0;
        },
        [](auto*, uint64_t fence, uint64_t) -> int32_t { calls.waits.push_back(fence); return 0; },
        [](auto*, uint64_t) {},
        [](auto*) -> int32_t { ++calls.idles; return 0; },
        [](auto*, uint64_t provider, uint32_t backend) { CHECK(provider == 700 && backend == 4); },
        [](auto*, void*, const auto*, const auto*, const char*, uint64_t group) { CHECK(group != 0); },
        [](auto*, uint64_t group) { calls.marked_complete.push_back(group); },
        [](auto*, uint64_t group) { calls.discarded.push_back(group); },
        [](auto*) -> int32_t { return 0; },
    },
};

rsf_fgvk_setup setup() {
    rsf_fgvk_setup s{sizeof(s), RSF_FGVK_ABI_VERSION};
    s.graphics = {sizeof(s.graphics), RSF_SRVK_ABI_VERSION};
    s.graphics.instance = s.graphics.physical_device = s.graphics.device = fake(5);
    s.graphics.instance_proc = [](void*, const char*) -> rsf_vk_function { return nullptr; };
    s.graphics.device_proc = s.graphics.instance_proc;
    s.graphics.runtime_directory_utf8 = "fixture";
    s.queues[0] = fake(6);
    s.graphics_submit = [](void*, uint32_t, const void*, uint64_t) -> int32_t { return 0; };
    s.lock_queues = s.unlock_queues = [](void*) {};
    return s;
}
int32_t create(rsf_fsr4gvk** owner, uint64_t* chain) {
    const auto graphics = setup();
    const rsf_fsr4gvk_options options{sizeof(options), 1, fake(7), 1, 1};
    const rsf_fsr4gvk_swapchain description{640, 360, VK_FORMAT_R8G8B8A8_UNORM, VK_IMAGE_USAGE_COLOR_ATTACHMENT_BIT,
        VK_COLOR_SPACE_SRGB_NONLINEAR_KHR, 1, VK_SHARING_MODE_EXCLUSIVE, 3, 0, 1, 1, 0};
    return rsf_core_fsr4gvk_create(&platform, &graphics, &options, &description, owner, chain);
}
rsf_frame_record source(uint64_t frame) {
    rsf_frame_record record{sizeof(record), RSF_GAME_FRAME_ABI_VERSION};
    record.frame_id = frame; record.session_id = 11; record.view_id = 12; record.resource_generation = 13;
    record.render_width = 320; record.render_height = 180;
    record.output_width = 640; record.output_height = 360;
    record.camera.struct_size = sizeof(record.camera); record.camera.abi_version = RSF_GAME_CAMERA_ABI_VERSION;
    return record;
}
rsf_fgvk_frame inputs(const rsf_frame_record& record) {
    auto srv = [](uint64_t image, uint32_t format, uint32_t width, uint32_t height) -> rsf_srvk_image {
        return {fake(5), image, format, width, height, VK_IMAGE_LAYOUT_SHADER_READ_ONLY_OPTIMAL, 1};
    };
    return {sizeof(rsf_fgvk_frame), RSF_FGVK_ABI_VERSION, &record, srv(21, VK_FORMAT_R32_SFLOAT, 320, 180),
        srv(22, VK_FORMAT_R16G16_SFLOAT, 320, 180), srv(23, VK_FORMAT_R8G8B8A8_UNORM, 640, 360), 320, 180};
}
void present(rsf_fsr4gvk* owner, uint64_t chain, const rsf_frame_record* record, uint64_t expected, uint32_t enabled) {
    uint32_t index = 0;
    CHECK(rsf_core_fsr4gvk_acquire(owner, 100, 1, 0, &index) == 0);
    const uint64_t wait = 88;
    int32_t result = 5;
    const rsf_fsr4gvk_present_info info{chain, index, 1, &wait, &result};
    CHECK(rsf_core_fsr4gvk_present(owner, &info, record, 1) == 0 && result == 0);
    CHECK(calls.presented == expected);
    rsf_fgvk_status status{sizeof(status), RSF_FGVK_ABI_VERSION};
    CHECK(rsf_core_fsr4gvk_status(owner, &status) == 0 && status.enabled == enabled);
    CHECK(status.source_frame == (enabled ? expected : 0) && status.last_present_count == 1 && status.generated_callbacks == 0);
}
void prove_identity_and_completion() {
    calls = {};
    rsf_fsr4gvk* owner{};
    uint64_t chain{};
    CHECK(create(&owner, &chain) == 0 && owner && chain);
    if (!owner) return;
    CHECK(rsf_core_fsr4gvk_enable_options(owner, 1, 1) == 0);
    auto first = source(1);
    auto invalid = first;
    ++invalid.resource_generation;
    void* guides = fake(90);
    void* hudless = fake(91);
    void* abandoned = fake(92);
    const auto initial = inputs(first);
    const auto mismatch = inputs(invalid);
    CHECK(rsf_core_fsr4gvk_prepare(owner, guides, &initial) == 0);
    CHECK(rsf_core_fsr4gvk_hudless(owner, hudless, &mismatch) == RSF_FSR4FG_INVALID_ARGUMENT);
    CHECK(rsf_core_fsr4gvk_hudless(owner, hudless, &initial) == 0);
    rsf_core_fsr4gvk_submitted(owner, &guides, 1);
    CHECK(rsf_core_fsr4gvk_drain(owner) == VK_NOT_READY && calls.idles == 0);
    present(owner, chain, &invalid, 0, 0);
    void* off_depth = calls.depth;
    CHECK(calls.marked_complete.empty() && calls.markers.size() == 1);
    rsf_core_fsr4gvk_submitted(owner, &hudless, 1);
    rsf_core_fsr4gvk_submitted(owner, &hudless, 1);
    CHECK(calls.markers.size() == 2 && calls.markers[0] != calls.markers[1] && calls.waits.empty());

    // A newer recording does not supply any resources for an exact older Present.
    const auto older_discarded = source(2);
    const auto newer = source(3);
    const auto old_inputs = inputs(older_discarded);
    const auto new_inputs = inputs(newer);
    CHECK(rsf_core_fsr4gvk_prepare(owner, abandoned, &old_inputs) == 0);
    CHECK(rsf_core_fsr4gvk_prepare(owner, abandoned, &new_inputs) == 0);
    rsf_core_fsr4gvk_submitted(owner, &abandoned, 1);
    CHECK(calls.discarded.size() == 1 && calls.markers.size() == 3);
    struct alignas(rsf_frame_record) IdentityPrefix {
        uint32_t size, version;
        uint64_t frame, session;
        uint32_t view, generation;
    };
    static_assert(sizeof(IdentityPrefix) == offsetof(rsf_frame_record, phase));
    const IdentityPrefix prefix{sizeof(prefix), RSF_GAME_FRAME_ABI_VERSION,
        first.frame_id, first.session_id, first.view_id, first.resource_generation};
    const auto* short_identity = reinterpret_cast<const rsf_frame_record*>(&prefix);
    present(owner, chain, short_identity, 1, 1);
    CHECK(calls.enables == 1 && calls.depth != off_depth && calls.cleared == 2);
    present(owner, chain, short_identity, 1, 0);
    CHECK(calls.depth == off_depth);
    CHECK(calls.marked_complete.empty());

    const auto reused = source(7);
    const auto reused_inputs = inputs(reused);
    CHECK(rsf_core_fsr4gvk_prepare(owner, guides, &reused_inputs) == 0);
    CHECK(calls.waits.size() == 2 && calls.marked_complete.size() == 2);
    CHECK(calls.initialized_copies[calls.initialized_copies.size() - 1] == 1);

    // A source with only the submitted guide stage may present the real frame.
    rsf_core_fsr4gvk_submitted(owner, &guides, 1);
    present(owner, chain, &reused, 7, 0);
    CHECK(rsf_core_fsr4gvk_drain(owner) == 0 && calls.marked_complete.size() == 4);
    CHECK(rsf_core_fsr4gvk_destroy(owner) == 0 && calls.releases == 1);
}
void prove_admission_and_retirement() {
    calls = {};
    calls.provider_luid += 1;
    rsf_fsr4gvk* owner{};
    uint64_t chain{};
    CHECK(create(&owner, &chain) == VK_ERROR_INVALID_EXTERNAL_HANDLE && !owner && !chain);
    CHECK(!calls.starts && calls.releases == 1);
    calls = {};
    calls.fail_start = calls.fail_importer_destroy = true;
    CHECK(create(&owner, &chain) == injected_error && owner && !chain);
    CHECK(calls.pools_destroyed == 1 && calls.images_destroyed == 3 && calls.releases == 0);
    calls.fail_importer_destroy = false;
    CHECK(rsf_core_fsr4gvk_destroy(owner) == 0);
    CHECK(calls.pools_destroyed == 1 && calls.images_destroyed == 3 && calls.releases == 1);

    calls = {};
    CHECK(create(&owner, &chain) == 0 && owner);
    const auto record = source(21);
    const auto frame = inputs(record);
    void* list = fake(90);
    CHECK(rsf_core_fsr4gvk_prepare(owner, list, &frame) == 0);
    calls.fail_marker = true;
    rsf_core_fsr4gvk_submitted(owner, &list, 1);
    rsf_fgvk_status status{sizeof(status), RSF_FGVK_ABI_VERSION};
    CHECK(rsf_core_fsr4gvk_status(owner, &status) == 0 && status.result == injected_error);
    CHECK(rsf_core_fsr4gvk_drain(owner) == VK_NOT_READY && rsf_core_fsr4gvk_destroy(owner) == VK_NOT_READY);
    // This synthetic terminal owner is intentionally retained through process exit.
    CHECK(calls.releases == 0 && calls.images_destroyed == 0 && calls.marked_complete.empty());
}
}
int main() {
    terminal_bridge();
    prove_identity_and_completion();
    prove_admission_and_retirement();
    if (!failures) std::puts("FSR4 Vulkan source identity, submission, timeline and retirement passed (CPU doubles).");
    return failures ? 1 : 0;
}
