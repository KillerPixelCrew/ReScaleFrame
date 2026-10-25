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
bool retire() {
    if (!context) return true;
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
    if (!active || value->fg_mode || value->latency_mode ||
        !(value->sr_backend == 0 || value->sr_backend == 2 || value->sr_backend == 3 || value->sr_backend == 6 || value->sr_backend == 7)) return 0;
    std::lock_guard lock(guard);
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
    if (setup->log) setup->log(setup->log_user, "Godot Vulkan SR host ready. Vulkan FG and PCL are not implemented yet.");
    return 1;
} catch (...) { return 0; }
extern "C" int rsfg_render_vulkan(void* commands, const rsf_godot_vk_packet* packet) try {
    if (!active || !commands || !packet || packet->struct_size < sizeof(*packet) || packet->abi_version != RSF_GODOT_ABI_VERSION ||
        !packet->device || !packet->instance_proc || !packet->device_proc || packet->pass.struct_size < sizeof(packet->pass) ||
        packet->pass.role != RSF_GAME_RENDER_SR || !packet->pass.camera_valid) return 0;
    std::lock_guard lock(guard);
    const auto& pass = packet->pass;
    const auto& camera = pass.camera;
    const uint32_t backend = requested.sr_backend == 7 ? 3 : requested.sr_backend;
    if (context && (generation != context_generation || packet->device != device ||
        current.output_width != camera.output_width || current.output_height != camera.output_height)) {
        if (!retire()) { current.sr_result = RSF_BACKEND_ERROR_NOT_READY; return 0; }
    }
    current.requested_sr = requested.sr_backend;
    current.output_width = camera.output_width; current.output_height = camera.output_height;
    current.render_width = camera.render_width; current.render_height = camera.render_height;
    current.source_frame_id = pass.source_frame_id;
    if (backend == 0 || backend == 6) { current.active_sr = backend; current.sr_result = RSF_BACKEND_OK; return 1; }
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
    rsf_frame_record record{}; record.struct_size = sizeof(record); record.abi_version = RSF_GAME_FRAME_ABI_VERSION;
    record.session_id = pass.session_id; record.frame_id = pass.source_frame_id; record.view_id = static_cast<uint32_t>(pass.view_key);
    record.resource_generation = pass.resource_generation; record.camera = camera; record.frame_time_ms = camera.frame_time_seconds * 1000;
    record.render_width = camera.render_width; record.render_height = camera.render_height;
    record.output_width = camera.output_width; record.output_height = camera.output_height;
    record.flags = pass.flags & RSF_GAME_RENDER_RESET ? RSF_FRAME_FLAG_RESET : 0;
    auto image = [packet](const rsf_godot_vk_image& value, uint32_t layout) {
        return rsf_srvk_image{packet->device, value.image, value.format, value.width, value.height, layout, value.copy_source};
    };
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
