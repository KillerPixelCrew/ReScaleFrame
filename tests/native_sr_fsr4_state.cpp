// SPDX-License-Identifier: MIT
// Exercise the linked Rust owner with synthetic graphics and SDK leaves.
#include "../runtime/orchestrator/src/native_sr_fsr4_vulkan_platform.h"
#include <windows.h>
#include <vulkan/vulkan.h>
#include <array>
#include <cstdio>
#include <string_view>
#include <vector>

namespace {
int failures{};
#define CHECK(value) do { if (!(value)) { std::printf("line %d: %s\n", __LINE__, #value); ++failures; } } while (false)

template<class T = void> T* fake(uint64_t id) { return reinterpret_cast<T*>(id); }

constexpr uint64_t adapter = 19, version = 29, timeline = 39;
constexpr uint32_t family = 7, render_width = 192, render_height = 108, output_width = 320, output_height = 180;
constexpr auto storage_sampled = VK_IMAGE_USAGE_STORAGE_BIT | VK_IMAGE_USAGE_SAMPLED_BIT;
constexpr uint32_t formats[] = {VK_FORMAT_R16G16B16A16_SFLOAT, VK_FORMAT_R32_SFLOAT, VK_FORMAT_R16G16_SFLOAT, VK_FORMAT_R16G16B16A16_SFLOAT};
const auto producer = fake(1), resumed = fake(2), device = fake(3);
const auto bridge = fake<rsf_fsr4_bridge>(4);
struct Barrier { void* commands; uint64_t image; uint32_t before, after, transfer; };
struct Copy { void* commands; uint64_t src, dst; };
struct Record { void* commands; uint64_t image; std::string_view role; uint32_t reset; };
enum class Step { wait, idle, prepare, flush, clear, image, memory, com, timeline, importer, bridge, graphics };
struct {
    unsigned images{}, resources{}, queries{}, retires{}, sharpened{};
    int sdk_error{}, split_error{}, wait_error{}, idle_error{}, prepare_error{}, destroy_error{};
    int bind_error_at{-1};
    bool repeat_commands{}, flush_error{};
    uint64_t expected_done{};
    float sharpness{};
    std::vector<Barrier> barriers;
    std::vector<Copy> copies;
    std::vector<Record> captures;
    std::vector<std::array<uint64_t, 3>> splits;
    std::vector<rsf_fsr4_bridge_frame> dispatches;
    std::vector<Step> retirement;
} calls;

int32_t fake_open(const rsf_srvk_setup* setup, void** owner) {
    CHECK(setup->device == device);
    *owner = device;
    return 0;
}

int32_t event(void** out) {
    // The real importer closes this Win32 NT handle after its synthetic import.
    *out = CreateEventW(nullptr, FALSE, FALSE, nullptr);
    return *out ? 0 : VK_ERROR_OUT_OF_HOST_MEMORY;
}

const rsf_dx12vk_platform interop{
    [](const auto*, uint64_t* out) -> int32_t { *out = adapter; return 0; },
    fake_open,
    [](void*) { calls.retirement.push_back(Step::importer); },
    [](void*) -> uint32_t { return 1; },
    [](void*, uint64_t* out) -> int32_t { *out = timeline; return 0; },
    [](void*, uint64_t value, void* handle) -> int32_t { CHECK(value == timeline && handle); return 0; },
    [](void*, uint64_t value) { CHECK(value == timeline); calls.retirement.push_back(Step::timeline); },
    [](void*, uint32_t format, uint32_t extra, uint32_t* allowed, uint32_t* width, uint32_t* height) -> int32_t {
        CHECK(calls.queries < std::size(formats) && format == formats[calls.queries]);
        CHECK(extra == (calls.queries == 3 ? storage_sampled : 0u));
        ++calls.queries;
        *allowed = 1; *width = output_width; *height = output_height;
        return 0;
    },
    [](void*, uint32_t format, uint32_t width, uint32_t height, uint32_t extra, uint64_t* out) -> int32_t {
        const auto index = calls.images++;
        CHECK(index < std::size(formats) && format == formats[index]);
        CHECK(extra == (index == 3 ? storage_sampled : 0u));
        CHECK(width == (index == 3 ? output_width : render_width) && height == (index == 3 ? output_height : render_height));
        *out = 100 + index;
        return 0;
    },
    [](void*, uint64_t, uint64_t* bytes, uint32_t* types) { *bytes = 256; *types = 0b1101; },
    [](void*, void*, uint32_t* types) -> int32_t { *types = 0b0110; return 0; },
    [](void*) -> uint32_t { return 0b1100; },
    [](void*, uint64_t image, void*, uint64_t bytes, uint32_t kind, uint64_t* out) -> int32_t {
        CHECK(bytes == 256 && kind == 2);
        *out = image + 100;
        return 0;
    },
    [](void*, uint64_t image, uint64_t memory) -> int32_t {
        CHECK(memory == image + 100);
        return static_cast<int>(image) == calls.bind_error_at ? VK_ERROR_UNKNOWN : 0;
    },
    [](void*, uint64_t) { calls.retirement.push_back(Step::image); },
    [](void*, uint64_t) { calls.retirement.push_back(Step::memory); },
};

const rsf_fsr4vk_platform platform{
    &interop, fake_open,
    [](void*) { calls.retirement.push_back(Step::graphics); },
    [](const rsf_fsr4_bridge_create* data, rsf_fsr4_bridge** out) -> int32_t {
        CHECK(data->struct_size == sizeof(*data) && data->abi_version == RSF_FSR4_BRIDGE_ABI_VERSION);
        CHECK(data->adapter_luid == adapter && data->depth_inverted == 1 && data->motion_jittered == 1);
        CHECK(data->render_width == output_width && data->render_height == output_height);
        CHECK(data->output_width == output_width && data->output_height == output_height);
        CHECK(std::u16string_view(reinterpret_cast<const char16_t*>(data->runtime_directory)) == u"C:\\signed\u00e9");
        *out = bridge;
        return 0;
    },
    [](auto* value, uint32_t quality, uint32_t* width, uint32_t* height) -> int32_t {
        CHECK(value == bridge && quality == 2);
        *width = render_width; *height = render_height;
        return 0;
    },
    [](const auto* value, rsf_fsr4_bridge_info* info) -> int32_t {
        CHECK(value == bridge && info->struct_size == sizeof(*info) && info->abi_version == RSF_FSR4_BRIDGE_ABI_VERSION);
        info->adapter_luid = adapter; info->provider_version = version;
        info->vendor_id = 0x8086; info->int8_compatibility = 1;
        return 0;
    },
    [](void* value, void** out) -> int32_t { CHECK(value == bridge); return event(out); },
    [](auto* value, uint32_t width, uint32_t height, uint32_t format, uint32_t writable, void** handle, void** out) -> int32_t {
        const auto index = calls.resources++;
        CHECK(value == bridge && index < std::size(formats) && format == formats[index]);
        CHECK(writable == (index == 3 ? 1u : 0u));
        CHECK(width == (index == 3 ? output_width : render_width) && height == (index == 3 ? output_height : render_height));
        *out = fake(300 + index);
        return event(handle);
    },
    [](void*) { calls.retirement.push_back(Step::com); },
    [](void*, const auto*, uint64_t provider) -> int32_t { CHECK(provider == version); return 0; },
    [](const rsf_frame_record* record) { CHECK(record->session_id == 10 && record->view_id == 11); },
    [](void*, void* commands, const auto* image, const auto*, const char* role, uint32_t reset) {
        calls.captures.push_back({commands, image->image, role, reset});
    },
    [](const rsf_srvk_frame*, uint32_t reset) { CHECK(calls.dispatches.back().reset == reset); },
    [](void*) -> int32_t { calls.retirement.push_back(Step::flush); return calls.flush_error ? 0 : 1; },
    [](void*, void* commands, uint64_t image, uint32_t before, uint32_t after, uint32_t queue_family, uint32_t transfer) {
        CHECK(queue_family == family);
        calls.barriers.push_back({commands, image, before, after, transfer});
    },
    [](void*, void* commands, uint64_t source, uint64_t target, uint32_t width, uint32_t height) {
        CHECK(width == (commands == producer ? render_width : output_width));
        CHECK(height == (commands == producer ? render_height : output_height));
        calls.copies.push_back({commands, source, target});
    },
    [](const auto* handoff, void* commands, uint64_t sem, uint64_t prior, uint64_t ready, uint64_t done, void** out) -> int32_t {
        CHECK(handoff->user == device && commands == producer && sem == timeline && !*out);
        calls.splits.push_back({prior, ready, done});
        *out = calls.repeat_commands ? producer : resumed;
        return calls.split_error;
    },
    [](const auto*) -> int32_t { return 1; },
    [](auto* value, float amount) -> int32_t { CHECK(value == bridge); calls.sharpness = amount; return 0; },
    [](auto* value, const rsf_fsr4_bridge_frame* frame) -> int32_t {
        CHECK(value == bridge && frame->struct_size == sizeof(*frame) && frame->abi_version == RSF_FSR4_BRIDGE_ABI_VERSION);
        CHECK(frame->color == fake(300) && frame->depth == fake(301) && frame->motion == fake(302) && frame->output == fake(303));
        CHECK(frame->ready_value == calls.splits.back()[1] && frame->done_value == calls.splits.back()[2]);
        CHECK(frame->motion_scale_x == -192.f && frame->motion_scale_y == 108.f && frame->jitter_x == .25f && frame->pre_exposure == 2.f);
        calls.dispatches.push_back(*frame);
        calls.expected_done = frame->done_value;
        return calls.sdk_error;
    },
    [](void*, const auto*, void* commands, const auto*, uint64_t image) {
        CHECK(commands == resumed && image == 103);
        const auto& acquire = calls.barriers.back();
        CHECK(acquire.commands == resumed && acquire.image == image && acquire.transfer == 1 &&
            acquire.before == VK_IMAGE_LAYOUT_GENERAL && acquire.after == VK_IMAGE_LAYOUT_GENERAL);
        ++calls.sharpened;
    },
    [](void*, void* commands) { CHECK(commands == resumed); ++calls.retires; },
    [](void*) { calls.retirement.push_back(Step::clear); },
    [](void*, uint64_t sem, uint64_t target, uint64_t timeout) -> int32_t {
        CHECK(sem == timeline && target == calls.expected_done && timeout == 10000000000ull);
        calls.retirement.push_back(Step::wait);
        return calls.wait_error;
    },
    [](void*) -> int32_t { calls.retirement.push_back(Step::idle); return calls.idle_error; },
    [](auto* value) -> int32_t { CHECK(value == bridge); calls.retirement.push_back(Step::prepare); return calls.prepare_error; },
    [](auto* value) -> int32_t { CHECK(value == bridge); calls.retirement.push_back(Step::bridge); return calls.destroy_error; },
};

struct Fixture {
    rsf_srvk_setup setup{sizeof(setup), RSF_SRVK_ABI_VERSION, fake(5), fake(6), device};
    rsf_fsr4vk_handoff handoff{sizeof(handoff), RSF_SRVK_SPLIT_TRACKED_ABI_VERSION, 1, family, device};
    rsf_frame_record record{};
    rsf_srvk_frame frame{};
    rsf_fsr4vk* owner{};
    void* next{};

    Fixture() {
        calls = {};
        setup.instance_proc = setup.device_proc = [](void*, const char*) -> rsf_vk_function { return nullptr; };
        setup.backend = RSF_SR_FSR4; setup.quality = 2;
        setup.output_width = output_width; setup.output_height = output_height;
        setup.depth_inverted = setup.motion_jittered = 1;
        setup.runtime_directory_utf8 = "C:\\signed\xc3\xa9";
        handoff.split_record = [](void*, void*, uint64_t, uint64_t, uint64_t, uint64_t, void**) -> int32_t { return -1; };
        record.struct_size = sizeof(record); record.abi_version = RSF_GAME_FRAME_ABI_VERSION;
        record.session_id = 10; record.view_id = 11; record.frame_id = 42; record.resource_generation = 12;
        record.render_width = render_width; record.render_height = render_height;
        record.output_width = output_width; record.output_height = output_height;
        record.frame_time_ms = 16.5f;
        auto& camera = record.camera;
        camera.struct_size = sizeof(camera); camera.abi_version = RSF_GAME_CAMERA_ABI_VERSION;
        camera.render_width = render_width; camera.render_height = render_height;
        camera.output_width = output_width; camera.output_height = output_height;
        camera.depth_inverted = camera.motion_jittered = 1;
        camera.jitter_pixels[0] = .25f; camera.jitter_pixels[1] = -.25f;
        camera.near_plane = .1f; camera.far_plane = 1000.f; camera.vertical_fov_radians = 1.f;
        frame.struct_size = sizeof(frame); frame.abi_version = RSF_SRVK_ABI_VERSION; frame.record = &record;
        frame.color = {device, 20, formats[0], render_width, render_height, VK_IMAGE_LAYOUT_SHADER_READ_ONLY_OPTIMAL, 1};
        frame.depth = frame.color; frame.depth.image = 21; frame.depth.format = formats[1];
        frame.motion = frame.color; frame.motion.image = 22; frame.motion.format = formats[2];
        frame.output = {device, 23, formats[3], output_width, output_height, VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL, 0};
        frame.sharpening.struct_size = sizeof(frame.sharpening);
        frame.sharpening.mode = RSF_SHARPENING_VENDOR; frame.sharpening.sharpness = .7f;
        frame.motion_scale_x = -192.f; frame.motion_scale_y = 108.f; frame.pre_exposure = 2.f;
    }

    int create() { return rsf_core_fsr4vk_create(&platform, &setup, &handoff, &owner); }
    int evaluate() { return rsf_fsr4vk_evaluate(owner, producer, &frame, &next); }
    int destroy() { return rsf_fsr4vk_destroy(owner); }
    void submitted() {
        CHECK(rsf_fsr4vk_submitted(owner, resumed) == RSF_FSR4_BRIDGE_INVALID_ARGUMENT);
        CHECK(rsf_fsr4vk_submitted(owner, fake(789)) == RSF_FSR4_BRIDGE_INVALID_ARGUMENT);
        CHECK(rsf_fsr4vk_submitted(owner, producer) == 0);
        CHECK(rsf_fsr4vk_submitted(owner, producer) == RSF_FSR4_BRIDGE_INVALID_ARGUMENT);
        CHECK(destroy() == VK_NOT_READY && calls.retirement.empty());
        CHECK(rsf_fsr4vk_submitted(owner, resumed) == 0);
    }
};

void evaluate_and_retire() {
    Fixture fixture;
    CHECK(fixture.create() == 0 && fixture.owner && calls.images == 4 && calls.resources == 4);
    uint32_t width{}, height{};
    CHECK(rsf_fsr4vk_plan(fixture.owner, &width, &height) == 0 && width == render_width && height == render_height);
    rsf_fsr4_bridge_info info{sizeof(info), RSF_FSR4_BRIDGE_ABI_VERSION};
    CHECK(rsf_fsr4vk_info(fixture.owner, &info) == 0 && info.provider_version == version);
    fixture.frame.depth.copy_source = 0;
    CHECK(fixture.evaluate() == RSF_FSR4_BRIDGE_INVALID_ARGUMENT && fixture.next == producer && calls.barriers.empty());
    fixture.frame.depth.copy_source = 1;
    CHECK(fixture.evaluate() == 0 && fixture.next == resumed && calls.sharpness == .7f);
    CHECK((calls.splits.back() == std::array<uint64_t, 3>{0, 1, 2}));
    CHECK(calls.dispatches.back().reset == 1 && calls.sharpened == 1 && calls.copies.size() == 4);
    CHECK(calls.copies.back().commands == resumed && calls.copies.back().src == 103 && calls.copies.back().dst == 23);
    CHECK(calls.captures.size() == 5 && calls.captures.back().commands == resumed &&
        calls.captures.back().role == "sr-output-linear" && calls.captures.back().image == 103);
    for (const auto& capture : calls.captures) CHECK(capture.reset == 1);
    CHECK(calls.barriers[1].transfer == 0 && calls.barriers[1].before == VK_IMAGE_LAYOUT_UNDEFINED);
    ++fixture.record.frame_id;
    CHECK(fixture.evaluate() == VK_NOT_READY && fixture.next == producer && calls.splits.size() == 1);
    fixture.submitted();
    rsf_fsr4vk_retire_commands(fixture.owner, resumed);
    CHECK(calls.retires == 1);

    --fixture.record.frame_id;
    CHECK(fixture.evaluate() == RSF_FSR4_BRIDGE_INVALID_ARGUMENT && fixture.next == producer && calls.splits.size() == 1);
    ++fixture.record.frame_id;
    ++fixture.record.view_id;
    CHECK(fixture.evaluate() == RSF_FSR4_BRIDGE_INVALID_ARGUMENT && fixture.next == producer && calls.splits.size() == 1);
    --fixture.record.view_id;
    calls.sdk_error = 71;
    const auto barriers_before = calls.barriers.size();
    CHECK(fixture.evaluate() == 71 && fixture.next == resumed && calls.dispatches.back().reset == 0);
    CHECK((calls.splits.back() == std::array<uint64_t, 3>{2, 3, 4}));
    CHECK(calls.barriers[barriers_before + 1].transfer == 1 && calls.barriers[barriers_before + 1].before == VK_IMAGE_LAYOUT_GENERAL);
    CHECK(calls.sharpened == 1 && calls.copies.size() == 7 && calls.captures.size() == 9);
    fixture.submitted();

    calls.sdk_error = 0;
    ++fixture.record.frame_id;
    CHECK(fixture.evaluate() == 0 && calls.dispatches.back().reset == 1);
    fixture.submitted();
    ++fixture.record.frame_id;
    ++fixture.record.resource_generation;
    CHECK(fixture.evaluate() == 0 && calls.dispatches.back().reset == 1);
    fixture.submitted();

    const auto captures_before_gap = calls.captures.size();
    fixture.record.frame_id += 2;
    CHECK(fixture.evaluate() == 0 && calls.dispatches.back().reset == 1);
    for (auto i = captures_before_gap; i < calls.captures.size(); ++i) CHECK(calls.captures[i].reset == 1);
    fixture.submitted();

    calls.wait_error = VK_TIMEOUT;
    CHECK(fixture.destroy() == VK_TIMEOUT && calls.retirement == std::vector{Step::wait});
    calls.wait_error = 0; calls.idle_error = -78; calls.retirement.clear();
    CHECK((fixture.destroy() == -78 && calls.retirement == std::vector{Step::wait, Step::idle}));
    calls.idle_error = 0; calls.prepare_error = -79; calls.retirement.clear();
    CHECK((fixture.destroy() == -79 && calls.retirement == std::vector{Step::wait, Step::idle, Step::prepare}));
    ++fixture.record.frame_id;
    CHECK(fixture.evaluate() == RSF_FSR4_BRIDGE_INVALID_ARGUMENT && !fixture.next);
    calls.prepare_error = 0; calls.flush_error = true; calls.retirement.clear();
    CHECK((fixture.destroy() == VK_ERROR_UNKNOWN && calls.retirement == std::vector{Step::wait, Step::idle, Step::prepare, Step::flush}));
    CHECK(rsf_fsr4vk_plan(fixture.owner, &width, &height) == RSF_FSR4_BRIDGE_INVALID_ARGUMENT);
    calls.flush_error = false; calls.destroy_error = -80; calls.retirement.clear();
    CHECK((fixture.destroy() == -80 && calls.retirement == std::vector{Step::wait, Step::idle, Step::prepare,
        Step::flush, Step::clear, Step::image, Step::memory, Step::com, Step::image, Step::memory, Step::com,
        Step::image, Step::memory, Step::com, Step::image, Step::memory, Step::com, Step::timeline, Step::importer, Step::bridge}));
    calls.destroy_error = 0; calls.retirement.clear();
    CHECK((fixture.destroy() == 0 && calls.retirement == std::vector{Step::idle, Step::prepare,
        Step::flush, Step::clear, Step::bridge, Step::graphics}));
}

void create_failure_and_legacy() {
    Fixture fixture;
    calls.bind_error_at = 101;
    calls.prepare_error = -90;
    CHECK(fixture.create() == VK_ERROR_UNKNOWN && fixture.owner);
    CHECK((calls.retirement == std::vector{Step::idle, Step::prepare}));
    CHECK(fixture.evaluate() == RSF_FSR4_BRIDGE_INVALID_ARGUMENT && !fixture.next);
    calls.prepare_error = 0; calls.retirement.clear();
    CHECK((fixture.destroy() == 0 && calls.retirement == std::vector{Step::idle, Step::prepare,
        Step::flush, Step::clear, Step::image, Step::memory, Step::com, Step::image, Step::memory, Step::com,
        Step::timeline, Step::importer, Step::bridge, Step::graphics}));

    Fixture legacy;
    legacy.handoff.abi_version = RSF_SRVK_SPLIT_ABI_VERSION;
    CHECK(legacy.create() == 0 && legacy.evaluate() == 0 && legacy.next == resumed);
    CHECK(rsf_fsr4vk_submitted(legacy.owner, producer) == RSF_FSR4_BRIDGE_INVALID_ARGUMENT);
    CHECK(legacy.destroy() == 0);

    Fixture terminal;
    CHECK(terminal.create() == 0);
    calls.repeat_commands = true;
    CHECK(terminal.evaluate() == VK_ERROR_INITIALIZATION_FAILED && !terminal.next && calls.dispatches.empty());
    CHECK(terminal.evaluate() == RSF_FSR4_BRIDGE_INVALID_ARGUMENT && !terminal.next);
    CHECK(terminal.destroy() == VK_NOT_READY && calls.retirement.empty());
    // The terminal owner intentionally remains live until process exit.
}
}

int main() {
    evaluate_and_retire();
    create_failure_and_legacy();
    return failures != 0;
}
