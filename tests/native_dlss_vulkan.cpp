// SPDX-License-Identifier: MIT
// Compile the enabled Vulkan/Streamline leaves, but drive the Rust owner
// with CPU-only callbacks. No graphics device or SDK library is opened.
#include "../runtime/orchestrator/src/native_dlss_vulkan.cpp"
#include <cstdio>
#include <cstdlib>

extern "C" rsf_backend_result rsf_slvk_invoke(const rsf_slvk_call*) {
    return RSF_BACKEND_ERROR_NOT_READY;
}

namespace {
uint32_t allocated{}, freed{}, published{}, captured{}, stage_count{}, reset{}, closes{}, retires{};
bool fail_evaluate{}, fail_release{};
void require(bool result, const char* message) {
    if (!result) { std::fprintf(stderr, "%s\n", message); std::exit(1); }
}
const rsf_vkdlss_platform fake{
    [](const rsf_srvk_setup*, void** native, uint32_t* w, uint32_t* h) -> int32_t {
        *native = reinterpret_cast<void*>(1); *w = 320; *h = 180; return 0;
    },
    [](void*) { ++closes; },
    [](const rsf_slvk_call* call) {
        require(call->abi_version == RSF_SLVK_ABI_VERSION, "SDK lease version");
        require((call->session_id == 0) == (call->frame_id == 0), "whole source identity");
        return call->callback(nullptr, call->frame_id, call->user);
    },
    [](void*, rsf_vkfg_image* image, uint32_t w, uint32_t h, uint32_t format, uint32_t usage) -> uint32_t {
        require(usage == (allocated == 4 ? 15u : 6u), "private resource usage");
        *image = {++allocated, 0, 0, w, h, format, usage, 0}; return 1;
    },
    [](void*, rsf_vkfg_image* image) { if (image->image) ++freed; },
    [](void*, uintptr_t token, const rsf_srvk_frame* input, uint32_t history_reset) -> int32_t {
        require(token == input->record->frame_id, "constants lease the input frame");
        reset = history_reset; return 0;
    },
    [](void*, const rsf_srvk_frame*, uint32_t) -> int32_t { return 0; },
    [](void*, void*, const rsf_srvk_image*, rsf_vkfg_image*, uint32_t) { ++stage_count; },
    [](void*, void*, const rsf_vkfg_image*) {},
    [](void*, uintptr_t, void*, const rsf_srvk_frame*, const rsf_vkdlss_images* images) -> int32_t {
        require(images->output.initialized && images->color.initialized, "initialized output and color are tagged");
        return 0;
    },
    [](void*, uintptr_t, void*) -> int32_t { return fail_evaluate ? RSF_BACKEND_ERROR_FEATURE_FAILED : 0; },
    [](void*, void*, const rsf_srvk_frame*, const rsf_vkfg_image*, uint32_t) { ++captured; },
    [](void*, void*, const rsf_srvk_frame*, uint64_t) {},
    [](void*, void*, const rsf_srvk_frame*, uint64_t) { ++published; },
    [](void*, uintptr_t token, void*) -> int32_t {
        require(!token, "effect retirement uses the SDK session without a frame token");
        return fail_release ? RSF_BACKEND_ERROR_NOT_READY : 0;
    },
    [](void*) {},
    [](void*, void*) { ++retires; },
};
}

int main() {
    rsf_dlssvk* owner = reinterpret_cast<rsf_dlssvk*>(1);
    require(rsf_dlssvk_create(nullptr, &owner) == RSF_BACKEND_ERROR_INVALID_ARGUMENT && !owner, "null setup refusal");
    rsf_srvk_setup setup{};
    setup.struct_size = sizeof(setup); setup.output_width = 640; setup.output_height = 360;
    require(rsf_dlssvk_create(&setup, &owner) == RSF_BACKEND_ERROR_NOT_READY && !owner,
        "enabled adapter respects refusal from the real SDK-lease boundary");
    require(rsf_core_vkdlss_create(&fake, &setup, &owner) == 0 && allocated == 5, "successful owner uses five private images");
    uint32_t width{}, height{};
    require(rsf_dlssvk_plan(owner, &width, &height) == 0 && width == 320 && height == 180, "SDK plan retained");
    auto* commands = reinterpret_cast<void*>(1);
    rsf_frame_record record{};
    record.struct_size = sizeof(record); record.frame_id = 1; record.session_id = 7;
    rsf_srvk_frame frame{}; frame.struct_size = sizeof(frame); frame.record = &record;
    frame.depth.format = VK_FORMAT_R32_SFLOAT; frame.color.copy_source = frame.depth.copy_source = frame.motion.copy_source = 1;
    require(rsf_dlssvk_evaluate(owner, commands, &frame) == 0 && reset && published == 1 && captured == 1 && stage_count == 3,
        "first accepted frame publishes once and resets history");
    ++record.frame_id; fail_evaluate = true;
    require(rsf_dlssvk_evaluate(owner, commands, &frame) == RSF_BACKEND_ERROR_FEATURE_FAILED && !reset &&
        published == 1 && captured == 1, "failed evaluation does not publish or capture results");
    ++record.frame_id; fail_evaluate = false;
    require(rsf_dlssvk_evaluate(owner, commands, &frame) == 0 && reset && published == 2, "failure invalidates history");
    rsf_dlssvk_retire_commands(owner, commands);
    require(retires == 1, "exact command retirement forwarded");
    fail_release = true;
    require(rsf_dlssvk_destroy(owner) == RSF_BACKEND_ERROR_NOT_READY && !freed && !closes, "refused release retains owner");
    fail_release = false;
    require(rsf_dlssvk_destroy(owner) == 0 && freed == 5 && closes == 1, "successful release retires resources exactly once");
    return 0;
}
