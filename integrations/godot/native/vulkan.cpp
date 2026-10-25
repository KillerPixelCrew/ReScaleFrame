// SPDX-License-Identifier: GPL-3.0-only
#include "vulkan_control.h"
#include "vulkan_creation.h"
#include <rescaleframe/native_sr_vulkan.h>
#include <rescaleframe/native_latency_vulkan.h>
#include <rescaleframe/native_streamline_vulkan.h>
#include <rescaleframe/native_xess_vulkan_requirements.h>
#if defined(RSF_VULKAN_FSR4_ENABLED)
#include "../../../runtime/orchestrator/src/native_sr_fsr4_vulkan.h"
#else
struct rsf_fsr4vk;
#endif
#include <windows.h>
#include <mutex>
#include <string>
#include <cstdio>
#include <atomic>

namespace {
std::mutex guard;
std::atomic<bool> active{false};
std::string directory;
std::string streamline_directory;
std::string fsr4_directory;
std::wstring intel_directory;
void* primary_window = nullptr;
uint64_t render_source_frame = 0;
rsf_godot_settings requested{sizeof(requested), RSF_GODOT_ABI_VERSION, 7, 1, 0, 7, 0, 1, 0, 0, 1};
rsf_godot_status current{sizeof(current), RSF_GODOT_ABI_VERSION};
rsf_srvk* context = nullptr;
rsf_fsr4vk* fsr4_context = nullptr;
bool context_ready = false;
void* device = nullptr;
rsf_godot_vk_proc device_proc = nullptr;
uint64_t generation = 1, context_generation = 0;
uint64_t presentation_generation = 0;
uint32_t context_backend = 0;
rsf_game_log_fn logger = nullptr;
void* logger_user = nullptr;
uint32_t diagnostics = 0;
rsf_fgvk* presentation = nullptr;
rsf_godot_vk_proc resolver = nullptr;
bool sdk_shutdown_pending = false;
bool graph_handoff_available = false;
uint32_t fg_backend(const rsf_godot_settings& value) { return value.fg_backend == 7 ? 3u : value.fg_backend; }
bool retire() {
    if (!context && !fsr4_context) return true;
    if (presentation && rsf_fgvk_drain(presentation) != RSF_BACKEND_OK) return false;
    using WaitIdle = int32_t (*)(void*);
    auto wait = device_proc ? reinterpret_cast<WaitIdle>(device_proc(device, "vkDeviceWaitIdle")) : nullptr;
    if (!wait || wait(device) != 0) return false;
#if defined(RSF_VULKAN_FSR4_ENABLED)
    if (fsr4_context && rsf_fsr4vk_destroy(fsr4_context) != 0) return false;
#endif
    fsr4_context = nullptr;
    if (context && rsf_srvk_destroy(context) != RSF_BACKEND_OK) return false;
    context = nullptr;
    context_ready = false;
    return true;
}
}
namespace rsf::godot_vk {
bool booted() { return active; }
void window(void* value) { std::lock_guard lock(guard); primary_window = value; }
int configure(const rsf_godot_settings* value) {
    // These paths are added independently. Do not accept unavailable FG or
    // silently substitute an FSR3 provider for a requested DLSS/XeSS/FSR4 path.
    if (!active ||
        !(value->sr_backend == 0 || value->sr_backend == 1 || value->sr_backend == 2 || value->sr_backend == 3 || value->sr_backend == 4 || value->sr_backend == 5 || value->sr_backend == 6 || value->sr_backend == 7)) return 0;
    std::lock_guard lock(guard);
    if (value->sr_backend == 5 && !xess_device_ready()) return 0;
    if (value->sr_backend == 4) {
#if defined(RSF_VULKAN_FSR4_ENABLED)
        if (!graph_handoff_available || !interop_device_ready()) return 0;
#else
        return 0;
#endif
    }
    const auto selected_fg = fg_backend(*value);
    if (!(selected_fg == 1 || selected_fg == 3 || selected_fg == 5)) return 0;
    if (selected_fg == 5) {
#if defined(RSF_VULKAN_XESS_FG_ENABLED)
        if (!interop_device_ready() || !primary_window || value->latency_mode > 1 || (value->fg_mode && value->latency_mode != 1)) return 0;
#else
        return 0;
#endif
    }
    if (value->fg_mode && (!presentation ||
        (selected_fg == 3 && value->generated_frames != 1) || (selected_fg == 1 && !value->latency_mode))) return 0;
    uint32_t owned_backend = 0, maximum = 0;
    if (presentation) (void)rsf_fgvk_backend(presentation, &owned_backend);
    if (value->fg_mode && owned_backend == selected_fg &&
        (rsf_fgvk_max_generated(presentation, &maximum) != RSF_BACKEND_OK || value->generated_frames > maximum)) return 0;
    if (owned_backend == 5 && rsf_fgvk_latency_options(presentation, value->latency_mode, value->frame_limit_us) != RSF_BACKEND_OK) return 0;
    const bool xell_transition = selected_fg == 5 || owned_backend == 5;
    if (rsf_latencyvk_options(xell_transition ? 0u : value->latency_mode, xell_transition ? 0u : value->frame_limit_us) != RSF_BACKEND_OK) return 0;
    if (value->sr_backend != requested.sr_backend || value->quality != requested.quality || value->dlss_preset != requested.dlss_preset) ++generation;
    requested = *value;
    return 1;
}
int status(rsf_godot_status* value) {
    std::lock_guard lock(guard);
    *value = current;
    rsf_latencyvk_status latency{sizeof(latency), RSF_LATENCYVK_ABI_VERSION};
    if (rsf_latencyvk_get_status(&latency) == RSF_BACKEND_OK) {
        value->latency_available = latency.low_latency_available; value->latency_report_available = latency.report_available;
        value->pcl_available = latency.pcl_message >= 0xc000u; value->latency_mode = latency.mode;
        value->sleep_calls = latency.sleep_calls; value->marker_calls = latency.marker_calls;
    }
    uint32_t backend = 0;
    if (presentation && rsf_fgvk_backend(presentation, &backend) == RSF_BACKEND_OK && backend == 5) {
        rsf_fgvk_latency_status pacing{sizeof(pacing), RSF_FGVK_LATENCY_ABI_VERSION};
        if (rsf_fgvk_latency_get_status(presentation, &pacing) == RSF_BACKEND_OK) {
            value->latency_available = pacing.available; value->latency_mode = pacing.mode;
            value->sleep_calls = pacing.sleep_calls;
        }
    }
    return active ? 1 : 0;
}
int settings(rsf_godot_settings* value) {
    std::lock_guard lock(guard); *value = requested; return active ? 1 : 0;
}
int shutdown() {
    std::lock_guard lock(guard);
    if (presentation) {
        (void)rsf_fgvk_enable(presentation, 0);
        if (rsf_fgvk_drain(presentation) != RSF_BACKEND_OK) return 0;
    }
    if (!retire()) return 0;
    if (rsf_xessvk_shutdown() != RSF_BACKEND_OK) return 0;
    stop_creation_requirements();
    // Scene teardown precedes the engine's swapchain destruction. A DLSS owner
    // still needs the live SDK to unregister captures and release its feature.
    sdk_shutdown_pending = presentation != nullptr;
    if (!sdk_shutdown_pending && rsf_latencyvk_shutdown() != RSF_BACKEND_OK) return 0;
    active = false; current.active_sr = 0; current.sr_result = RSF_BACKEND_ERROR_NOT_READY;
    device = nullptr; device_proc = nullptr; directory.clear();
    return 1;
}
void cpu(const rsf_game_cpu_event* event) {
    rsf_latencyvk_cpu(event);
    if (!event || event->struct_size < sizeof(*event)) return;
    std::lock_guard lock(guard);
    uint32_t backend = 0;
    if (!presentation || rsf_fgvk_backend(presentation, &backend) != RSF_BACKEND_OK || backend != 5) return;
    if (event->stage == RSF_GAME_CPU_PACING && rsf_fgvk_latency_sleep(presentation, event->source_frame_id) == RSF_BACKEND_OK)
        (void)rsf_fgvk_latency_marker(presentation, event->source_frame_id, 0);
    if (event->stage == RSF_GAME_CPU_SIMULATION_END) (void)rsf_fgvk_latency_marker(presentation, event->source_frame_id, 1);
    if (event->stage == RSF_GAME_CPU_INPUT_SAMPLE) (void)rsf_fgvk_latency_marker(presentation, event->source_frame_id, 6);
}
void render_begin(uint64_t frame) {
    rsf_latencyvk_render(frame);
    std::lock_guard lock(guard); render_source_frame = frame;
    uint32_t backend = 0;
    if (presentation && rsf_fgvk_backend(presentation, &backend) == RSF_BACKEND_OK && backend == 5)
        (void)rsf_fgvk_latency_marker(presentation, frame, 2);
}
int input(uint64_t frame, uint32_t kind, uint32_t message) { return rsf_latencyvk_input(frame, kind, message) == RSF_BACKEND_OK ? 1 : 0; }
uint32_t ping_message() {
    rsf_latencyvk_status latency{sizeof(latency), RSF_LATENCYVK_ABI_VERSION};
    return rsf_latencyvk_get_status(&latency) == RSF_BACKEND_OK ? latency.pcl_message : 0;
}
}
extern "C" rsf_godot_vk_proc rsfg_vk_resolver() {
    return resolver;
}
extern "C" int rsfg_vk_host_capabilities(uint32_t flags) {
    std::lock_guard lock(guard);
    if (active || (flags & ~RSF_GODOT_VK_GRAPH_HANDOFF)) return 0;
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
    if (!setup || setup->struct_size < sizeof(*setup) || setup->abi_version != RSF_GODOT_ABI_VERSION ||
        !setup->runtime_directory || !rsfg_vulkan_supported()) return 0;
    std::lock_guard lock(guard);
    if (active) return 0;
    if (sdk_shutdown_pending) return 0;
    requested = {sizeof(requested), RSF_GODOT_ABI_VERSION, 7, 1, 0, 7, 0, 1, 0, 0, 1};
    primary_window = nullptr; render_source_frame = 0;
    directory = std::string(setup->runtime_directory) + "/fidelityfx-vulkan";
    fsr4_directory = std::string(setup->runtime_directory) + "/fidelityfx";
    logger = setup->log; logger_user = setup->log_user;
    current = {sizeof(current), RSF_GODOT_ABI_VERSION};
    current.session_id = GetTickCount64() + 1; diagnostics = 0;
    current.fg_result = RSF_BACKEND_ERROR_NOT_SUPPORTED;
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
    resolver = rsf::godot_vk::creation_resolver(base, xess_result == RSF_BACKEND_OK, setup->log, setup->log_user);
    active = true;
    if (setup->log) setup->log(setup->log_user, latency_result == RSF_BACKEND_OK ?
        "Godot Vulkan host ready with Streamline Vulkan dispatch and PCL startup." : "Godot Vulkan host ready. Streamline Vulkan initialization failed; latency is unavailable.");
    return 1;
} catch (...) { return 0; }
extern "C" int32_t rsfg_vk_create(const rsf_fgvk_setup* setup, void** result, uint64_t* chain) try {
    if (!active || !setup || setup->struct_size < sizeof(*setup) || !result || !chain) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    std::lock_guard lock(guard);
    if (presentation) return RSF_BACKEND_ERROR_NOT_SUPPORTED;
    const uint32_t selected = fg_backend(requested);
    auto value = *setup; value.graphics.runtime_directory_utf8 = selected == 1 ? streamline_directory.c_str() : directory.c_str();
    const rsf_fgvk_host_options host{sizeof(host), RSF_FGVK_HOST_ABI_VERSION, primary_window,
        rsf::godot_vk::interop_device_ready() ? 1u : 0u, reinterpret_cast<const uint16_t*>(intel_directory.c_str())};
    auto code = rsf_fgvk_create_with_host(&value, selected, requested.generated_frames, &host, &presentation, chain);
    *result = presentation;
    current.fg_available = presentation ? 1u : 0u;
    current.fg_backend = presentation ? selected : 0u;
    current.max_generated_frames = 0;
    if (presentation) (void)rsf_fgvk_max_generated(presentation, &current.max_generated_frames);
    if (presentation) {
        presentation_generation = generation;
        if (selected == 5) (void)rsf_fgvk_latency_options(presentation, requested.latency_mode, requested.frame_limit_us);
        else (void)rsf_latencyvk_options(requested.latency_mode, requested.frame_limit_us);
    }
    if (logger) {
        char message[128]; std::snprintf(message, sizeof(message), "Godot Vulkan swapchain owner: create=%d reserved queues=%u", code, presentation ? 4u : 0u);
        logger(logger_user, message);
    }
    return code;
} catch (...) { return RSF_BACKEND_ERROR_FEATURE_FAILED; }
extern "C" int32_t rsfg_vk_images(void* value, uint32_t* count, uint64_t* images) { return rsf_fgvk_images(static_cast<rsf_fgvk*>(value), count, images); }
extern "C" int32_t rsfg_vk_acquire(void* value, uint64_t timeout, uint64_t semaphore, uint64_t fence, uint32_t* index) {
    std::lock_guard lock(guard);
    uint32_t owned_backend = 0;
    if (rsf_fgvk_backend(static_cast<rsf_fgvk*>(value), &owned_backend) != RSF_BACKEND_OK) return -3;
    // Let the engine drain and recreate the complete WSI owner on a provider switch.
    if (owned_backend != fg_backend(requested)) return -1000001004; // VK_ERROR_OUT_OF_DATE_KHR
    // This interop owner's guide allocations are fixed to the active SR plan.
    if (owned_backend == 5 && presentation_generation != generation) return -1000001004;
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
    uint32_t backend = 0;
    if (rsf_fgvk_backend(target, &backend) == RSF_BACKEND_OK && backend == 5)
        (void)rsf_fgvk_latency_marker(target, render_source_frame, 3);
    auto result = rsf_fgvk_present(target, info);
    rsf_fgvk_status status{sizeof(status), RSF_FGVK_ABI_VERSION};
    if (rsf_fgvk_get_status(target, &status) == RSF_BACKEND_OK) {
        current.fg_active = status.enabled; current.fg_result = status.result; current.generated_callbacks = status.generated_callbacks;
        current.generated_frames = status.enabled ? requested.generated_frames : 0u;
        current.total_presented = status.last_present_count; current.tagged_frames = status.tagged;
    }
    return result;
} catch (...) { return -3; }
extern "C" void rsfg_vk_submitted(void* const* commands, uint32_t count) try {
    std::lock_guard lock(guard); if (presentation) rsf_fgvk_submitted(presentation, commands, count);
} catch (...) {}
extern "C" int32_t rsfg_vk_destroy(void* value) try {
    std::lock_guard lock(guard);
    auto* target = static_cast<rsf_fgvk*>(value);
    auto result = rsf_fgvk_destroy(target);
    if (result == RSF_BACKEND_OK && presentation == target) {
        presentation = nullptr; current.fg_available = current.fg_active = 0;
        current.fg_backend = current.max_generated_frames = current.generated_frames = 0;
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
    if (!active || !commands || !packet || packet->struct_size < sizeof(*packet) || packet->abi_version != RSF_GODOT_ABI_VERSION ||
        !packet->device || !packet->instance_proc || !packet->device_proc || packet->pass.struct_size < sizeof(packet->pass) ||
        (packet->pass.role != RSF_GAME_RENDER_SR && packet->pass.role != RSF_GAME_RENDER_FINAL_SCENE) || !packet->pass.camera_valid) return 0;
    std::lock_guard lock(guard);
    const auto& pass = packet->pass;
    const auto& camera = pass.camera;
    rsf_frame_record record{}; record.struct_size = sizeof(record); record.abi_version = RSF_GAME_FRAME_ABI_VERSION;
    record.session_id = pass.session_id; record.frame_id = pass.source_frame_id; record.view_id = static_cast<uint32_t>(pass.view_key);
    record.resource_generation = pass.resource_generation; record.camera = camera; record.frame_time_ms = camera.frame_time_seconds * 1000;
    record.render_width = camera.render_width; record.render_height = camera.render_height;
    record.output_width = camera.output_width; record.output_height = camera.output_height;
    record.flags = pass.flags & RSF_GAME_RENDER_RESET ? RSF_FRAME_FLAG_RESET : 0;
    auto image = [packet](const rsf_godot_vk_image& value, uint32_t layout) {
        return rsf_srvk_image{packet->device, value.image, value.format, value.width, value.height, layout, value.copy_source};
    };
    if (presentation) {
        const bool enabled = requested.fg_mode && requested.gameplay && pass.screen == RSF_SCREEN_FLIGHT;
        const auto options_result = rsf_fgvk_enable_options(presentation, enabled ? 1u : 0u, requested.generated_frames);
        rsf_fgvk_frame fg{}; fg.struct_size = sizeof(fg); fg.abi_version = RSF_FGVK_ABI_VERSION; fg.record = &record;
        fg.depth = image(packet->depth, 5); fg.motion = image(packet->motion, 5); fg.hudless = image(packet->color, 5);
        fg.motion_scale_x = -float(camera.render_width) * pass.motion_to_uv[0]; fg.motion_scale_y = -float(camera.render_height) * pass.motion_to_uv[1];
        (void)rsf_fgvk_backend(presentation, &current.fg_backend);
        current.generated_frames = enabled && options_result == RSF_BACKEND_OK ? requested.generated_frames : 0u;
        current.fg_result = options_result != RSF_BACKEND_OK ? options_result :
            (pass.role == RSF_GAME_RENDER_SR ? rsf_fgvk_prepare(presentation, commands, &fg) : rsf_fgvk_hudless(presentation, commands, &fg));
    }
    if (pass.role == RSF_GAME_RENDER_FINAL_SCENE) return current.fg_result == RSF_BACKEND_OK ? 1 : 0;
    auto fallback = [&]() {
        rsf_srvk_fallback call{}; call.struct_size = sizeof(call); call.abi_version = RSF_SRVK_ABI_VERSION;
        call.instance = packet->instance; call.physical_device = packet->physical_device; call.device = packet->device;
        call.instance_proc = reinterpret_cast<rsf_vk_get_proc>(packet->instance_proc);
        call.device_proc = reinterpret_cast<rsf_vk_get_proc>(packet->device_proc); call.command_buffer = commands;
        call.color = image(packet->color, 5); call.output = image(packet->output, 7);
        if (!commands) return 0;
        const auto result = rsf_srvk_spatial_fallback(&call);
        current.active_sr = 0; ++current.refused_frames;
        if (diagnostics++ < 4 && logger) {
            char message[128]; std::snprintf(message, sizeof(message), "Vulkan SR rejected: provider=%d spatial fallback=%d", current.sr_result, result);
            logger(logger_user, message);
        }
        return result == RSF_BACKEND_OK ? 1 : 0;
    };
    const uint32_t backend = requested.sr_backend == 7 ? 3 : requested.sr_backend;
    if ((context || fsr4_context) && (!context_ready || generation != context_generation || packet->device != device ||
        current.output_width != camera.output_width || current.output_height != camera.output_height)) {
        if (!retire()) { current.sr_result = RSF_BACKEND_ERROR_NOT_READY; return fallback(); }
    }
    current.requested_sr = requested.sr_backend;
    current.output_width = camera.output_width; current.output_height = camera.output_height;
    current.render_width = camera.render_width; current.render_height = camera.render_height;
    current.source_frame_id = pass.source_frame_id;
    if (backend == 0 || backend == 6) { current.active_sr = backend; current.sr_result = RSF_BACKEND_OK; ++current.evaluated_frames; return 1; }
    if (!context && !fsr4_context) {
        context_ready = false;
        device = packet->device; device_proc = packet->device_proc;
        rsf_srvk_setup setup{}; setup.struct_size = sizeof(setup); setup.abi_version = RSF_SRVK_ABI_VERSION;
        setup.instance = packet->instance; setup.physical_device = packet->physical_device; setup.device = packet->device;
        setup.instance_proc = reinterpret_cast<rsf_vk_get_proc>(packet->instance_proc);
        setup.device_proc = reinterpret_cast<rsf_vk_get_proc>(packet->device_proc);
        setup.backend = backend; setup.quality = requested.quality; setup.output_width = camera.output_width; setup.output_height = camera.output_height;
        setup.depth_inverted = camera.depth_inverted;
        // The pinned Godot producer supplies finite z_far; camera ABI 1 has no infinite-depth promise.
        setup.depth_infinite = 0;
        setup.motion_jittered = camera.motion_jittered; setup.runtime_directory_utf8 = directory.c_str();
        if (backend == 4) {
#if defined(RSF_VULKAN_FSR4_ENABLED)
            if (!handoff || !handoff->split_record || !continuation) { current.sr_result = RSF_BACKEND_ERROR_NOT_READY; return fallback(); }
            setup.runtime_directory_utf8 = fsr4_directory.c_str();
            const rsf_fsr4vk_handoff exchange{sizeof(exchange), 1, handoff->timeline_enabled, handoff->queue_family,
                handoff->user, handoff->split_record};
            const int code = rsf_fsr4vk_create(&setup, &exchange, &fsr4_context);
            current.sr_result = code == 0 ? RSF_BACKEND_OK : RSF_BACKEND_ERROR_INIT_FAILED;
            if (logger) {
                char message[160]; std::snprintf(message, sizeof(message), "Godot Vulkan FSR4 interop context: create=%d retained=%u", code, fsr4_context ? 1u : 0u);
                logger(logger_user, message);
            }
#else
            current.sr_result = RSF_BACKEND_ERROR_NOT_COMPILED;
#endif
        } else current.sr_result = rsf_srvk_create(&setup, &context);
        current.active_sr = 0;
        if (!context && !fsr4_context) return fallback();
        if (current.sr_result != RSF_BACKEND_OK) return fallback();
        if (backend == 1) (void)rsf_srvk_dlss_preset(context, requested.dlss_preset);
        device = packet->device; device_proc = packet->device_proc;
        context_generation = generation; context_backend = backend;
        context_ready = true;
    }
    uint32_t width = 0, height = 0;
#if defined(RSF_VULKAN_FSR4_ENABLED)
    if (fsr4_context) (void)rsf_fsr4vk_plan(fsr4_context, &width, &height); else
#endif
    (void)rsf_srvk_plan(context, &width, &height);
    current.render_width = width; current.render_height = height;
    rsf_srvk_frame frame{}; frame.struct_size = sizeof(frame); frame.abi_version = RSF_SRVK_ABI_VERSION; frame.record = &record;
    // Vulkan SHADER_READ_ONLY_OPTIMAL = 5, TRANSFER_DST_OPTIMAL = 7.
    frame.color = image(packet->color, 5); frame.depth = image(packet->depth, 5); frame.motion = image(packet->motion, 5); frame.output = image(packet->output, 7);
    frame.motion_scale_x = -float(camera.render_width) * pass.motion_to_uv[0];
    frame.motion_scale_y = -float(camera.render_height) * pass.motion_to_uv[1]; frame.pre_exposure = 1;
    if (backend == 4) {
#if defined(RSF_VULKAN_FSR4_ENABLED)
        if (!handoff || !continuation) { current.sr_result = RSF_BACKEND_ERROR_NOT_READY; return fallback(); }
        void* next = commands;
        const int code = rsf_fsr4vk_evaluate(fsr4_context, commands, &frame, &next);
        commands = next; *continuation = next;
        current.sr_result = code == 0 ? RSF_BACKEND_OK :
            (code == RSF_FSR4_BRIDGE_INVALID_ARGUMENT ? RSF_BACKEND_ERROR_INVALID_ARGUMENT : RSF_BACKEND_ERROR_FEATURE_FAILED);
        if (code && diagnostics++ < 4 && logger) {
            char message[160]; std::snprintf(message, sizeof(message), "Godot Vulkan FSR4 dispatch: result=%d continuation=%u", code, next ? 1u : 0u);
            logger(logger_user, message);
        }
#else
        current.sr_result = RSF_BACKEND_ERROR_NOT_COMPILED;
#endif
    } else current.sr_result = rsf_srvk_evaluate(context, commands, &frame);
    if (current.sr_result == RSF_BACKEND_ERROR_INVALID_ARGUMENT && diagnostics++ < 4 && logger) {
        char message[512];
        std::snprintf(message, sizeof(message), "Vulkan SR input refusal: frame=%llu render=%ux%u plan=%ux%u output=%ux%u formats=%u/%u/%u/%u sizes=%ux%u/%ux%u/%ux%u/%ux%u ms=%.3f near=%.3f far=%.3f fov=%.3f",
            static_cast<unsigned long long>(record.frame_id), record.render_width, record.render_height, width, height, record.output_width, record.output_height,
            frame.color.format, frame.depth.format, frame.motion.format, frame.output.format,
            frame.color.width, frame.color.height, frame.depth.width, frame.depth.height, frame.motion.width, frame.motion.height, frame.output.width, frame.output.height,
            record.frame_time_ms, record.camera.near_plane, record.camera.far_plane, record.camera.vertical_fov_radians);
        logger(logger_user, message);
    }
    current.active_sr = current.sr_result == RSF_BACKEND_OK ? context_backend : 0;
    if (current.sr_result != RSF_BACKEND_OK) return fallback();
    ++current.evaluated_frames;
    return 1;
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
