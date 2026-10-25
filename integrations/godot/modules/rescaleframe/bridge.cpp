// SPDX-License-Identifier: GPL-3.0-only
#include "rsf_godot_engine.h"
#include "api.h"
#include "servers/rendering/renderer_rd/storage_rd/render_data_rd.h"
#include "core/config/project_settings.h"
#include "core/config/engine.h"
#include "core/object/callable_mp.h"
#include "core/object/class_db.h"
#include "core/os/os.h"
#include "core/string/print_string.h"
#include "servers/rendering/renderer_rd/storage_rd/render_scene_data_rd.h"
#include "servers/rendering/renderer_rd/storage_rd/render_scene_buffers_rd.h"
#include "servers/rendering/renderer_rd/storage_rd/texture_storage.h"
#include "servers/rendering/rendering_server_globals.h"
#include "servers/rendering/rendering_device.h"
#include "servers/rendering/renderer_viewport.h"
#include "servers/rendering/rendering_server.h"
#include "drivers/d3d12/rendering_device_driver_d3d12.h"
#include "drivers/vulkan/rendering_device_driver_vulkan.h"
#include "scene/main/scene_tree.h"
#include "scene/main/window.h"
#include "servers/display/display_server.h"
#include <windows.h>
#include <atomic>
#include <cstring>

namespace {
decltype(&rsfg_boot) native_boot = nullptr;
decltype(&rsfg_boot_vulkan) native_boot_vulkan = nullptr;
decltype(&rsfg_vk_resolver) native_vk_resolver = nullptr;
decltype(&rsfg_vk_window) native_vk_window = nullptr;
decltype(&rsfg_vk_present_marker) native_vk_present_marker = nullptr;
decltype(&rsfg_vk_native_present) native_vk_native_present = nullptr;
decltype(&rsfg_vk_wsi_call) native_vk_wsi_call = nullptr;
decltype(&rsfg_render_vulkan) native_render_vulkan = nullptr;
decltype(&rsfg_vk_create) native_vk_create = nullptr;
decltype(&rsfg_vk_images) native_vk_images = nullptr;
decltype(&rsfg_vk_acquire) native_vk_acquire = nullptr;
decltype(&rsfg_vk_present) native_vk_present = nullptr;
decltype(&rsfg_vk_submitted) native_vk_submitted = nullptr;
decltype(&rsfg_vk_destroy) native_vk_destroy = nullptr;
decltype(&rsfg_vk_drain) native_vk_drain = nullptr;
decltype(&rsfg_create_swapchain) native_create_swapchain = nullptr;
decltype(&rsfg_configure) native_configure = nullptr;
decltype(&rsfg_status) native_status = nullptr;
decltype(&rsfg_render) native_render = nullptr;
decltype(&rsfg_cpu) native_cpu = nullptr;
decltype(&rsfg_render_begin) native_render_begin = nullptr;
decltype(&rsfg_submitted) native_submitted = nullptr;
decltype(&rsfg_present) native_present = nullptr;
decltype(&rsfg_shutdown) native_shutdown = nullptr;
bool ready = false;
std::atomic<bool> enabled{false};
std::atomic<uint32_t> requested_sr{7};
std::atomic<bool> reset_requested{true};
uint32_t pending_input = 0;
bool cpu_open = false, simulation_ended = false, frame_queued = false, render_gameplay = true;
uint64_t cpu_frame = 0, render_frame = 0, session = 0, last_frame = 0;
uint64_t previous_cpu_usec = 0;
double cpu_delta = 1.0 / 60.0, render_delta = 1.0 / 60.0;
RID primary_viewport, primary_target;
uint32_t generation = 1;
Vector2i previous_size;
Projection previous_projection;
Transform3D previous_camera;
uint64_t previous_color = 0;
rsf_game_render_pass last_pass{};

int configure_from_extension(const rsf_godot_settings *value, uint64_t viewport_id) {
    if (!value || value->struct_size < sizeof(*value) || value->abi_version != RSF_GODOT_ABI_VERSION) return 0;
    auto *bridge = Object::cast_to<ReScaleFrameBridge>(Engine::get_singleton()->get_singleton_object("ReScaleFrame"));
    if (!bridge) return 0;
    Dictionary options;
#define RSF_FIELD(name) options[#name] = value->name
    RSF_FIELD(sr_backend); RSF_FIELD(quality); RSF_FIELD(dlss_preset); RSF_FIELD(fg_backend); RSF_FIELD(fg_mode);
    RSF_FIELD(generated_frames); RSF_FIELD(latency_mode); RSF_FIELD(frame_limit_us); RSF_FIELD(gameplay);
#undef RSF_FIELD
    return bridge->configure(options, RID::from_uint64(viewport_id)) ? 1 : 0;
}
void reset_from_extension() { reset_requested.store(true); }

void log(void *, const char *message) { print_line(String("ReScaleFrame: ") + String::utf8(message)); }
void cpu(uint32_t stage, uint32_t expected = 0) {
    if (!ready || !enabled.load() || !cpu_frame) return;
    LARGE_INTEGER now, frequency;
    QueryPerformanceCounter(&now); QueryPerformanceFrequency(&frequency);
    const rsf_game_cpu_event event{sizeof(event), stage, session, cpu_frame,
        uint64_t(now.QuadPart), uint64_t(frequency.QuadPart), expected, 0, 0};
    native_cpu(&event);
}
void set_primary(RID viewport) {
    primary_viewport = viewport;
    primary_target = RSG::viewport->viewport_get_render_target(viewport);
    ++generation; last_frame = 0;
}
void set_render_frame(uint64_t frame, bool gameplay, double seconds) {
    render_frame = frame;
    render_gameplay = gameplay;
    render_delta = seconds;
    native_render_begin(frame);
    rsf_godot_status status{sizeof(status), RSF_GODOT_ABI_VERSION};
    if (requested_sr.load() != 0 && requested_sr.load() != 6 && primary_viewport.is_valid() && native_status(&status) &&
        status.requested_sr == requested_sr.load() && status.render_width && status.output_width) {
        RSG::viewport->viewport_set_scaling_3d_scale(primary_viewport,
            double(status.render_width) / status.output_width);
    }
}
void matrix(float *out, const Projection &value) {
    // Godot multiplies column vectors. The SDK uses row vectors, so transpose
    // the mathematical transform. Copying columns into SDK rows also places
    // camera translation at elements 12..14, as all shared consumers require.
    for (int row = 0; row < 4; ++row)
        for (int column = 0; column < 4; ++column) out[row * 4 + column] = value.columns[row][column];
}
String disk_path(const String &value) {
    auto *project_settings = ProjectSettings::get_singleton();
    String path = project_settings->globalize_path(value);
    if (!path.is_absolute_path()) {
        String base = project_settings->globalize_path("res://");
        if (!base.is_absolute_path()) base = OS::get_singleton()->get_executable_path().get_base_dir();
        path = base.path_join(path);
    }
    return path.simplify_path();
}
struct Packet {
    rsf_game_render_pass pass{};
    void *device = nullptr;
    void *queue = nullptr;
    bool vulkan = false;
    rsf_godot_vk_packet vk{};
};
void execute(RenderingDeviceDriver *driver, RenderingDeviceDriver::CommandBufferID commands, void *userdata) {
    Packet *packet = static_cast<Packet *>(userdata);
    if (packet->vulkan) {
        auto *vk = static_cast<RenderingDeviceDriverVulkan *>(driver);
        if (void *list = vk->rescaleframe_begin(commands)) native_render_vulkan(list, &packet->vk);
    } else {
        auto *d3d = static_cast<RenderingDeviceDriverD3D12 *>(driver);
        void *list = d3d->rescaleframe_begin(commands);
        native_render(packet->device, packet->queue, list, &packet->pass);
        d3d->rescaleframe_end(commands);
    }
    memdelete(packet);
}
bool admit(const RenderDataRD *data) {
    return ready && enabled.load() && render_frame && data && data->render_buffers.is_valid() &&
        data->render_buffers->get_render_target() == primary_target && data->render_buffers->get_view_count() == 1 &&
        (RenderingServer::get_singleton()->get_current_rendering_driver_name() == "d3d12" ||
        RenderingServer::get_singleton()->get_current_rendering_driver_name() == "vulkan");
}
Packet *packet_for(const rsf_game_render_pass &pass) {
    RD *rd = RD::get_singleton();
    Packet *packet = memnew(Packet);
    packet->pass = pass;
    packet->vulkan = RenderingServer::get_singleton()->get_current_rendering_driver_name() == "vulkan";
    packet->device = reinterpret_cast<void *>(rd->get_driver_resource(RD::DRIVER_RESOURCE_LOGICAL_DEVICE));
    if (!packet->vulkan) packet->queue = reinterpret_cast<void *>(rd->get_driver_resource(RD::DRIVER_RESOURCE_COMMAND_QUEUE));
    return packet;
}
}

void ReScaleFrameBridge::_bind_methods() {
    ClassDB::bind_method(D_METHOD("configure", "options", "viewport"), &ReScaleFrameBridge::configure);
    ClassDB::bind_method(D_METHOD("get_status"), &ReScaleFrameBridge::get_status);
    ClassDB::bind_method(D_METHOD("reset_history"), &ReScaleFrameBridge::reset_history);
}
void ReScaleFrameBridge::reset_history() { reset_requested.store(true); }
bool ReScaleFrameBridge::configure(const Dictionary &options, RID viewport) {
    if (!ready || !viewport.is_valid()) return false;
    SceneTree *tree = SceneTree::get_singleton();
    if (!tree || !tree->get_root() || tree->get_root()->get_viewport_rid() != viewport) return false;
    rsf_godot_settings value{sizeof(value), 1, 7, 1, 0, 7, 0, 1, 0, 0, 1};
    value.sr_backend = options.get("sr_backend", value.sr_backend);
    value.quality = options.get("quality", value.quality);
    value.dlss_preset = options.get("dlss_preset", value.dlss_preset);
    value.fg_backend = options.get("fg_backend", value.fg_backend);
    value.fg_mode = options.get("fg_mode", value.fg_mode);
    value.generated_frames = options.get("generated_frames", value.generated_frames);
    value.latency_mode = options.get("latency_mode", value.latency_mode);
    value.frame_limit_us = options.get("frame_limit_us", value.frame_limit_us);
    value.gameplay = options.get("gameplay", value.gameplay);
    if (RSFGodot::vulkan_enabled()) native_vk_window(reinterpret_cast<void *>(DisplayServer::get_singleton()->window_get_native_handle(DisplayServerEnums::WINDOW_HANDLE)));
    if (!native_configure(&value)) return false;
    requested_sr.store(value.sr_backend);
    RenderingServer *rs = RenderingServer::get_singleton();
    rs->call_on_render_thread(callable_mp_static(set_primary).bind(viewport));
    rs->viewport_set_scaling_3d_mode(viewport, value.sr_backend == 0 ? RSE::VIEWPORT_SCALING_3D_MODE_BILINEAR :
        value.sr_backend == 6 ? RSE::VIEWPORT_SCALING_3D_MODE_FSR : RSE::VIEWPORT_SCALING_3D_MODE_FSR2);
    const double scales[] = {1.0, 2.0 / 3.0, 1.0 / 1.7, 0.5, 1.0 / 3.0, 0.77};
    rs->viewport_set_scaling_3d_scale(viewport, value.sr_backend == 0 ? 1.0 : scales[value.quality]);
    enabled.store(true);
    return true;
}
Dictionary ReScaleFrameBridge::get_status() const {
    Dictionary result;
    result["engine_bridge_ready"] = ready;
    rsf_godot_status status{sizeof(status), RSF_GODOT_ABI_VERSION};
    if (!ready || !native_status(&status)) return result;
#define RSF_FIELD(name) result[#name] = status.name
    RSF_FIELD(session_id); RSF_FIELD(requested_sr); RSF_FIELD(active_sr);
    RSF_FIELD(render_width); RSF_FIELD(render_height); RSF_FIELD(output_width); RSF_FIELD(output_height);
    RSF_FIELD(sr_result); RSF_FIELD(fg_result); RSF_FIELD(fg_backend); RSF_FIELD(fg_available);
    RSF_FIELD(fg_active); RSF_FIELD(max_generated_frames); RSF_FIELD(generated_frames);
    RSF_FIELD(latency_available); RSF_FIELD(latency_mode); RSF_FIELD(reason); RSF_FIELD(pcl_available);
    RSF_FIELD(latency_report_available); RSF_FIELD(evaluated_frames); RSF_FIELD(refused_frames);
    RSF_FIELD(tagged_frames); RSF_FIELD(sleep_calls); RSF_FIELD(marker_calls);
    RSF_FIELD(total_presented); RSF_FIELD(generated_presented); RSF_FIELD(presentation_statistics_valid);
    RSF_FIELD(presentation_statistics_flags); RSF_FIELD(source_frame_id); RSF_FIELD(provider_generation); RSF_FIELD(generated_callbacks);
#undef RSF_FIELD
    return result;
}

namespace RSFGodot {
void boot() {
    if (!GLOBAL_DEF("rescaleframe/enabled", false)) return;
    if (Engine::get_singleton()->is_editor_hint()) return;
    // Import and other headless tools have no presentation surface. In particular,
    // do not install presentation interception before an editor import scans DLLs.
    for (const String &argument : OS::get_singleton()->get_cmdline_args()) {
        if (argument == "--headless") return;
    }
    const String library = disk_path(GLOBAL_DEF("rescaleframe/native_library", "res://addons/rescaleframe/bin/ReScaleFrame.Godot.Native.dll"));
    const String runtime = disk_path(GLOBAL_DEF("rescaleframe/runtime_directory", "res://addons/rescaleframe/runtime"));
    const String id = GLOBAL_DEF("rescaleframe/project_id", "125d2115-7176-46e1-8806-95b790f01717");
    HMODULE module = LoadLibraryW(reinterpret_cast<const wchar_t *>(library.utf16().get_data()));
    if (!module) { ERR_PRINT("Cannot load ReScaleFrame's native library: " + library + " (Windows error " + itos(GetLastError()) + ")"); return; }
#define RSF_RESOLVE(name) native_##name = reinterpret_cast<decltype(native_##name)>(GetProcAddress(module, "rsfg_" #name)); if (!native_##name) return
    RSF_RESOLVE(boot); RSF_RESOLVE(boot_vulkan); RSF_RESOLVE(render_vulkan);
    RSF_RESOLVE(vk_resolver); RSF_RESOLVE(vk_window); RSF_RESOLVE(vk_present_marker); RSF_RESOLVE(vk_native_present); RSF_RESOLVE(vk_wsi_call);
    RSF_RESOLVE(vk_create); RSF_RESOLVE(vk_images); RSF_RESOLVE(vk_acquire); RSF_RESOLVE(vk_present); RSF_RESOLVE(vk_submitted); RSF_RESOLVE(vk_destroy);
    RSF_RESOLVE(vk_drain);
    RSF_RESOLVE(create_swapchain); RSF_RESOLVE(configure); RSF_RESOLVE(status); RSF_RESOLVE(render);
    RSF_RESOLVE(cpu); RSF_RESOLVE(render_begin); RSF_RESOLVE(submitted); RSF_RESOLVE(present); RSF_RESOLVE(shutdown);
#undef RSF_RESOLVE
    const CharString path_utf8 = runtime.utf8(), id_utf8 = id.utf8();
    const rsf_godot_setup setup{sizeof(setup), RSF_GODOT_ABI_VERSION, path_utf8.get_data(), "4.7.2", id_utf8.get_data(), log, nullptr};
    ready = (OS::get_singleton()->get_current_rendering_driver_name() == "vulkan" ? native_boot_vulkan(&setup) : native_boot(&setup)) != 0;
    if (!ready) ERR_PRINT("ReScaleFrame native host rejected startup; inspect its dependency and initialization diagnostics.");
    if (ready) {
        auto attach = reinterpret_cast<decltype(&rsfg_attach_engine)>(GetProcAddress(module, "rsfg_attach_engine"));
        const rsf_godot_engine_services services{sizeof(services), RSF_GODOT_ABI_VERSION, configure_from_extension, reset_from_extension};
        if (!attach || !attach(&services)) { ready = false; return; }
        rsf_godot_status status{sizeof(status), RSF_GODOT_ABI_VERSION}; native_status(&status); session = status.session_id;
    }
}
bool create_swapchain(void *factory, void *queue, void *window, const void *description, void **out) {
    return ready && SUCCEEDED(native_create_swapchain(factory, queue, window, description, out));
}
bool vulkan_enabled() { return ready && OS::get_singleton()->get_current_rendering_driver_name() == "vulkan"; }
uint64_t vk_resolver() { return vulkan_enabled() ? reinterpret_cast<uint64_t>(native_vk_resolver()) : 0; }
bool vk_present_marker(uint64_t window, uint32_t begin) { return vulkan_enabled() && native_vk_present_marker(window, begin); }
int32_t vk_native_present(uint64_t callback, void* queue, const void* info) {
    if (vulkan_enabled()) return native_vk_native_present(callback, queue, info);
    using Present = int32_t (*)(void*, const void*);
    return reinterpret_cast<Present>(callback)(queue, info);
}
int32_t vk_wsi_call(int32_t (*callback)(void*), void* user) { return vulkan_enabled() ? native_vk_wsi_call(callback, user) : callback(user); }
bool vk_create(const void *setup, void **owner, uint64_t *chain) {
    return vulkan_enabled() && native_vk_create(static_cast<const rsf_fgvk_setup *>(setup), owner, chain) == 0;
}
int32_t vk_images(void *owner, uint32_t *count, uint64_t *images) { return native_vk_images(owner, count, images); }
int32_t vk_acquire(void *owner, uint64_t timeout, uint64_t semaphore, uint64_t fence, uint32_t *index) { return native_vk_acquire(owner, timeout, semaphore, fence, index); }
int32_t vk_present(void *owner, const void *info) { return native_vk_present(owner, info); }
void vk_submitted(void *const *commands, uint32_t count) { if (ready) native_vk_submitted(commands, count); }
bool vk_destroy(void *owner) { return native_vk_destroy(owner) == 0; }
bool vk_drain(void *owner) { return native_vk_drain(owner) == 0; }
void shutdown() {
    enabled.store(false);
    if (auto *rs = RenderingServer::get_singleton()) {
        rs->call_on_render_thread(callable_mp_static(release_motion));
        rs->sync();
    }
    if (ready) native_shutdown();
}
void cpu_begin() {
    if (!ready || !enabled.load() || cpu_open) return;
    const uint64_t now = OS::get_singleton()->get_ticks_usec();
    cpu_delta = previous_cpu_usec ? double(now - previous_cpu_usec) / 1000000.0 : 1.0 / 60.0;
    previous_cpu_usec = now;
    ++cpu_frame;
    cpu_open = true; simulation_ended = false; frame_queued = false;
    cpu(RSF_GAME_CPU_FRAME_BEGIN); cpu(RSF_GAME_CPU_PACING);
}
void cpu_simulation_begin() {
    if (!cpu_open) return;
    cpu(RSF_GAME_CPU_INPUT_SAMPLE); cpu(RSF_GAME_CPU_SIMULATION_BEGIN);
    if (pending_input) { const uint32_t kind = pending_input; pending_input = 0; input_event(kind); }
}
void queue_frame(bool present) {
    if (!ready || !enabled.load()) return;
    cpu(RSF_GAME_CPU_SIMULATION_END);
    simulation_ended = true;
    frame_queued = present;
    const bool gameplay = (!SceneTree::get_singleton() || !SceneTree::get_singleton()->is_paused()) &&
        DisplayServer::get_singleton()->window_is_focused();
    RenderingServer::get_singleton()->call_on_render_thread(callable_mp_static(set_render_frame).bind(present ? cpu_frame : uint64_t(0), gameplay, cpu_delta));
}
void cpu_end() {
    if (!simulation_ended) cpu(RSF_GAME_CPU_SIMULATION_END);
    cpu(RSF_GAME_CPU_FRAME_END, frame_queued ? 1 : 0);
    cpu_open = false;
}
void input_event(uint32_t kind) {
    if (!ready || !enabled.load() || !kind || (kind & ~7u)) return;
    if (!cpu_open) { pending_input |= kind; return; }
    LARGE_INTEGER now, frequency; QueryPerformanceCounter(&now); QueryPerformanceFrequency(&frequency);
    const rsf_game_cpu_event event{sizeof(event), RSF_GAME_CPU_INPUT_EVENT, session, cpu_frame,
        uint64_t(now.QuadPart), uint64_t(frequency.QuadPart), 0, kind, 0};
    native_cpu(&event);
}
void render_size(RID target, const Vector2i &output, Vector2i &internal) {
    if (!ready || !enabled.load() || target != primary_target) return;
    if (requested_sr.load() == 0) { internal = output; return; }
    if (requested_sr.load() == 6) return;
    rsf_godot_status status{sizeof(status), RSF_GODOT_ABI_VERSION};
    if (native_status(&status) && status.requested_sr == requested_sr.load() &&
        status.output_width == uint32_t(output.x) && status.output_height == uint32_t(output.y) && status.render_width && status.render_height)
        internal = Vector2i(status.render_width, status.render_height);
}
void camera_changed(RID target) {
    if (ready && enabled.load() && target == primary_target) reset_requested.store(true);
}
void upscale(const RenderDataRD *data, double) {
    if (!admit(data)) return;
    Ref<RenderSceneBuffersRD> rb = data->render_buffers;
    RD *rd = RD::get_singleton();
    rb->ensure_upscaled();
    const RID color = rb->get_internal_texture(0), depth = rb->get_depth_texture(0), motion = rb->get_velocity_buffer(false, 0), output = rb->get_upscaled_texture(0);
    if (!color.is_valid() || !depth.is_valid() || !motion.is_valid() || !output.is_valid()) return;
    const RenderSceneDataRD *scene = data->scene_data;
    rsf_game_render_pass pass{};
    pass.struct_size = sizeof(pass); pass.role = RSF_GAME_RENDER_SR;
    pass.session_id = session; pass.source_frame_id = pass.native_frame = render_frame;
    pass.view_key = pass.history_key = pass.viewport_key = rb->get_render_target().get_id();
    pass.resource_generation = generation;
    pass.flags = RSF_GAME_RENDER_PRIMARY;
    if (previous_color != color.get_id()) ++generation;
    pass.resource_generation = generation;
    pass.screen = render_gameplay ? RSF_SCREEN_FLIGHT : RSF_SCREEN_MENU;
    if (reset_requested.exchange(false) || previous_color != color.get_id() || previous_size != rb->get_internal_size() ||
        last_frame + 1 != render_frame || render_delta > 0.25 || previous_camera.origin.distance_to(scene->cam_transform.origin) > 20.0f)
        pass.flags |= RSF_GAME_RENDER_RESET;
    auto native = [rd](RID texture) { return reinterpret_cast<void *>(rd->get_driver_resource(RD::DRIVER_RESOURCE_TEXTURE, texture)); };
    pass.color_input = native(color); pass.color_output = native(output); pass.depth = native(depth); pass.motion = native(motion);
    pass.motion_to_uv[0] = pass.motion_to_uv[1] = -1;
    pass.motion_camera_included = 1; pass.camera_valid = 1;
    auto &camera = pass.camera;
    camera.struct_size = sizeof(camera); camera.abi_version = RSF_GAME_FRAME_ABI_VERSION;
    camera.render_width = rb->get_internal_size().x; camera.render_height = rb->get_internal_size().y;
    camera.output_width = rb->get_target_size().x; camera.output_height = rb->get_target_size().y;
    camera.near_plane = scene->z_near; camera.far_plane = scene->z_far;
    camera.vertical_fov_radians = Math::deg_to_rad(scene->cam_projection.get_fovy(scene->cam_projection.get_fov(), 1.0 / scene->cam_projection.get_aspect()));
    camera.depth_inverted = 1; camera.frame_time_seconds = float(render_delta);
    camera.jitter_pixels[0] = scene->taa_jitter.x * camera.render_width * 0.5f;
    camera.jitter_pixels[1] = scene->taa_jitter.y * camera.render_height * 0.5f;
    camera.previous_jitter_pixels[0] = scene->prev_taa_jitter.x * camera.render_width * 0.5f;
    camera.previous_jitter_pixels[1] = scene->prev_taa_jitter.y * camera.render_height * 0.5f;
    // SDK clip space has +Y up. Godot's render-target Y flip is already reflected
    // in its texture-space velocity and must not be applied to camera metadata.
    Projection correction; correction.set_depth_correction(false);
    const Projection projection = correction * scene->cam_projection;
    matrix(camera.view_to_clip, projection); matrix(camera.clip_to_view, projection.inverse());
    matrix(camera.view_to_world, Projection(scene->cam_transform)); matrix(camera.world_to_view, Projection(scene->cam_transform.affine_inverse()));
    const Projection to_previous = (pass.flags & RSF_GAME_RENDER_RESET) ? Projection() :
        previous_projection * Projection(previous_camera.affine_inverse() * scene->cam_transform) * projection.inverse();
    matrix(camera.clip_to_previous_clip, to_previous);
    matrix(pass.previous_clip_to_clip, to_previous.inverse());
    const RID resolved_motion = resolve_motion(rb, depth, motion, to_previous, scene->taa_jitter);
    if (!resolved_motion.is_valid()) return;
    const RID resolved_depth = RenderingServer::get_singleton()->get_current_rendering_driver_name() == "vulkan" ?
        rb->get_texture(StringName("rescaleframe"), StringName("depth_r32")) : depth;
    if (!resolved_depth.is_valid()) return;
    pass.depth = native(resolved_depth);
    pass.motion = native(resolved_motion);
    previous_projection = projection; previous_camera = scene->cam_transform; previous_size = rb->get_internal_size();
    previous_color = color.get_id(); last_frame = render_frame;
    last_pass = pass;
    last_pass.color_input = last_pass.color_output = last_pass.depth = last_pass.motion = last_pass.exposure = nullptr;
    last_pass.color_output_readable = last_pass.ui_input = last_pass.swapchain = last_pass.scene_surface = last_pass.sampled_texture = nullptr;
    RD::CallbackResource resources[] = {
        {color, RD::CALLBACK_RESOURCE_TYPE_TEXTURE, RD::CALLBACK_RESOURCE_USAGE_TEXTURE_SAMPLE},
        {resolved_depth, RD::CALLBACK_RESOURCE_TYPE_TEXTURE, RD::CALLBACK_RESOURCE_USAGE_TEXTURE_SAMPLE},
        {resolved_motion, RD::CALLBACK_RESOURCE_TYPE_TEXTURE, RD::CALLBACK_RESOURCE_USAGE_TEXTURE_SAMPLE},
        {output, RD::CALLBACK_RESOURCE_TYPE_TEXTURE, RD::CALLBACK_RESOURCE_USAGE_COPY_TO}};
    Packet *packet = packet_for(pass);
    if (packet->vulkan) {
        auto &vk = packet->vk;
        vk.struct_size = sizeof(vk); vk.abi_version = RSF_GODOT_ABI_VERSION; vk.pass = pass;
        vk.pass.color_input = vk.pass.color_output = vk.pass.depth = vk.pass.motion = nullptr;
        vk.instance = reinterpret_cast<void *>(rd->get_driver_resource(RD::DRIVER_RESOURCE_TOPMOST_OBJECT));
        vk.physical_device = reinterpret_cast<void *>(rd->get_driver_resource(RD::DRIVER_RESOURCE_PHYSICAL_DEVICE));
        vk.device = packet->device;
        vk.instance_proc = reinterpret_cast<rsf_godot_vk_proc>(RenderingDeviceDriverVulkan::rescaleframe_instance_proc());
        vk.device_proc = reinterpret_cast<rsf_godot_vk_proc>(RenderingDeviceDriverVulkan::rescaleframe_device_proc());
        auto describe = [rd](RID texture, const Size2i &size) {
            return rsf_godot_vk_image{rd->get_driver_resource(RD::DRIVER_RESOURCE_TEXTURE, texture),
                uint32_t(rd->get_driver_resource(RD::DRIVER_RESOURCE_TEXTURE_DATA_FORMAT, texture)), uint32_t(size.x), uint32_t(size.y),
                uint32_t((rd->texture_get_format(texture).usage_bits & RD::TEXTURE_USAGE_CAN_COPY_FROM_BIT) != 0)};
        };
        vk.color = describe(color, rb->get_internal_size()); vk.depth = describe(resolved_depth, rb->get_internal_size());
        vk.motion = describe(resolved_motion, rb->get_internal_size()); vk.output = describe(output, rb->get_target_size());
    }
    rd->driver_callback_add(execute, packet, VectorView<RD::CallbackResource>(resources, 4));
}
void hudless(const RenderDataRD *data) {
    if (!admit(data) || last_pass.source_frame_id != render_frame) return;
    RD *rd = RD::get_singleton();
    RID color = RendererRD::TextureStorage::get_singleton()->render_target_get_rd_texture_slice(primary_target, 0);
    if (!color.is_valid()) return;
    auto pass = last_pass; pass.role = RSF_GAME_RENDER_FINAL_SCENE;
    pass.color_input = reinterpret_cast<void *>(rd->get_driver_resource(RD::DRIVER_RESOURCE_TEXTURE, color));
    const bool vulkan = RenderingServer::get_singleton()->get_current_rendering_driver_name() == "vulkan";
    RD::CallbackResource resource{color, RD::CALLBACK_RESOURCE_TYPE_TEXTURE,
        vulkan ? RD::CALLBACK_RESOURCE_USAGE_TEXTURE_SAMPLE : RD::CALLBACK_RESOURCE_USAGE_ATTACHMENT_COLOR_READ_WRITE};
    Packet *packet = packet_for(pass);
    if (vulkan) {
        packet->vk.struct_size = sizeof(packet->vk); packet->vk.abi_version = RSF_GODOT_ABI_VERSION;
        packet->vk.pass = pass; packet->vk.pass.color_input = nullptr;
        packet->vk.device = packet->device;
        packet->vk.instance_proc = reinterpret_cast<rsf_godot_vk_proc>(RenderingDeviceDriverVulkan::rescaleframe_instance_proc());
        packet->vk.device_proc = reinterpret_cast<rsf_godot_vk_proc>(RenderingDeviceDriverVulkan::rescaleframe_device_proc());
        packet->vk.color = {rd->get_driver_resource(RD::DRIVER_RESOURCE_TEXTURE, color),
            uint32_t(rd->get_driver_resource(RD::DRIVER_RESOURCE_TEXTURE_DATA_FORMAT, color)), pass.camera.output_width, pass.camera.output_height,
            uint32_t((rd->texture_get_format(color).usage_bits & RD::TEXTURE_USAGE_CAN_COPY_FROM_BIT) != 0)};
    }
    rd->driver_callback_add(execute, packet, VectorView<RD::CallbackResource>(&resource, 1));
}
void submitted(void *queue, void *const *commands, uint32_t count) { if (ready && enabled.load()) native_submitted(queue, commands, count); }
void present(void *chain) { if (ready && enabled.load()) native_present(chain); }
}
