// SPDX-License-Identifier: MIT
#include "vulkan_control.h"
#include "vulkan_creation.h"
#include <rescaleframe/native_sr_vulkan.h>
#include <rescaleframe/native_latency_vulkan.h>
#include <rescaleframe/native_streamline_vulkan.h>
#include <rescaleframe/native_xess_vulkan_requirements.h>
#include <rescaleframe/input_dump.h>
#include <rescaleframe/frame_generation.h>
#if defined(RSF_FRAME_DUMP_VULKAN)
#include <rescaleframe/input_dump_vulkan.h>
#include "../../../runtime/backends/common/frame_capture_vk.h"
#endif
#if defined(RSF_VULKAN_FSR4_FG_ENABLED)
#include "../../../runtime/orchestrator/src/native_fg_fsr4_vulkan.h"
#endif
#if defined(RSF_VULKAN_FSR4_ENABLED)
#include "../../../runtime/orchestrator/src/native_sr_fsr4_vulkan.h"
#else
struct rsf_fsr4vk;
#endif
#include <windows.h>
#include <cstddef>
#include <mutex>
#include <string>
#include <cstdio>
#include <vector>

// The boot state, session identity, requested settings and their generation, the status record,
// configure admission and the presenter, FG and SR accounting are owned by the Rust Vulkan session
// (native/vulkan.rs), which also keeps the engine's primary window, and the SR contexts, their
// retirement, creation, plan, evaluate and fallback sequencing by its SR context owner
// (native/vulkan/sr.rs). The CPU events and present markers route through native/window.rs and
// the leaves in runtime.cpp. The creation resolver's routes, requirements and XeSS and interop
// readiness are owned by native/vulkan/creation.rs, with its leaves in vulkan_creation.cpp; the
// start leaf below still picks its underlying resolver. This file keeps the guard, the Vulkan,
// Streamline, XeSS, FidelityFX, presenter, capture and SR provider calls; under the guard it reads
// the session through rsf_godot_vk_view_get and reports each transition. Lock order: guard, then
// the Rust state.
static_assert(sizeof(rsf_godot_vk_facts) == 40 && sizeof(rsf_godot_vk_latency_facts) == 40 &&
    offsetof(rsf_godot_vk_latency_facts, sleep_calls) == 24);
static_assert(sizeof(rsf_godot_vk_view) == 136 && offsetof(rsf_godot_vk_view, generation) == 104 &&
    offsetof(rsf_godot_vk_view, fg_result) == 132);
static_assert(sizeof(rsf_godot_vk_fg_pass) == 48 && offsetof(rsf_godot_vk_fg_pass, options_result) == 36);
static_assert(sizeof(rsf_godot_vk_present_facts) == 32);
static_assert(sizeof(rsf_godot_vk_sr_pass) == 72 && offsetof(rsf_godot_vk_sr_pass, output_width) == 32 &&
    offsetof(rsf_godot_vk_sr_pass, source_frame_id) == 40 && offsetof(rsf_godot_vk_sr_pass, handoff) == 56 &&
    sizeof(rsf_godot_vk_sr_create) == 8 && sizeof(rsf_godot_vk_sr_context) == 16 &&
    offsetof(rsf_godot_vk_sr_context, result) == 8);
static_assert(RSF_BACKEND_ERROR_NOT_SUPPORTED == -7 && RSF_BACKEND_ERROR_NOT_READY == -8 &&
    RSF_SCREEN_FLIGHT == 4 && RSF_GAME_RENDER_SR == 1 && RSF_SR_FSR3 == 3 && RSF_SR_FSR4 == 4 &&
    RSF_SR_XESS == 5 && RSF_SR_AUTO == 7 && RSF_FG_BACKEND_DLSS == 1 && RSF_FG_BACKEND_FSR3 == 3 &&
    RSF_FG_BACKEND_FSR4 == 4 && RSF_GODOT_ABI_VERSION == 3,
    "native/vulkan.rs mirrors these Runtime constants");
static_assert(RSF_BACKEND_OK == 0 && RSF_BACKEND_ERROR_INVALID_ARGUMENT == -1 &&
    RSF_BACKEND_ERROR_NOT_COMPILED == -3 && RSF_SR_NONE == 0 && RSF_SR_DLSS == 1 && RSF_SR_SPATIAL == 6,
    "native/vulkan/sr.rs mirrors these Runtime constants");

namespace {
std::mutex guard;
std::string directory;
std::string streamline_directory;
std::string fsr4_directory;
std::wstring intel_directory;
uint64_t render_source_frame = 0;
rsf_game_log_fn logger = nullptr;
void* logger_user = nullptr;
rsf_fgvk* presentation = nullptr;
// Device/proc callback borrows remain valid while the presentation owner lives.
// This copy never retains caller create-info or runtime-directory pointers.
rsf_srvk_setup presentation_graphics{};
rsf_godot_vk_proc resolver = nullptr;
bool sdk_shutdown_pending = false;
bool graph_handoff_available = false;
#if defined(RSF_FRAME_DUMP_VULKAN)
rsf_input_dump_vk* dump_owner = nullptr;
rsf::FrameCaptureVk dump_stages;
rsf_input_dump_vk_setup dump_setup{};
VkFormat dump_format = VK_FORMAT_UNDEFINED;
VkExtent2D dump_extent{};
VkImageUsageFlags dump_usage = 0;
VkSwapchainKHR dump_chain = VK_NULL_HANDLE;
rsf_frame_record dump_record{};
int32_t release_capture(void* user) {
    if (!dump_owner || dump_setup.user != user) return RSF_BACKEND_OK;
    const auto joined = rsf_input_dump_vk_drain(dump_owner);
    if (joined != RSF_BACKEND_OK) return joined;
    (void)dump_stages.flush();
    const auto stopped = rsf_input_dump_vk_destroy(dump_owner);
    if (stopped != RSF_BACKEND_OK) return stopped;
    dump_owner = nullptr; dump_chain = VK_NULL_HANDLE; dump_record = {}; dump_setup = {};
    return RSF_BACKEND_OK;
}
void setup_capture(const rsf_fgvk_setup& setup) {
    if (!setup.create_info || (dump_owner && dump_setup.user != setup.queue_user)) return;
    const auto& info = *static_cast<const VkSwapchainCreateInfoKHR*>(setup.create_info);
    dump_setup = {sizeof(dump_setup), 1, setup.graphics, setup.queues[0], setup.queue_family, setup.graphics_submit, setup.queue_user};
    dump_setup.graphics.runtime_directory_utf8 = nullptr;
    if (!dump_owner) (void)rsf_input_dump_vk_create(&dump_setup, &dump_owner);
    dump_format = info.imageFormat; dump_extent = info.imageExtent; dump_usage = info.imageUsage;
    VkPhysicalDeviceMemoryProperties memory{};
    const auto properties = reinterpret_cast<PFN_vkGetPhysicalDeviceMemoryProperties>(setup.graphics.instance_proc(setup.graphics.instance, "vkGetPhysicalDeviceMemoryProperties"));
    if (properties) { properties(static_cast<VkPhysicalDevice>(setup.graphics.physical_device), &memory);
        (void)dump_stages.initialize(static_cast<VkDevice>(setup.graphics.device), setup.graphics.device_proc, memory); }
}
void capture_present_impl(rsf_fgvk* owner, const void* native_info, int32_t result, uint32_t begin) {
    const bool armed = rsf_input_dump_requested() != 0;
    rsf_input_dump_poll();
    if (!armed || !dump_owner || !native_info || !render_source_frame) return;
    rsf_godot_vk_view view{};
    (void)rsf_godot_vk_view_get(&view);
    auto record = dump_record;
    if (record.frame_id != render_source_frame) {
        record = {}; record.struct_size = sizeof(record); record.abi_version = RSF_GAME_FRAME_ABI_VERSION;
        record.session_id = view.session_id; record.frame_id = render_source_frame;
        record.output_width = dump_extent.width; record.output_height = dump_extent.height;
        record.flags = RSF_FRAME_FLAG_NO_SR | RSF_FRAME_FLAG_NO_FG;
    }
    if (!begin) { rsf_input_dump_complete(&record, result); return; }
    (void)rsf_input_dump_begin_record(&record, view.settings.sr_backend);
    if (!rsf_input_dump_wants(&record)) return;
    const auto& info = *static_cast<const VkPresentInfoKHR*>(native_info);
    if (info.swapchainCount != 1 || !info.pSwapchains || !info.pImageIndices) return;
    if (!owner && dump_chain && dump_chain != info.pSwapchains[0]) return;
    dump_chain = info.pSwapchains[0];
    uint32_t count = 0;
    std::vector<uint64_t> images;
    uint32_t layout = VK_IMAGE_LAYOUT_PRESENT_SRC_KHR;
    if (owner) {
        if (rsf_fgvk_images(owner, &count, nullptr) != 0 || !count || count > 16) return;
        images.resize(count);
        if (rsf_fgvk_images(owner, &count, images.data()) != 0) return;
        if (rsf_fgvk_image_layout(owner, &layout) != 0) return;
    } else {
        const auto get = reinterpret_cast<PFN_vkGetSwapchainImagesKHR>(dump_setup.graphics.device_proc(dump_setup.graphics.device, "vkGetSwapchainImagesKHR"));
        if (!get || get(static_cast<VkDevice>(dump_setup.graphics.device), dump_chain, &count, nullptr) != VK_SUCCESS || !count || count > 16) return;
        std::vector<VkImage> native(count);
        if (get(static_cast<VkDevice>(dump_setup.graphics.device), dump_chain, &count, native.data()) != VK_SUCCESS) return;
        for (auto image : native) images.push_back(reinterpret_cast<uint64_t>(image));
    }
    if (info.pImageIndices[0] >= images.size()) return;
    const rsf_srvk_image image{dump_setup.graphics.device, images[info.pImageIndices[0]], uint32_t(dump_format),
        dump_extent.width, dump_extent.height, layout, (dump_usage & VK_IMAGE_USAGE_TRANSFER_SRC_BIT) ? 1u : 0u};
    const auto code = rsf_input_dump_vk_snapshot(dump_owner, &record, &image, "final_color");
    if (code) {
        const auto submitted = rsf_input_dump_gpu_submission(&record);
        if (rsf_input_dump_vk_drain(dump_owner) == RSF_BACKEND_OK)
            rsf_input_dump_gpu_completed(submitted, 3, dump_setup.graphics.device);
    }
    if (code && logger) logger(logger_user, "Core frame dump: final Vulkan image could not be read; see the bundle's missing stage.");
}
#endif
rsf_srvk_image image(const rsf_godot_vk_packet* packet, const rsf_godot_vk_image& value, uint32_t layout) {
    return rsf_srvk_image{packet->device, value.image, value.format, value.width, value.height, layout, value.copy_source};
}
// One SR pass as the SR leaves see it. The Rust owner holds only its address for the call.
struct sr_call {
    void* commands;
    void** continuation;
    const rsf_godot_vk_packet* packet;
    const rsf_godot_vk_handoff* handoff;
    rsf_frame_record* record;
    rsf_srvk_frame frame; // the last evaluated frame, for the input refusal log
    uint32_t plan_width, plan_height;
    int fsr4_code;
};
sr_call& pass_of(void* call) { return *static_cast<sr_call*>(call); }
rsf_srvk_setup sr_setup(const sr_call& call, const rsf_godot_vk_sr_create& request) {
    const auto* packet = call.packet;
    const auto& camera = packet->pass.camera;
    rsf_srvk_setup setup{}; setup.struct_size = sizeof(setup); setup.abi_version = RSF_SRVK_ABI_VERSION;
    setup.instance = packet->instance; setup.physical_device = packet->physical_device; setup.device = packet->device;
    setup.instance_proc = reinterpret_cast<rsf_vk_get_proc>(packet->instance_proc);
    setup.device_proc = reinterpret_cast<rsf_vk_get_proc>(packet->device_proc);
    setup.backend = request.backend; setup.quality = request.quality; setup.output_width = camera.output_width; setup.output_height = camera.output_height;
    setup.depth_inverted = camera.depth_inverted;
    // The pinned Godot producer supplies finite z_far; camera ABI 1 has no infinite-depth promise.
    setup.depth_infinite = 0;
    setup.motion_jittered = camera.motion_jittered; setup.runtime_directory_utf8 = directory.c_str();
    return setup;
}
rsf_srvk_frame sr_frame(const sr_call& call, const rsf_sharpening_options& sharpening) {
    const auto* packet = call.packet;
    const auto& pass = packet->pass;
    const auto& camera = pass.camera;
    rsf_srvk_frame frame{}; frame.struct_size = sizeof(frame); frame.abi_version = RSF_SRVK_ABI_VERSION; frame.record = call.record;
    // Vulkan SHADER_READ_ONLY_OPTIMAL = 5, TRANSFER_DST_OPTIMAL = 7.
    frame.sharpening = sharpening;
    frame.log=logger;frame.log_user=logger_user;
    frame.color = image(packet, packet->color, 5); frame.depth = image(packet, packet->depth, 5); frame.motion = image(packet, packet->motion, 5); frame.output = image(packet, packet->output, 7);
    frame.motion_scale_x = -float(camera.render_width) * pass.motion_to_uv[0];
    frame.motion_scale_y = -float(camera.render_height) * pass.motion_to_uv[1]; frame.pre_exposure = 1;
    return frame;
}
}
namespace rsf::godot_vk {
bool booted() { return rsf_godot_vk_booted() != 0; }
void render_begin(uint64_t frame) {
    rsf_latencyvk_render(frame);
    std::lock_guard lock(guard); render_source_frame=frame;
}
void capture_present(const void* info, int32_t result, uint32_t begin) {
#if defined(RSF_FRAME_DUMP_VULKAN)
    // Provider-owned Present already captures the application image outside its SDK call.
    // Nested SDK WSI calls must not re-enter that owner's lock or capture generated frames.
    std::unique_lock lock(guard, std::try_to_lock);
    if (lock.owns_lock()) capture_present_impl(nullptr, info, result, begin);
#else
    (void)info; (void)result; (void)begin;
#endif
}

uint32_t leaf_supported() { return rsfg_vulkan_supported() ? 1u : 0u; }
uint32_t leaf_latency_facts(rsf_godot_vk_latency_facts* facts) try {
    rsf_latencyvk_status latency{sizeof(latency), RSF_LATENCYVK_ABI_VERSION};
    if (rsf_latencyvk_get_status(&latency) != RSF_BACKEND_OK) return 0;
    facts->low_latency_available = latency.low_latency_available; facts->report_available = latency.report_available;
    facts->pcl_message = latency.pcl_message; facts->mode = latency.mode; facts->owner = latency.owner;
    facts->sleep_calls = latency.sleep_calls; facts->marker_calls = latency.marker_calls;
    return 1;
} catch (...) { return 0; }
int32_t leaf_locked(int32_t (*body)(void*), void* user) try {
    std::lock_guard lock(guard);
    return body(user);
} catch (...) { return 0; }
int32_t leaf_start(const rsf_godot_setup* setup) try {
    // The session cleared its primary window just before this leaf.
    render_source_frame = 0;
    directory = std::string(setup->runtime_directory) + "/fidelityfx-vulkan";
    fsr4_directory = std::string(setup->runtime_directory) + "/fidelityfx";
    logger = setup->log; logger_user = setup->log_user;
    streamline_directory = std::string(setup->runtime_directory) + "/streamline-vulkan";
    const rsf_slvk_setup latency{sizeof(latency), RSF_SLVK_ABI_VERSION, streamline_directory.c_str(), setup->project_id, RSF_SLVK_DLSS_SR | RSF_SLVK_DLSS_FG};
    const auto latency_result = rsf_slvk_initialize(&latency);
    const std::string xess = std::string(setup->runtime_directory) + "/xess";
    const int intel_length = MultiByteToWideChar(CP_UTF8, MB_ERR_INVALID_CHARS, xess.c_str(), -1, nullptr, 0);
    if (intel_length <= 1) return 0;
    intel_directory.resize(static_cast<size_t>(intel_length));
    if (!MultiByteToWideChar(CP_UTF8, MB_ERR_INVALID_CHARS, xess.c_str(), -1, intel_directory.data(), intel_length)) return 0;
    const rsf_xessvk_setup xess_setup{sizeof(xess_setup), RSF_XESSVK_REQUIREMENTS_ABI_VERSION, xess.c_str()};
    const auto xess_result = rsf_xessvk_initialize(&xess_setup);
    rsf_godot_vk_proc base = nullptr;
    if (rsf_latencyvk_instance_proc(nullptr, "vkCreateInstance")) base = reinterpret_cast<rsf_godot_vk_proc>(rsf_latencyvk_instance_proc);
    else {
        // Keep the loader mapped through late Vulkan destruction, even when
        // the optional Streamline session could not be initialized.
        static HMODULE vulkan = LoadLibraryExW(L"vulkan-1.dll", nullptr, LOAD_LIBRARY_SEARCH_SYSTEM32);
        if (vulkan) base = reinterpret_cast<rsf_godot_vk_proc>(GetProcAddress(vulkan, "vkGetInstanceProcAddr"));
    }
    resolver = creation_resolver(base, xess_result == RSF_BACKEND_OK, setup->log, setup->log_user);
    if (setup->log) setup->log(setup->log_user, latency_result == RSF_BACKEND_OK ?
        "Godot Vulkan host ready with Streamline Vulkan dispatch and PCL startup." : "Godot Vulkan host ready. Streamline Vulkan initialization failed; platform latency capabilities are queried at device creation.");
    return 1;
} catch (...) { return 0; }
int32_t leaf_stop() try {
    // The owner already disabled and drained the presenter and retired the SR contexts.
#if defined(RSF_FRAME_DUMP_VULKAN)
    if (release_capture(dump_setup.user) != RSF_BACKEND_OK) return 0;
#endif
    if (rsf_xessvk_shutdown() != RSF_BACKEND_OK) return 0;
    stop_creation_requirements();
    // Scene teardown precedes the engine's swapchain destruction. A DLSS owner
    // still needs the live SDK to unregister captures and release its feature.
    sdk_shutdown_pending = presentation != nullptr;
    if (!sdk_shutdown_pending && rsf_latencyvk_shutdown() != RSF_BACKEND_OK) return 0;
    return 1;
} catch (...) { return 0; }
void leaf_release(uint64_t session_id) try {
    rsf_input_dump_cancel(session_id);
    directory.clear();
} catch (...) {}
void leaf_facts(rsf_godot_vk_facts* facts) try {
    *facts = {};
    facts->graph_handoff = graph_handoff_available ? 1u : 0u;
#if defined(RSF_VULKAN_FSR4_ENABLED)
    facts->fsr4_sr_compiled = 1;
#endif
#if defined(RSF_VULKAN_FSR4_FG_ENABLED)
    facts->fsr4_fg_compiled = 1;
#endif
#if defined(RSF_VULKAN_XESS_FG_ENABLED)
    facts->xess_fg_compiled = 1;
#endif
    facts->presenter = presentation ? 1u : 0u;
    uint32_t owned_backend = 0;
    if (presentation) (void)rsf_fgvk_backend(presentation, &owned_backend);
    facts->presenter_backend = owned_backend;
    facts->sdk_shutdown_pending = sdk_shutdown_pending ? 1u : 0u;
} catch (...) {}
int32_t leaf_presenter_max(uint32_t* count) try {
    return presentation ? rsf_fgvk_max_generated(presentation, count) : RSF_BACKEND_ERROR_NOT_READY;
} catch (...) { return RSF_BACKEND_ERROR_FEATURE_FAILED; }
uint32_t leaf_fsr4_fg_probe() try {
#if defined(RSF_VULKAN_FSR4_FG_ENABLED)
    auto probe = presentation_graphics;
    probe.runtime_directory_utf8 = fsr4_directory.c_str();
    rsf_fsr4fg_info info{sizeof(info), RSF_FSR4FG_BRIDGE_ABI_VERSION};
    return rsf_fsr4gvk_probe(&probe, &info) == 0 ? info.max_generated : 0u;
#else
    return 0;
#endif
} catch (...) { return 0; }
int32_t leaf_latency_options(uint32_t mode, uint32_t frame_limit_us) try {
    return rsf_latencyvk_options(mode, frame_limit_us);
} catch (...) { return RSF_BACKEND_ERROR_FEATURE_FAILED; }

// SR context leaves for native/vulkan/sr.rs. Each runs with the guard held and decides nothing.
int32_t leaf_presenter_drain(uint32_t disable) try {
    if (!presentation) return RSF_BACKEND_OK;
    if (disable) (void)rsf_fgvk_enable(presentation, 0);
    return rsf_fgvk_drain(presentation);
} catch (...) { return RSF_BACKEND_ERROR_FEATURE_FAILED; }
uint32_t leaf_device_idle(void* device, rsf_godot_vk_proc device_proc) try {
    using WaitIdle = int32_t (*)(void*);
    auto wait = device_proc ? reinterpret_cast<WaitIdle>(device_proc(device, "vkDeviceWaitIdle")) : nullptr;
    return wait && wait(device) == 0 ? 1u : 0u;
} catch (...) { return 0; }
int32_t leaf_sr_destroy(void* context) try {
    return rsf_srvk_destroy(static_cast<rsf_srvk*>(context));
} catch (...) { return RSF_BACKEND_ERROR_FEATURE_FAILED; }
int32_t leaf_fsr4_destroy(void* context) try {
#if defined(RSF_VULKAN_FSR4_ENABLED)
    return rsf_fsr4vk_destroy(static_cast<rsf_fsr4vk*>(context));
#else
    (void)context; // Never created without the provider.
    return 0;
#endif
} catch (...) { return RSF_BACKEND_ERROR_FEATURE_FAILED; }
uint32_t leaf_sr_create(void* call, const rsf_godot_vk_sr_create* request, rsf_godot_vk_sr_context* created) try {
    auto setup = sr_setup(pass_of(call), *request);
    rsf_srvk* context = nullptr;
    created->result = rsf_srvk_create(&setup, &context);
    created->handle = context;
    return 1;
} catch (...) { return 0; }
uint32_t leaf_fsr4_create(void* call, const rsf_godot_vk_sr_create* request, rsf_godot_vk_sr_context* created) try {
#if defined(RSF_VULKAN_FSR4_ENABLED)
    const auto& value = pass_of(call);
    const auto* handoff = value.handoff;
    auto setup = sr_setup(value, *request);
    setup.runtime_directory_utf8 = fsr4_directory.c_str();
    const rsf_fsr4vk_handoff exchange{sizeof(exchange), 1, handoff->timeline_enabled, handoff->queue_family,
        handoff->user, handoff->split_record};
    rsf_fsr4vk* context = nullptr;
    const int code = rsf_fsr4vk_create(&setup, &exchange, &context);
    created->handle = context;
    created->result = code == 0 ? RSF_BACKEND_OK : RSF_BACKEND_ERROR_INIT_FAILED;
    if (logger) {
        char message[160]; std::snprintf(message, sizeof(message), "Godot Vulkan FSR4 interop context: create=%d retained=%u", code, context ? 1u : 0u);
        logger(logger_user, message);
    }
#else
    (void)call; (void)request;
    created->result = RSF_BACKEND_ERROR_NOT_COMPILED;
#endif
    return 1;
} catch (...) { return 0; }
uint32_t leaf_dlss_preset(void* context, uint32_t preset) try {
    (void)rsf_srvk_dlss_preset(static_cast<rsf_srvk*>(context), preset);
    return 1;
} catch (...) { return 0; }
uint32_t leaf_sr_plan(void* call, void* context, uint32_t* width, uint32_t* height) try {
    auto& value = pass_of(call);
    uint32_t planned_width = 0, planned_height = 0;
    (void)rsf_srvk_plan(static_cast<rsf_srvk*>(context), &planned_width, &planned_height);
    *width = value.plan_width = planned_width; *height = value.plan_height = planned_height;
    return 1;
} catch (...) { return 0; }
uint32_t leaf_fsr4_plan(void* call, void* context, uint32_t* width, uint32_t* height) try {
    auto& value = pass_of(call);
    uint32_t planned_width = 0, planned_height = 0;
#if defined(RSF_VULKAN_FSR4_ENABLED)
    (void)rsf_fsr4vk_plan(static_cast<rsf_fsr4vk*>(context), &planned_width, &planned_height);
#else
    (void)context;
#endif
    *width = value.plan_width = planned_width; *height = value.plan_height = planned_height;
    return 1;
} catch (...) { return 0; }
uint32_t leaf_sr_evaluate(void* call, void* context, const rsf_sharpening_options* sharpening, int32_t* result) try {
    auto& value = pass_of(call);
    value.frame = sr_frame(value, *sharpening);
    *result = rsf_srvk_evaluate(static_cast<rsf_srvk*>(context), value.commands, &value.frame);
    return 1;
} catch (...) { return 0; }
uint32_t leaf_fsr4_evaluate(void* call, void* context, const rsf_sharpening_options* sharpening, int32_t* result,
    void** next) try {
    auto& value = pass_of(call);
    value.frame = sr_frame(value, *sharpening);
#if defined(RSF_VULKAN_FSR4_ENABLED)
    void* following = value.commands;
    const int code = rsf_fsr4vk_evaluate(static_cast<rsf_fsr4vk*>(context), value.commands, &value.frame, &following);
    value.commands = following; *value.continuation = following;
    value.fsr4_code = code;
    *result = code == 0 ? RSF_BACKEND_OK :
        (code == RSF_FSR4_BRIDGE_INVALID_ARGUMENT ? RSF_BACKEND_ERROR_INVALID_ARGUMENT : RSF_BACKEND_ERROR_FEATURE_FAILED);
#else
    (void)context;
    *result = RSF_BACKEND_ERROR_NOT_COMPILED;
#endif
    *next = value.commands;
    return 1;
} catch (...) { return 0; }
uint32_t leaf_sr_fallback(void* call, int32_t* result) try {
    const auto& value = pass_of(call);
    const auto* packet = value.packet;
    rsf_srvk_fallback fallback{}; fallback.struct_size = sizeof(fallback); fallback.abi_version = RSF_SRVK_ABI_VERSION;
    fallback.instance = packet->instance; fallback.physical_device = packet->physical_device; fallback.device = packet->device;
    fallback.instance_proc = reinterpret_cast<rsf_vk_get_proc>(packet->instance_proc);
    fallback.device_proc = reinterpret_cast<rsf_vk_get_proc>(packet->device_proc); fallback.command_buffer = value.commands;
    fallback.color = image(packet, packet->color, 5); fallback.output = image(packet, packet->output, 7);
    *result = rsf_srvk_spatial_fallback(&fallback);
    return 1;
} catch (...) { return 0; }
uint32_t leaf_sr_log(void* call, uint32_t kind, int32_t provider_result, int32_t fallback_result) try {
    if (!logger) return 1;
    const auto& value = pass_of(call);
    if (kind == RSF_GODOT_VK_LOG_REJECTED) {
        char message[128]; std::snprintf(message, sizeof(message), "Vulkan SR rejected: provider=%d spatial fallback=%d", provider_result, fallback_result);
        logger(logger_user, message);
    } else if (kind == RSF_GODOT_VK_LOG_FSR4_DISPATCH) {
        char message[160]; std::snprintf(message, sizeof(message), "Godot Vulkan FSR4 dispatch: result=%d continuation=%u", value.fsr4_code, value.commands ? 1u : 0u);
        logger(logger_user, message);
    } else if (kind == RSF_GODOT_VK_LOG_INPUT_REFUSAL) {
        const auto& record = *value.record;
        const auto& frame = value.frame;
        char message[512];
        std::snprintf(message, sizeof(message), "Vulkan SR input refusal: frame=%llu render=%ux%u plan=%ux%u output=%ux%u formats=%u/%u/%u/%u sizes=%ux%u/%ux%u/%ux%u/%ux%u ms=%.3f near=%.3f far=%.3f fov=%.3f",
            static_cast<unsigned long long>(record.frame_id), record.render_width, record.render_height, value.plan_width, value.plan_height, record.output_width, record.output_height,
            frame.color.format, frame.depth.format, frame.motion.format, frame.output.format,
            frame.color.width, frame.color.height, frame.depth.width, frame.depth.height, frame.motion.width, frame.motion.height, frame.output.width, frame.output.height,
            record.frame_time_ms, record.camera.near_plane, record.camera.far_plane, record.camera.vertical_fov_radians);
        logger(logger_user, message);
    }
    return 1;
} catch (...) { return 0; }
uint32_t leaf_sr_dump_result(void* call, uint32_t backend, int32_t result) try {
    rsf_input_dump_result(pass_of(call).record, "sr", backend, result);
    return 1;
} catch (...) { return 0; }
}
extern "C" rsf_godot_vk_proc rsfg_vk_resolver() {
    return resolver;
}
extern "C" int rsfg_vk_host_capabilities(uint32_t flags) {
    std::lock_guard lock(guard);
    if (rsf::godot_vk::booted() || (flags & ~RSF_GODOT_VK_GRAPH_HANDOFF)) return 0;
    graph_handoff_available = (flags & RSF_GODOT_VK_GRAPH_HANDOFF) != 0;
    return 1;
}
extern "C" int rsfg_vulkan_supported(void) {
#if defined(RSF_VULKAN_SR_ENABLED)
    return 1;
#else
    return 0;
#endif
}
extern "C" int rsfg_boot_vulkan(const rsf_godot_setup* setup) try {
    return rsf_godot_boot_vulkan(rsf_godot_platform_leaves(), setup);
} catch (...) { return 0; }
extern "C" int32_t rsfg_vk_create(const rsf_fgvk_setup* setup, void** result, uint64_t* chain) try {
    if (!rsf::godot_vk::booted() || !setup || setup->struct_size < sizeof(*setup) || !result || !chain) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    std::lock_guard lock(guard);
    if (presentation) return RSF_BACKEND_ERROR_NOT_SUPPORTED;
    rsf_godot_vk_view view{};
    (void)rsf_godot_vk_view_get(&view);
    const auto& requested = view.settings;
    const uint32_t selected = view.fg_backend;
    auto value = *setup;
#if defined(RSF_FRAME_DUMP_VULKAN)
    VkSwapchainCreateInfoKHR readable{};
    if (setup->create_info) {
        readable = *static_cast<const VkSwapchainCreateInfoKHR*>(setup->create_info);
        VkSurfaceCapabilitiesKHR capabilities{};
        const auto get = reinterpret_cast<PFN_vkGetPhysicalDeviceSurfaceCapabilitiesKHR>(setup->graphics.instance_proc(setup->graphics.instance, "vkGetPhysicalDeviceSurfaceCapabilitiesKHR"));
        if (get && get(static_cast<VkPhysicalDevice>(setup->graphics.physical_device), readable.surface, &capabilities) == VK_SUCCESS &&
            (capabilities.supportedUsageFlags & VK_IMAGE_USAGE_TRANSFER_SRC_BIT)) readable.imageUsage |= VK_IMAGE_USAGE_TRANSFER_SRC_BIT;
        value.create_info = &readable;
        setup_capture(value);
    }
#endif
    value.graphics.runtime_directory_utf8 = selected == 1 ? streamline_directory.c_str() :
        selected == 4 ? fsr4_directory.c_str() : directory.c_str();
    const rsf_fgvk_host_options host{sizeof(host), RSF_FGVK_HOST_ABI_VERSION, rsf_godot_vk_primary_window(),
        rsf::godot_vk::interop_device_ready() ? 1u : 0u, reinterpret_cast<const uint16_t*>(intel_directory.c_str())};
    auto code = rsf_fgvk_create_with_host(&value, selected, requested.generated_frames, &host, &presentation, chain);
    *result = presentation;
    const bool ready = code == RSF_BACKEND_OK && presentation;
    uint32_t maximum = 0;
    if (ready) (void)rsf_fgvk_max_generated(presentation, &maximum);
    rsf_godot_vk_presenter_created(ready ? 1u : 0u, selected, maximum);
    if (ready) {
        presentation_graphics = setup->graphics;
        presentation_graphics.runtime_directory_utf8 = nullptr;
        (void)rsf_latencyvk_presentation(presentation);
        (void)rsf_latencyvk_options(requested.latency_mode, requested.frame_limit_us);
    }
    if (logger) {
        char message[128]; std::snprintf(message, sizeof(message), "Godot Vulkan swapchain owner: create=%d reserved queues=%u", code, presentation ? 4u : 0u);
        logger(logger_user, message);
    }
    return code;
} catch (...) { return RSF_BACKEND_ERROR_FEATURE_FAILED; }
extern "C" void rsfg_vk_capture_setup(const rsf_fgvk_setup* setup) try {
#if defined(RSF_FRAME_DUMP_VULKAN)
    if (!rsf::godot_vk::booted() || !setup || setup->struct_size < sizeof(*setup) || !setup->graphics.instance_proc || !setup->graphics.device_proc) return;
    std::lock_guard lock(guard); setup_capture(*setup);
#else
    (void)setup;
#endif
} catch (...) {}
extern "C" int32_t rsfg_vk_capture_release(void* user) try {
#if defined(RSF_FRAME_DUMP_VULKAN)
    std::lock_guard lock(guard); return release_capture(user);
#else
    (void)user; return 0;
#endif
} catch (...) { return RSF_BACKEND_ERROR_FEATURE_FAILED; }
extern "C" int32_t rsfg_vk_images(void* value, uint32_t* count, uint64_t* images) { return rsf_fgvk_images(static_cast<rsf_fgvk*>(value), count, images); }
extern "C" int32_t rsfg_vk_acquire(void* value, uint64_t timeout, uint64_t semaphore, uint64_t fence, uint32_t* index) {
    std::lock_guard lock(guard);
    uint32_t owned_backend = 0;
    if (rsf_fgvk_backend(static_cast<rsf_fgvk*>(value), &owned_backend) != RSF_BACKEND_OK) return -3;
    // The session asks the engine to drain and recreate the complete WSI owner on a provider
    // switch, or on a new SR plan for an interop owner whose guides are fixed to the old one.
    if (!rsf_godot_vk_presenter_current(owned_backend)) return -1000001004; // VK_ERROR_OUT_OF_DATE_KHR
    return rsf_fgvk_acquire(static_cast<rsf_fgvk*>(value), timeout, semaphore, fence, index);
}
extern "C" uint32_t rsfg_vk_image_layout(void* value) {
    uint32_t layout = 1000001002u; // VK_IMAGE_LAYOUT_PRESENT_SRC_KHR
    if (value) (void)rsf_fgvk_image_layout(static_cast<rsf_fgvk*>(value), &layout);
    return layout;
}
extern "C" int32_t rsfg_vk_present(void* value, const void* info) try {
    std::lock_guard lock(guard);
    auto* target = static_cast<rsf_fgvk*>(value);
    rsf_latencyvk_submit_end(render_source_frame);
#if defined(RSF_FRAME_DUMP_VULKAN)
    capture_present_impl(target, info, 0, 1);
#endif
    auto result = rsf_fgvk_present(target, info);
#if defined(RSF_FRAME_DUMP_VULKAN)
    capture_present_impl(target, info, result, 0);
#endif
    rsf_fgvk_status status{sizeof(status), RSF_FGVK_ABI_VERSION};
    if (rsf_fgvk_get_status(target, &status) == RSF_BACKEND_OK) {
        const rsf_godot_vk_present_facts facts{status.enabled, status.result, status.generated_callbacks,
            status.last_present_count, status.tagged};
        rsf_godot_vk_presented(&facts);
    }
    return result;
} catch (...) { return -3; }
extern "C" void rsfg_vk_recording_begin(void* commands) try {
    std::lock_guard lock(guard);
    void* context = nullptr;
    void* fsr4_context = nullptr;
    rsf_godot_vk_sr_contexts(&context, &fsr4_context);
    if (context) rsf_srvk_retire_commands(static_cast<rsf_srvk*>(context),commands);
#if defined(RSF_VULKAN_FSR4_ENABLED)
    if (fsr4_context) rsf_srvk_fsr4_retire_commands(static_cast<rsf_fsr4vk*>(fsr4_context),commands);
#endif
} catch (...) {}
extern "C" void rsfg_vk_submitted(void* const* commands, uint32_t count) try {
    std::lock_guard lock(guard); if (presentation) rsf_fgvk_submitted(presentation, commands, count);
} catch (...) {}
extern "C" int32_t rsfg_vk_destroy(void* value) try {
    std::lock_guard lock(guard);
    auto* target = static_cast<rsf_fgvk*>(value);
#if defined(RSF_FRAME_DUMP_VULKAN)
    if (target == presentation && release_capture(dump_setup.user) != RSF_BACKEND_OK) return RSF_BACKEND_ERROR_NOT_READY;
#endif
    auto result = rsf_fgvk_destroy(target);
    if (result == RSF_BACKEND_OK && presentation == target) {
        presentation = nullptr;
        presentation_graphics = {};
        rsf_godot_vk_presenter_retired();
        if (sdk_shutdown_pending) {
            if (rsf_latencyvk_shutdown() == RSF_BACKEND_OK) sdk_shutdown_pending = false;
            else if (logger) logger(logger_user, "Godot Vulkan SDK shutdown refused after owner retirement; retaining the mapped session.");
        }
    }
    return result;
} catch (...) { return RSF_BACKEND_ERROR_FEATURE_FAILED; }
extern "C" int32_t rsfg_vk_drain(void* value) try {
    std::lock_guard lock(guard);
    return rsf_fgvk_drain(static_cast<rsf_fgvk*>(value));
} catch (...) { return RSF_BACKEND_ERROR_FEATURE_FAILED; }
namespace {
int render_vulkan_impl(void* commands, const rsf_godot_vk_packet* packet,
    const rsf_godot_vk_handoff* handoff, void** continuation) try {
    (void)handoff;
    if (continuation) *continuation = commands;
    if (!rsf::godot_vk::booted() || !commands || !packet || packet->struct_size < sizeof(*packet) || packet->abi_version != RSF_GODOT_ABI_VERSION ||
        !packet->device || !packet->instance_proc || !packet->device_proc || packet->pass.struct_size < sizeof(packet->pass) ||
        (packet->pass.role != RSF_GAME_RENDER_SR && packet->pass.role != RSF_GAME_RENDER_FINAL_SCENE) || !packet->pass.camera_valid) return 0;
    std::lock_guard lock(guard);
    const auto& pass = packet->pass;
    if (rsf_game_depth_domains_requested(&pass.depth_domains)) return 0;
    rsf_godot_vk_view view{};
    (void)rsf_godot_vk_view_get(&view);
    const auto& camera = pass.camera;
    rsf_frame_record record{}; record.struct_size = sizeof(record); record.abi_version = RSF_GAME_FRAME_ABI_VERSION;
    record.session_id = pass.session_id; record.frame_id = pass.source_frame_id; record.view_id = static_cast<uint32_t>(pass.view_key);
    record.resource_generation = pass.resource_generation; record.camera = camera; record.frame_time_ms = camera.frame_time_seconds * 1000;
    record.render_width = camera.render_width; record.render_height = camera.render_height;
    record.output_width = camera.output_width; record.output_height = camera.output_height;
    record.flags = pass.flags & RSF_GAME_RENDER_RESET ? RSF_FRAME_FLAG_RESET : 0;
#if defined(RSF_FRAME_DUMP_VULKAN)
    dump_record = record;
    if (pass.role == RSF_GAME_RENDER_SR) (void)rsf_input_dump_begin_record(&record, view.settings.sr_backend);
    (void)dump_stages.wants(record.frame_id); // Poll completed snapshots even after the selected frame.
    if (rsf_input_dump_wants(&record)) {
        rsf_input_dump_pass_metadata(&pass);
        if (pass.role == RSF_GAME_RENDER_FINAL_SCENE)
            (void)dump_stages.record(static_cast<VkCommandBuffer>(commands), image(packet, packet->color, 5), record, "hudless_color", record.flags);
    }
#endif
    if (presentation) {
        rsf_godot_vk_fg_pass fg_pass{pass.role, pass.screen, record.session_id, record.view_id, record.render_width, record.render_height};
        rsf_godot_vk_fg_begin(&fg_pass);
        fg_pass.options_result = rsf_fgvk_enable_options(presentation, fg_pass.enabled, fg_pass.generated_frames);
        rsf_fgvk_frame fg{}; fg.struct_size = sizeof(fg); fg.abi_version = RSF_FGVK_ABI_VERSION; fg.record = &record;
        fg.depth = image(packet, packet->depth, 5); fg.motion = image(packet, packet->motion, 5); fg.hudless = image(packet, packet->color, 5);
        fg.motion_scale_x = -float(camera.render_width) * pass.motion_to_uv[0]; fg.motion_scale_y = -float(camera.render_height) * pass.motion_to_uv[1];
        fg_pass.backend_valid = rsf_fgvk_backend(presentation, &fg_pass.backend) == RSF_BACKEND_OK ? 1u : 0u;
        if (rsf_godot_vk_fg_admit(&fg_pass)) {
            const auto recorded = pass.role == RSF_GAME_RENDER_SR ? rsf_fgvk_prepare(presentation, commands, &fg) :
                rsf_fgvk_hudless(presentation, commands, &fg);
            rsf_godot_vk_fg_recorded(&fg_pass, recorded);
        }
    }
    if (pass.role == RSF_GAME_RENDER_FINAL_SCENE) {
        rsf_godot_vk_view after{};
        (void)rsf_godot_vk_view_get(&after);
        return after.fg_result == RSF_BACKEND_OK ? 1 : 0;
    }
    // The SR context owner (native/vulkan/sr.rs) retires, creates, plans and evaluates through the
    // leaves below this call, takes the spatial fallback and reports the outcome once.
    sr_call call{commands, continuation, packet, handoff, &record, {}, 0, 0, 0};
    rsf_godot_vk_sr_pass sr{};
    sr.call = &call; sr.commands = commands; sr.device = packet->device; sr.device_proc = packet->device_proc;
    sr.output_width = camera.output_width; sr.output_height = camera.output_height;
    sr.source_frame_id = pass.source_frame_id;
    sr.render_width = camera.render_width; sr.render_height = camera.render_height;
    sr.handoff = handoff ? 1u : 0u; sr.split_record = handoff && handoff->split_record ? 1u : 0u;
    sr.continuation = continuation ? 1u : 0u;
#if defined(RSF_VULKAN_FSR4_ENABLED)
    sr.fsr4_compiled = 1;
#endif
    return rsf_godot_vk_sr_render(rsf_godot_platform_leaves(), &sr);
} catch (...) { return 0; }
} // namespace
extern "C" int rsfg_render_vulkan(void* commands, const rsf_godot_vk_packet* packet) {
    return render_vulkan_impl(commands, packet, nullptr, nullptr);
}
extern "C" int rsfg_render_vulkan_handoff(void* commands, const rsf_godot_vk_packet* packet,
    const rsf_godot_vk_handoff* handoff, void** continuation) {
    if (continuation) *continuation = commands;
    if (!handoff || handoff->struct_size < sizeof(*handoff) || handoff->abi_version != RSF_GODOT_VK_HANDOFF_ABI_VERSION || !continuation)
        return 0;
    return render_vulkan_impl(commands, packet, handoff, continuation);
}
