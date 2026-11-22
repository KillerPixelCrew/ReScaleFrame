// SPDX-License-Identifier: MIT
#include "../runtime/vulkan/fsr4-fg/bridge_platform.h"
#include "../runtime/orchestrator/src/native_fg_fsr4_vulkan_platform.h"
#include <vulkan/vulkan.h>
#include <vector>
#include <cstdio>

namespace {
int failures = 0;
#define CHECK(value) do { if (!(value)) { std::printf("line %d: %s\n", __LINE__, #value); ++failures; } } while (false)

constexpr uint64_t bridge_luid = 201, bridge_provider = 202;
constexpr int32_t bridge_error = -93;
constexpr auto color = 10, depth = 11, motion = 12, hudless = 13;
struct BridgeCalls {
    uint32_t objects = 0;
    uint64_t reported_provider = bridge_provider;
    uint64_t sequence = 0, done = 0, completion = 0;
    uint32_t presents = 0, releases = 0, destroys_generation = 0, releases_chain = 0, destroys_swapchain = 0;
    int32_t configure_result = 0, wait_result = 0;
    bool fail_generation_destroy = false, fail_swapchain_destroy = false;
    void* telemetry{};
    rsf_fsr4fg_generated_fn notify{};
    std::vector<uint32_t> resets;
    std::vector<char> steps;
} bridge_calls;

template<class T = void>
T* fake(uint64_t id) { return reinterpret_cast<T*>(id); }

const rsf_fsr4fg_platform bridge_platform{
    [](void* telemetry, rsf_fsr4fg_generated_fn notify) {
        bridge_calls.telemetry = telemetry;
        bridge_calls.notify = notify;
        return fake<rsf_fsr4fg_native>(1);
    },
    [](auto*) { ++bridge_calls.releases; },
    [](auto*, uint32_t flags) -> int32_t { CHECK(flags == 0); return 0; },
    [](auto*, uint64_t luid) -> int32_t {
        CHECK(luid == bridge_luid);
        return 0;
    },
    [](auto*, uint32_t* vendor) -> int32_t {
        *vendor = 0x1002;
        return 0;
    },
    [](auto*, uint32_t level) -> int32_t {
        CHECK(level == 0xc000);
        bridge_calls.objects |= RSF_FSR4FG_DEVICE;
        return 0;
    },
    [](auto*, const uint16_t*) -> int32_t { return 0; },
    [](auto*, rsf_ffx_version* versions, uint64_t* count) -> int32_t {
        CHECK(*count >= 1);
        *count = 1;
        versions[0] = {bridge_provider, "4.0.1", 4, 0, 1};
        return 0;
    },
    [](auto*, const auto*, uint64_t provider) -> int32_t {
        CHECK(provider == bridge_provider);
        bridge_calls.objects |= RSF_FSR4FG_GENERATION;
        return 0;
    },
    [](auto*, uint64_t* provider) -> int32_t { *provider = bridge_calls.reported_provider; return 0; },
    [](auto*, const rsf_fsr4fg_queue_desc* desc) -> int32_t {
        CHECK(!desc->list_type && !desc->priority && !desc->flags && !desc->node_mask);
        return 0;
    },
    [](auto*, uint32_t list_type) -> int32_t { CHECK(list_type == 0); return 0; },
    [](auto*, uint32_t node_mask, uint32_t list_type) -> int32_t { CHECK(!node_mask && !list_type); return 0; },
    [](auto*, uint64_t initial_value, uint32_t flags) -> int32_t { CHECK(!initial_value && !flags); return 0; },
    [](auto*, uint32_t manual_reset, uint32_t initial_state) -> int32_t {
        CHECK(!manual_reset && !initial_state);
        bridge_calls.objects |= RSF_FSR4FG_LOCAL_FENCE;
        return 0;
    },
    [](auto*) { return bridge_calls.objects; },
    [](auto*, void*, const rsf_fsr4fg_swapchain_desc* desc, uint32_t first_attempt) -> int32_t {
        CHECK(desc->width == 640 && desc->height == 360 && desc->format == 87);
        CHECK(desc->buffer_count == 3 && desc->buffer_usage == 0x20 && desc->swap_effect == 4);
        CHECK(desc->sample_count == 1 && !desc->sample_quality && !desc->stereo);
        CHECK(!desc->scaling && !desc->alpha_mode && !desc->flags && first_attempt == 1);
        bridge_calls.objects |= RSF_FSR4FG_CHAIN | RSF_FSR4FG_SWAPCHAIN;
        return 0;
    },
    [](auto*, uint32_t, uint32_t, uint32_t, void**, void**) -> int32_t { return bridge_error; },
    [](auto*, void** handle) -> int32_t {
        *handle = fake(2);
        bridge_calls.objects |= RSF_FSR4FG_SHARED_FENCE;
        return 0;
    },
    [](void* resource, rsf_fsr4fg_texture* desc) {
        if (resource == fake(depth)) *desc = {320, 180, 41, 3, 1, 1};
        else if (resource == fake(motion)) *desc = {320, 180, 34, 3, 1, 1};
        else {
            CHECK(resource == fake(color) || resource == fake(hudless));
            *desc = {640, 360, 87, 3, 1, 1};
        }
    },
    [](auto*) -> int32_t { bridge_calls.steps.push_back('R'); return 0; },
    [](auto*) -> int32_t { bridge_calls.steps.push_back('C'); return 0; },
    [](auto*, uint64_t sequence, uint32_t width, uint32_t height, uint32_t enabled, void* image) -> int32_t {
        CHECK(sequence == ++bridge_calls.sequence && width == 640 && height == 360);
        CHECK(!enabled || image == fake(hudless));
        bridge_calls.steps.push_back('F');
        return bridge_calls.configure_result;
    },
    [](auto*, void* image, uint32_t enter) {
        bridge_calls.steps.push_back(enter ? '+' : image == fake(hudless) ? 'H' : '-');
    },
    [](auto*, const auto*, uint64_t sequence, uint32_t reset) -> int32_t {
        CHECK(sequence == bridge_calls.sequence);
        bridge_calls.resets.push_back(reset);
        bridge_calls.steps.push_back('G');
        return 0;
    },
    [](auto*, void* image) -> int32_t {
        CHECK(image == fake(color));
        bridge_calls.steps.push_back('B');
        return 0;
    },
    [](auto*, uint64_t ready) -> int32_t {
        CHECK(ready == bridge_calls.done + 1);
        bridge_calls.steps.push_back('W');
        return 0;
    },
    [](auto*) { bridge_calls.steps.push_back('X'); },
    [](auto*, uint32_t vsync) -> int32_t {
        CHECK(vsync == 1);
        bridge_calls.steps.push_back('P');
        ++bridge_calls.presents;
        return 0;
    },
    [](auto*) -> int32_t {
        bridge_calls.steps.push_back('A');
        return bridge_calls.wait_result;
    },
    [](auto*, uint64_t value, uint32_t shared) -> int32_t {
        if (shared) {
            CHECK(value == bridge_calls.done + 2);
            bridge_calls.done = value;
        } else {
            CHECK(value == ++bridge_calls.completion);
        }
        bridge_calls.steps.push_back(shared ? 'D' : 'S');
        return 0;
    },
    [](auto*, uint64_t value) -> int32_t {
        CHECK(value == bridge_calls.completion);
        bridge_calls.steps.push_back('L');
        return 0;
    },
    [](auto*, uint32_t* count) -> int32_t { *count = bridge_calls.presents; return 0; },
    [](auto*) -> int32_t {
        ++bridge_calls.destroys_generation;
        if (bridge_calls.fail_generation_destroy) return bridge_error;
        bridge_calls.objects &= ~RSF_FSR4FG_GENERATION;
        return 0;
    },
    [](auto*) {
        ++bridge_calls.releases_chain;
        bridge_calls.objects &= ~RSF_FSR4FG_CHAIN;
    },
    [](auto*) -> int32_t {
        ++bridge_calls.destroys_swapchain;
        if (bridge_calls.fail_swapchain_destroy) return bridge_error;
        bridge_calls.objects &= ~RSF_FSR4FG_SWAPCHAIN;
        return 0;
    },
};

int create_bridge(rsf_fsr4fg_bridge** bridge) {
    const uint16_t directory[] = {'X', ':', 92, 0};
    const rsf_fsr4fg_create setup{sizeof(setup), 1, bridge_luid, directory, 640, 360, 1, 0, 0, 1};
    return rsf_core_fsr4fg_create(&bridge_platform, &setup, bridge);
}
void terminal_bridge() {
    bridge_calls = {};
    rsf_fsr4fg_bridge* bridge{};
    CHECK(create_bridge(&bridge) == 0 && bridge);
    if (!bridge) return;
    void* fence{};
    CHECK(RsfFsr4FgCreateFence(bridge, &fence) == 0 && fence);
    CHECK(RsfFsr4FgStart(bridge, fake(3)) == 0);
    bridge_calls.steps.clear();

    rsf_frame_record source{sizeof(source), RSF_GAME_FRAME_ABI_VERSION};
    source.frame_id = source.session_id = 1;
    source.render_width = 320; source.render_height = 180;
    source.output_width = 640; source.output_height = 360;
    source.frame_time_ms = 16.7f;
    source.camera.struct_size = sizeof(source.camera);
    source.camera.abi_version = RSF_GAME_CAMERA_ABI_VERSION;
    source.camera.depth_inverted = 1;
    source.camera.near_plane = 0.1f; source.camera.far_plane = 1000;
    source.camera.vertical_fov_radians = 1.2f;
    rsf_fsr4fg_frame frame{sizeof(frame), 1, &source, fake(color), fake(depth), fake(motion), fake(hudless),
        320, 180, 1, 2, 1, 1};
    rsf_fsr4fg_present_status status{sizeof(status), 1};
    auto present = [&] {
        frame.ready_value = bridge_calls.done + 1;
        frame.done_value = frame.ready_value + 1;
        return RsfFsr4FgPresent(bridge, &frame, &status);
    };

    bridge_calls.notify(bridge_calls.telemetry, 0, 1);
    CHECK(present() == 0 && status.fg_enabled == 1 && status.frames_presented == 1 && status.generated_callbacks == 1);
    const std::vector<char> expected{'R', 'F', '+', '+', '+', 'G', '-', '-', 'B', 'C', 'W', 'X', 'P',
        'A', 'S', 'L', 'R', 'H', 'C', 'X', 'W', 'D', 'S'};
    CHECK(bridge_calls.steps == expected);

    ++source.frame_id;
    CHECK(present() == 0);
    source.frame_id += 2;
    CHECK(present() == 0);
    ++source.frame_id;
    source.flags = RSF_FRAME_FLAG_RESET;
    bridge_calls.notify(bridge_calls.telemetry, bridge_error, 1);
    CHECK(present() == bridge_error && status.generated_callbacks == 1);
    const std::vector<uint32_t> resets{1, 0, 1, 1};
    CHECK(bridge_calls.resets == resets);

    frame.enabled = 0;
    bridge_calls.configure_result = -92;
    CHECK(present() == -92 && status.result == -92 && bridge_calls.presents == 4 && !status.terminal);
    bridge_calls.configure_result = 0;
    frame.enabled = 1;
    CHECK(present() == bridge_error);
    frame.enabled = 0;
    CHECK(present() == 0 && status.fg_enabled == 0 && status.generated_callbacks == 1);

    const auto done = bridge_calls.done;
    bridge_calls.wait_result = bridge_error;
    CHECK(present() == bridge_error && status.result == bridge_error && status.terminal == 1 && bridge_calls.done == done);
    CHECK(RsfFsr4FgDrain(bridge) == bridge_error && bridge_calls.done == done);
    CHECK(RsfFsr4FgPrepareDestroy(bridge) == bridge_error);
    CHECK(RsfFsr4FgDestroy(bridge) == bridge_error);
    CHECK(!bridge_calls.destroys_generation && !bridge_calls.releases_chain && !bridge_calls.releases);
}

void bridge_retirement() {
    bridge_calls = {};
    bridge_calls.reported_provider = bridge_provider + 1;
    bridge_calls.fail_generation_destroy = true;
    rsf_fsr4fg_bridge* bridge{};
    CHECK(create_bridge(&bridge) == RSF_FSR4FG_NOT_SUPPORTED && bridge);
    CHECK(RsfFsr4FgStart(bridge, fake(3)) == RSF_FSR4FG_INVALID_ARGUMENT);
    CHECK(bridge_calls.destroys_generation == 1 && !bridge_calls.releases);
    bridge_calls.fail_generation_destroy = false;
    CHECK(RsfFsr4FgDestroy(bridge) == 0 && bridge_calls.releases == 1);

    bridge_calls = {};
    CHECK(create_bridge(&bridge) == 0 && bridge);
    if (!bridge) return;
    CHECK(RsfFsr4FgStart(bridge, fake(3)) == 0);
    bridge_calls.fail_swapchain_destroy = true;
    CHECK(RsfFsr4FgPrepareDestroy(bridge) == bridge_error);
    CHECK(bridge_calls.destroys_generation == 1 && bridge_calls.releases_chain == 1 && !bridge_calls.releases);
    bridge_calls.fail_swapchain_destroy = false;
    CHECK(RsfFsr4FgPrepareDestroy(bridge) == 0);
    const auto retired = bridge_calls.completion;
    CHECK(RsfFsr4FgDrain(bridge) == 0 && bridge_calls.completion == retired);
    CHECK(RsfFsr4FgDestroy(bridge) == 0 && bridge_calls.releases == 1);
    CHECK(bridge_calls.destroys_generation == 1 && bridge_calls.releases_chain == 1 && bridge_calls.destroys_swapchain == 2);
}
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
    uint32_t cleared = 0, enables = 0, pending_initialized = 0, guide_releases = 0;
    bool fail_marker = false, fail_start = false, fail_importer_destroy = false;
    std::vector<uint64_t> waits, markers, marked_complete, discarded;
    std::vector<uint32_t> initialized_copies;
} calls;

const rsf_vkfg_platform images{
    [](void*) -> uint32_t { return 1; },
    [](void*, uint32_t, uint32_t, uint32_t, uint32_t usage, uint64_t* image) -> int32_t {
        CHECK((usage & (VK_IMAGE_USAGE_TRANSFER_SRC_BIT | VK_IMAGE_USAGE_SAMPLED_BIT)) ==
            (VK_IMAGE_USAGE_TRANSFER_SRC_BIT | VK_IMAGE_USAGE_SAMPLED_BIT));
        *image = calls.next_handle++;
        return 0;
    },
    [](void*, uint64_t, uint64_t* bytes, uint32_t* types) { *bytes = 1; *types = 1; },
    [](void*, uint64_t, uint32_t, uint64_t* memory) -> int32_t { *memory = calls.next_handle++; return 0; },
    [](void*, uint64_t image, uint64_t memory) -> int32_t { CHECK(image && memory); return 0; },
    nullptr, nullptr,
    [](void*, uint64_t image) { CHECK(image); ++calls.images_destroyed; },
    [](void*, uint64_t memory) { CHECK(memory); },
};
constexpr uint32_t all_commands = VK_PIPELINE_STAGE_ALL_COMMANDS_BIT, general_layout = VK_IMAGE_LAYOUT_GENERAL;
constexpr uint32_t transfer_dst_layout = VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL, rgba8 = VK_FORMAT_R8G8B8A8_UNORM, bgra8 = VK_FORMAT_B8G8R8A8_UNORM;
constexpr uint32_t blit_features = VK_FORMAT_FEATURE_BLIT_SRC_BIT | VK_FORMAT_FEATURE_BLIT_DST_BIT;
VkSwapchainCreateInfoKHR swapchain_info{VK_STRUCTURE_TYPE_SWAPCHAIN_CREATE_INFO_KHR};
const rsf_fsr4gvk_ops platform{
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
        // The FIFO swapchain maps to vsync in the Rust admission.
        CHECK(frame->vsync == 1);
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
        [](auto*, uint64_t depth, uint64_t motion) -> int32_t {
            CHECK(depth || motion);
            CHECK(calls.waits.size() == 2 && calls.marked_complete.size() == 2);
            ++calls.guide_releases;
            return 0;
        },
    },
    // Shared graphics table in rsf_dx12gvk_ops order (30 slots).
    {
        [](const auto*, uint64_t* luid) -> int32_t { *luid = adapter; return 0; },
        [](const rsf_fgvk_setup* setup) -> rsf_dx12gvk_platform* {
            CHECK(setup && setup->create_info == &swapchain_info);
            return fake<rsf_dx12gvk_platform>(3);
        },
        [](auto*) {},
        [](auto*, const rsf_srvk_setup* graphics) -> int32_t {
            CHECK(graphics->device == fake(5) && graphics->instance_proc && graphics->device_proc);
            return 0;
        },
        // Blit admission queries RGBA8, then BGRA8, optimal tiling.
        [](auto*, uint32_t format) -> uint32_t {
            CHECK(format == rgba8 || format == bgra8);
            return blit_features;
        },
        [](auto*, const rsf_srvk_setup* graphics) -> int32_t { CHECK(graphics && graphics->device == fake(5)); return 0; },
        [](auto*, uint64_t* pool) -> int32_t { *pool = calls.next_handle++; return 0; },
        [](auto*, uint64_t) { ++calls.pools_destroyed; },
        &images,
        [](auto*, uint64_t, void** list) -> int32_t { *list = fake(calls.next_handle++); return 0; },
        [](auto*, void*) -> int32_t { return 0; },
        [](auto*, void*) -> int32_t { return 0; },
        // The target acquire barrier says whether the following copy or blit reuses initialized contents.
        [](auto*, void* list, const rsf_dx12gvk_barrier* plan) {
            CHECK(list && plan->image);
            CHECK(plan->source_stages == all_commands && plan->target_stages == all_commands);
            if (plan->new_layout == transfer_dst_layout) calls.pending_initialized = plan->old_layout == general_layout;
        },
        [](auto*, void* list, const rsf_dx12gvk_transfer* plan) {
            CHECK(list && plan->source && plan->target);
            calls.initialized_copies.push_back(calls.pending_initialized);
        },
        [](auto*, void* list, const rsf_dx12gvk_transfer* plan) {
            CHECK(list && plan->source && plan->target);
            calls.initialized_copies.push_back(calls.pending_initialized);
        },
        [](auto*, void* list, uint64_t image) { CHECK(list && image); ++calls.cleared; },
        [](auto*, uint64_t, uint64_t) -> int32_t { return 0; },
        // One binary wait from the fixture Present, then the prior done value on the shared timeline.
        [](auto*, void* list, const rsf_dx12gvk_present_submit* plan) -> int32_t {
            CHECK(list && plan->signal == completion_timeline && plan->ready == calls.last_done + 1);
            CHECK(plan->waits[0] == 88 && plan->values[0] == 0 && plan->stages[0] == all_commands);
            if (calls.last_done) {
                CHECK(plan->wait_count == 2 && plan->waits[1] == completion_timeline);
                CHECK(plan->values[1] == calls.last_done && plan->stages[1] == all_commands);
            } else {
                CHECK(plan->wait_count == 1);
            }
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
        // Fixture copy of the native fact reader; Rust applies the admission policy.
        [](const void* create_info, rsf_dx12gvk_swapchain* out) {
            CHECK(create_info == &swapchain_info);
            const auto& info = *static_cast<const VkSwapchainCreateInfoKHR*>(create_info);
            CHECK(!info.pNext);
            *out = {info.imageExtent.width, info.imageExtent.height, static_cast<uint32_t>(info.imageFormat), info.imageUsage,
                static_cast<uint32_t>(info.imageColorSpace), info.imageArrayLayers, static_cast<uint32_t>(info.imageSharingMode),
                info.minImageCount, info.flags, static_cast<uint32_t>(info.presentMode), 0, 0,
                reinterpret_cast<uint64_t>(info.oldSwapchain)};
        },
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
    auto& info = swapchain_info;
    info.imageExtent = {640, 360}; info.imageFormat = VK_FORMAT_R8G8B8A8_UNORM;
    info.imageUsage = VK_IMAGE_USAGE_COLOR_ATTACHMENT_BIT; info.imageColorSpace = VK_COLOR_SPACE_SRGB_NONLINEAR_KHR;
    info.imageArrayLayers = 1; info.imageSharingMode = VK_SHARING_MODE_EXCLUSIVE; info.minImageCount = 3;
    info.presentMode = VK_PRESENT_MODE_FIFO_KHR;
    auto graphics = setup();
    graphics.create_info = &info;
    const rsf_fsr4gvk_options options{sizeof(options), 1, fake(7), 1, 1};
    return rsf_core_fsr4gvk_create(&platform, &graphics, &options, owner, chain);
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
    return {sizeof(rsf_fgvk_frame), RSF_FGVK_ABI_VERSION, &record,
        srv(21, VK_FORMAT_R32_SFLOAT, record.render_width, record.render_height),
        srv(22, VK_FORMAT_R16G16_SFLOAT, record.render_width, record.render_height),
        srv(23, VK_FORMAT_R8G8B8A8_UNORM, 640, 360), 320, 180};
}
void present(rsf_fsr4gvk* owner, uint64_t chain, const rsf_frame_record* record, uint64_t expected, uint32_t enabled) {
    uint32_t index = 0;
    CHECK(rsf_fsr4gvk_acquire(owner, 100, 1, 0, &index) == 0);
    // Exercise the same Vulkan descriptor reader used by both providers.
    const auto semaphore = reinterpret_cast<VkSemaphore>(uint64_t{88});
    const auto swapchain = reinterpret_cast<VkSwapchainKHR>(chain);
    VkResult written = VK_INCOMPLETE;
    VkPresentInfoKHR info{VK_STRUCTURE_TYPE_PRESENT_INFO_KHR};
    info.waitSemaphoreCount = 1; info.pWaitSemaphores = &semaphore;
    info.swapchainCount = 1; info.pSwapchains = &swapchain; info.pImageIndices = &index; info.pResults = &written;
    const int32_t code = rsf_dx12gvk_present_marshaled(
        [](void* context, const rsf_dx12gvk_present_info* admitted, const rsf_frame_record* identity, uint32_t exact) {
            return rsf_core_fsr4gvk_present(static_cast<rsf_fsr4gvk*>(context), admitted, identity, exact);
        }, owner, &info, record, 1);
    CHECK(code == 0 && written == VK_SUCCESS);
    CHECK(calls.presented == expected);
    rsf_fgvk_status status{sizeof(status), RSF_FGVK_ABI_VERSION};
    CHECK(rsf_fsr4gvk_status(owner, &status) == 0 && status.enabled == enabled);
    CHECK(status.source_frame == (enabled ? expected : 0) && status.last_present_count == 1 && status.generated_callbacks == 0);
}
void prove_identity_and_completion() {
    calls = {};
    rsf_fsr4gvk* owner{};
    uint64_t chain{};
    CHECK(create(&owner, &chain) == 0 && owner && chain);
    if (!owner) return;
    CHECK(rsf_fsr4gvk_enable_options(owner, 1, 1) == 0);
    auto first = source(1);
    auto invalid = first;
    ++invalid.resource_generation;
    void* guides = fake(90);
    void* hudless = fake(91);
    void* abandoned = fake(92);
    const auto initial = inputs(first);
    const auto mismatch = inputs(invalid);
    CHECK(rsf_fsr4gvk_prepare(owner, guides, &initial) == 0);
    CHECK(rsf_fsr4gvk_hudless(owner, hudless, &mismatch) == RSF_FSR4FG_INVALID_ARGUMENT);
    CHECK(rsf_fsr4gvk_hudless(owner, hudless, &initial) == 0);
    rsf_fsr4gvk_submitted(owner, &guides, 1);
    CHECK(rsf_fsr4gvk_drain(owner) == VK_NOT_READY && calls.idles == 0);
    present(owner, chain, &invalid, 0, 0);
    void* off_depth = calls.depth;
    CHECK(calls.marked_complete.empty() && calls.markers.size() == 1);
    rsf_fsr4gvk_submitted(owner, &hudless, 1);
    rsf_fsr4gvk_submitted(owner, &hudless, 1);
    CHECK(calls.markers.size() == 2 && calls.markers[0] != calls.markers[1] && calls.waits.empty());

    // A newer recording does not supply any resources for an exact older Present.
    const auto older_discarded = source(2);
    const auto newer = source(3);
    const auto old_inputs = inputs(older_discarded);
    const auto new_inputs = inputs(newer);
    CHECK(rsf_fsr4gvk_prepare(owner, abandoned, &old_inputs) == 0);
    CHECK(rsf_fsr4gvk_prepare(owner, abandoned, &new_inputs) == 0);
    rsf_fsr4gvk_submitted(owner, &abandoned, 1);
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

    auto reused = source(7);
    reused.render_width = 288;
    reused.render_height = 162;
    const auto reused_inputs = inputs(reused);
    CHECK(rsf_fsr4gvk_prepare(owner, guides, &reused_inputs) == 0);
    CHECK(calls.waits.size() == 2 && calls.marked_complete.size() == 2);
    CHECK(calls.guide_releases == 1);
    CHECK(calls.initialized_copies[calls.initialized_copies.size() - 1] == 0);
    CHECK(calls.initialized_copies[calls.initialized_copies.size() - 2] == 0);

    // A source with only the submitted guide stage may present the real frame.
    rsf_fsr4gvk_submitted(owner, &guides, 1);
    present(owner, chain, &reused, 7, 0);
    CHECK(rsf_fsr4gvk_drain(owner) == 0 && calls.marked_complete.size() == 4);
    CHECK(rsf_fsr4gvk_destroy(owner) == 0 && calls.releases == 1);
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
    CHECK(rsf_fsr4gvk_destroy(owner) == 0);
    CHECK(calls.pools_destroyed == 1 && calls.images_destroyed == 3 && calls.releases == 1);

    calls = {};
    CHECK(create(&owner, &chain) == 0 && owner);
    const auto record = source(21);
    const auto frame = inputs(record);
    void* list = fake(90);
    CHECK(rsf_fsr4gvk_prepare(owner, list, &frame) == 0);
    calls.fail_marker = true;
    rsf_fsr4gvk_submitted(owner, &list, 1);
    rsf_fgvk_status status{sizeof(status), RSF_FGVK_ABI_VERSION};
    CHECK(rsf_fsr4gvk_status(owner, &status) == 0 && status.result == injected_error);
    CHECK(rsf_fsr4gvk_drain(owner) == VK_NOT_READY && rsf_fsr4gvk_destroy(owner) == VK_NOT_READY);
    // This synthetic terminal owner is intentionally retained through process exit.
    CHECK(calls.releases == 0 && calls.images_destroyed == 0 && calls.marked_complete.empty());
}
}
int main() {
    bridge_retirement();
    terminal_bridge();
    prove_identity_and_completion();
    prove_admission_and_retirement();
    if (!failures) std::puts("FSR4 Vulkan source identity, submission, timeline and retirement passed (CPU doubles).");
    return failures ? 1 : 0;
}
