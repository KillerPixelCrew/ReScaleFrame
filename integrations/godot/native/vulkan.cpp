// SPDX-License-Identifier: GPL-3.0-only
#include "vulkan_control.h"
#include <rescaleframe/native_sr_vulkan.h>
#include <windows.h>
#include <mutex>
#include <string>
#include <cstdio>
#include <atomic>

namespace {
std::mutex guard;
std::atomic<bool> active{false};
std::string directory;
rsf_godot_settings requested{sizeof(requested), RSF_GODOT_ABI_VERSION, 7, 1, 0, 7, 0, 1, 0, 0, 1};
rsf_godot_status current{sizeof(current), RSF_GODOT_ABI_VERSION};
rsf_srvk* context = nullptr;
void* device = nullptr;
rsf_godot_vk_proc device_proc = nullptr;
uint64_t generation = 1, context_generation = 0;
uint32_t context_backend = 0;
rsf_game_log_fn logger = nullptr;
void* logger_user = nullptr;
uint32_t diagnostics = 0;
rsf_fgvk* presentation = nullptr;
bool retire() {
    if (!context) return true;
    if (presentation && rsf_fgvk_drain(presentation) != RSF_BACKEND_OK) return false;
    using WaitIdle = int32_t (*)(void*);
    auto wait = device_proc ? reinterpret_cast<WaitIdle>(device_proc(device, "vkDeviceWaitIdle")) : nullptr;
    if (!wait || wait(device) != 0 || rsf_srvk_destroy(context) != RSF_BACKEND_OK) return false;
    context = nullptr;
    return true;
}
}
namespace rsf::godot_vk {
bool booted() { return active; }
int configure(const rsf_godot_settings* value) {
    // These paths are added independently. Do not accept unavailable FG/PCL or
    // silently substitute an FSR3 provider for a requested DLSS/XeSS/FSR4 path.
    if (!active || value->latency_mode ||
        !(value->sr_backend == 0 || value->sr_backend == 2 || value->sr_backend == 3 || value->sr_backend == 6 || value->sr_backend == 7)) return 0;
    std::lock_guard lock(guard);
    if (value->fg_mode && (!presentation || value->generated_frames != 1 || !(value->fg_backend == 3 || value->fg_backend == 7))) return 0;
    if (value->sr_backend != requested.sr_backend || value->quality != requested.quality) ++generation;
    requested = *value;
    return 1;
}
int status(rsf_godot_status* value) {
    std::lock_guard lock(guard);
    *value = current;
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
    active = false; current.active_sr = 0; current.sr_result = RSF_BACKEND_ERROR_NOT_READY;
    device = nullptr; device_proc = nullptr; directory.clear();
    return 1;
}
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
    directory = std::string(setup->runtime_directory) + "/fidelityfx-vulkan";
    logger = setup->log; logger_user = setup->log_user;
    current = {sizeof(current), RSF_GODOT_ABI_VERSION};
    current.session_id = GetTickCount64() + 1; diagnostics = 0;
    current.fg_result = RSF_BACKEND_ERROR_NOT_SUPPORTED;
    active = true;
    if (setup->log) setup->log(setup->log_user, "Godot Vulkan host ready. Native FSR presentation requires its reserved queues. PCL is pending.");
    return 1;
} catch (...) { return 0; }
extern "C" int32_t rsfg_vk_create(const rsf_fgvk_setup* setup, void** result, uint64_t* chain) try {
    if (!active || !setup || setup->struct_size < sizeof(*setup) || !result || !chain) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    std::lock_guard lock(guard);
    if (presentation) return RSF_BACKEND_ERROR_NOT_SUPPORTED;
    auto value = *setup; value.graphics.runtime_directory_utf8 = directory.c_str();
    auto code = rsf_fgvk_create(&value, &presentation, chain);
    *result = presentation;
    current.fg_available = presentation ? 1u : 0u;
    current.max_generated_frames = presentation ? 1u : 0u;
    if (logger) {
        char message[128]; std::snprintf(message, sizeof(message), "Godot Vulkan swapchain owner: create=%d reserved queues=%u", code, presentation ? 4u : 0u);
        logger(logger_user, message);
    }
    return code;
} catch (...) { return RSF_BACKEND_ERROR_FEATURE_FAILED; }
extern "C" int32_t rsfg_vk_images(void* value, uint32_t* count, uint64_t* images) { return rsf_fgvk_images(static_cast<rsf_fgvk*>(value), count, images); }
extern "C" int32_t rsfg_vk_acquire(void* value, uint64_t timeout, uint64_t semaphore, uint64_t fence, uint32_t* index) {
    return rsf_fgvk_acquire(static_cast<rsf_fgvk*>(value), timeout, semaphore, fence, index);
}
extern "C" int32_t rsfg_vk_present(void* value, const void* info) try {
    std::lock_guard lock(guard);
    auto* target = static_cast<rsf_fgvk*>(value);
    auto result = rsf_fgvk_present(target, info);
    rsf_fgvk_status status{sizeof(status), RSF_FGVK_ABI_VERSION};
    if (rsf_fgvk_get_status(target, &status) == RSF_BACKEND_OK) {
        current.fg_active = status.enabled; current.fg_result = status.result; current.generated_callbacks = status.generated_callbacks;
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
    if (result == RSF_BACKEND_OK && presentation == target) { presentation = nullptr; current.fg_available = current.fg_active = 0; }
    return result;
} catch (...) { return RSF_BACKEND_ERROR_FEATURE_FAILED; }
extern "C" int32_t rsfg_vk_drain(void* value) try {
    std::lock_guard lock(guard);
    return rsf_fgvk_drain(static_cast<rsf_fgvk*>(value));
} catch (...) { return RSF_BACKEND_ERROR_FEATURE_FAILED; }
extern "C" int rsfg_render_vulkan(void* commands, const rsf_godot_vk_packet* packet) try {
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
        (void)rsf_fgvk_enable(presentation, enabled ? 1u : 0u);
        rsf_fgvk_frame fg{}; fg.struct_size = sizeof(fg); fg.abi_version = RSF_FGVK_ABI_VERSION; fg.record = &record;
        fg.depth = image(packet->depth, 5); fg.motion = image(packet->motion, 5); fg.hudless = image(packet->color, 5);
        fg.motion_scale_x = -float(camera.render_width) * pass.motion_to_uv[0]; fg.motion_scale_y = -float(camera.render_height) * pass.motion_to_uv[1];
        current.fg_backend = 3; current.generated_frames = enabled ? 1u : 0u;
        current.fg_result = pass.role == RSF_GAME_RENDER_SR ? rsf_fgvk_prepare(presentation, commands, &fg) : rsf_fgvk_hudless(presentation, commands, &fg);
    }
    if (pass.role == RSF_GAME_RENDER_FINAL_SCENE) return current.fg_result == RSF_BACKEND_OK ? 1 : 0;
    const uint32_t backend = requested.sr_backend == 7 ? 3 : requested.sr_backend;
    if (context && (generation != context_generation || packet->device != device ||
        current.output_width != camera.output_width || current.output_height != camera.output_height)) {
        if (!retire()) { current.sr_result = RSF_BACKEND_ERROR_NOT_READY; return 0; }
    }
    current.requested_sr = requested.sr_backend;
    current.output_width = camera.output_width; current.output_height = camera.output_height;
    current.render_width = camera.render_width; current.render_height = camera.render_height;
    current.source_frame_id = pass.source_frame_id;
    if (backend == 0 || backend == 6) { current.active_sr = backend; current.sr_result = RSF_BACKEND_OK; ++current.evaluated_frames; return 1; }
    if (!context) {
        rsf_srvk_setup setup{}; setup.struct_size = sizeof(setup); setup.abi_version = RSF_SRVK_ABI_VERSION;
        setup.instance = packet->instance; setup.physical_device = packet->physical_device; setup.device = packet->device;
        setup.instance_proc = reinterpret_cast<rsf_vk_get_proc>(packet->instance_proc);
        setup.device_proc = reinterpret_cast<rsf_vk_get_proc>(packet->device_proc);
        setup.backend = backend; setup.quality = requested.quality; setup.output_width = camera.output_width; setup.output_height = camera.output_height;
        setup.depth_inverted = camera.depth_inverted; setup.motion_jittered = camera.motion_jittered; setup.runtime_directory_utf8 = directory.c_str();
        current.sr_result = rsf_srvk_create(&setup, &context);
        current.active_sr = 0;
        if (!context) { ++current.refused_frames; return 0; }
        device = packet->device; device_proc = packet->device_proc;
        context_generation = generation; context_backend = backend;
    }
    uint32_t width = 0, height = 0;
    (void)rsf_srvk_plan(context, &width, &height);
    current.render_width = width; current.render_height = height;
    rsf_srvk_frame frame{}; frame.struct_size = sizeof(frame); frame.abi_version = RSF_SRVK_ABI_VERSION; frame.record = &record;
    // Vulkan SHADER_READ_ONLY_OPTIMAL = 5, TRANSFER_DST_OPTIMAL = 7.
    frame.color = image(packet->color, 5); frame.depth = image(packet->depth, 5); frame.motion = image(packet->motion, 5); frame.output = image(packet->output, 7);
    frame.motion_scale_x = -float(camera.render_width) * pass.motion_to_uv[0];
    frame.motion_scale_y = -float(camera.render_height) * pass.motion_to_uv[1]; frame.pre_exposure = 1;
    current.sr_result = rsf_srvk_evaluate(context, commands, &frame);
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
    if (current.sr_result == RSF_BACKEND_OK) ++current.evaluated_frames;
    else ++current.refused_frames;
    return current.sr_result == RSF_BACKEND_OK ? 1 : 0;
} catch (...) { return 0; }
