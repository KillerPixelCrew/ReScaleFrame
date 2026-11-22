// SPDX-License-Identifier: MIT
#include "rsf_godot_engine.h"
#include "api.h"
#include "cpu_facts.h"
#include "frame.h"
#include "graph.h"
#include "host.h"
#include "packet.h"
#include "pass.h"
#include "primary.h"
#include "viewport.h"
#include "servers/rendering/renderer_rd/storage_rd/render_data_rd.h"
#include "core/config/project_settings.h"
#include "core/config/engine.h"
#include "core/math/projection.h"
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

namespace {
decltype(&rsfg_boot) native_boot = nullptr;
decltype(&rsfg_boot_vulkan) native_boot_vulkan = nullptr;
decltype(&rsfg_vk_resolver) native_vk_resolver = nullptr;
decltype(&rsfg_vk_window) native_vk_window = nullptr;
decltype(&rsfg_vk_present_marker) native_vk_present_marker = nullptr;
decltype(&rsfg_vk_execution) native_vk_execution = nullptr;
decltype(&rsfg_vk_native_present) native_vk_native_present = nullptr;
decltype(&rsfg_vk_wsi_call) native_vk_wsi_call = nullptr;
decltype(&rsfg_vk_foreign_call) native_vk_foreign_call = nullptr;
decltype(&rsfg_render_vulkan) native_render_vulkan = nullptr;
decltype(&rsfg_render_vulkan_handoff) native_render_vulkan_handoff = nullptr;
decltype(&rsfg_vk_host_capabilities) native_vk_host_capabilities = nullptr;
decltype(&rsfg_vk_create) native_vk_create = nullptr;
decltype(&rsfg_vk_capture_setup) native_vk_capture_setup = nullptr;
decltype(&rsfg_vk_capture_release) native_vk_capture_release = nullptr;
decltype(&rsfg_vk_images) native_vk_images = nullptr;
decltype(&rsfg_vk_image_layout) native_vk_image_layout = nullptr;
decltype(&rsfg_vk_acquire) native_vk_acquire = nullptr;
decltype(&rsfg_vk_present) native_vk_present = nullptr;
decltype(&rsfg_vk_recording_begin) native_vk_recording_begin = nullptr;

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
// host.rs keeps the ready, enabled, session and requested SR state and decides the boot,
// configure, shutdown and status-driven extent order. The leaves below do the engine work.
HMODULE native_module = nullptr;
String boot_runtime, boot_id;
static_assert(sizeof(real_t) == sizeof(float), "pass.rs takes Godot's jitter and camera scalars as float");

bool configure_viewport(RID viewport, const Dictionary *options, const rsf_godot_settings *typed);

int configure_from_extension(const rsf_godot_settings *value, uint64_t viewport_id) {
    if (!value || value->struct_size < sizeof(*value) || value->abi_version != RSF_GODOT_SETTINGS_ABI_VERSION) return 0;
    auto *bridge = Object::cast_to<ReScaleFrameBridge>(Engine::get_singleton()->get_singleton_object("ReScaleFrame"));
    if (!bridge) return 0;
    return configure_viewport(RID::from_uint64(viewport_id), nullptr, value) ? 1 : 0;
}
void reset_from_extension() { rsf_godot_reset_request(); }

void log(void *, const char *message) { print_line(String("ReScaleFrame: ") + String::utf8(message)); }
void set_render_frame(uint64_t frame, bool gameplay, double seconds);
// Typed engine callbacks for frame.rs, which owns the lifecycle state and calls these in order.
uint64_t frame_now_usec() { return OS::get_singleton()->get_ticks_usec(); }
void report_cpu(uint32_t stage, uint64_t source_frame_id, uint32_t expected, uint32_t kind, uint32_t unframed_input) {
    if (!rsf_godot_host_active()) return;
    LARGE_INTEGER now, frequency;
    QueryPerformanceCounter(&now); QueryPerformanceFrequency(&frequency);
    rsf_game_cpu_event event;
    rsf_godot_build_cpu_event(&event, stage, rsf_godot_host_session(), source_frame_id, uint64_t(now.QuadPart),
        uint64_t(frequency.QuadPart), expected, kind, unframed_input);
    native_cpu(&event);
}
void queue_render(uint64_t frame, double cpu_seconds) {
    const bool gameplay = (!SceneTree::get_singleton() || !SceneTree::get_singleton()->is_paused()) &&
        DisplayServer::get_singleton()->window_is_focused();
    RenderingServer::get_singleton()->call_on_render_thread(callable_mp_static(set_render_frame).bind(frame, gameplay, cpu_seconds));
}
const rsf_godot_frame_callbacks frame_callbacks{frame_now_usec, report_cpu, queue_render};
void set_primary(RID viewport) {
    rsf_godot_primary_set(viewport.get_id(), RSG::viewport->viewport_get_render_target(viewport).get_id());
}
// Driver and Godot leaves for host.rs's status-driven extent.
uint32_t status_extent(rsf_godot_extent *out) {
    rsf_godot_status status{sizeof(status), RSF_GODOT_ABI_VERSION};
    if (!native_status(&status)) return 0;
    *out = {status.requested_sr, status.render_width, status.render_height, status.output_width, status.output_height};
    return 1;
}
void render_begin(uint64_t frame) {
    if (RSFGodot::vk_execution_supported()) {
        RSFGodot::vk_execution(RSF_GAME_RENDER_FRAME, rsf_godot_host_session(), frame, 0, 1);
    } else {
        native_render_begin(frame);
    }
}
void set_scale(uint64_t viewport, double scale) {
    RSG::viewport->viewport_set_scaling_3d_scale(RID::from_uint64(viewport), scale);
}
const rsf_godot_render_callbacks render_callbacks{render_begin, status_extent, set_scale};
void set_render_frame(uint64_t frame, bool gameplay, double seconds) {
    rsf_godot_host_render_begin(frame, gameplay, seconds, &render_callbacks);
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
void execute(RenderingDeviceDriver *driver, RenderingDeviceDriver::CommandBufferID commands, void *userdata) {
    struct Context {
        RenderingDeviceDriver *driver;
        RenderingDeviceDriver::CommandBufferID commands;
    } context{driver, commands};
    const rsf_godot_packet_calls calls{
        [](void *opaque, uint32_t route) -> void * {
            const auto &lease = *static_cast<Context *>(opaque);
            if (route == RSF_GODOT_DRIVER_VULKAN) {
                return static_cast<RenderingDeviceDriverVulkan *>(lease.driver)->rescaleframe_begin(lease.commands);
            }
            return static_cast<RenderingDeviceDriverD3D12 *>(lease.driver)->rescaleframe_begin(lease.commands);
        },
        [](void *opaque) {
            const auto &lease = *static_cast<Context *>(opaque);
            static_cast<RenderingDeviceDriverD3D12 *>(lease.driver)->rescaleframe_end(lease.commands);
        },
        [](void *opaque, rsf_godot_vk_handoff *out) -> uint32_t {
            const auto &lease = *static_cast<Context *>(opaque);
            return RSFGodot::GraphScope::handoff(lease.driver, lease.commands, *out);
        },
        native_render, native_render_vulkan, native_render_vulkan_handoff,
    };
    auto *packet = static_cast<rsf_godot_driver_packet *>(userdata);
    rsf_godot_packet_execute(packet, &calls, &context);
    memdelete(packet);
}
// Engine facts for primary.rs's admission. Every read is a pure query.
rsf_godot_render_admission admission(const RenderDataRD *data, uint64_t frame) {
    rsf_godot_render_admission facts{frame, 0, rsf_godot_host_active(), 0, 0, RSF_GODOT_DRIVER_OTHER};
    if (data && data->render_buffers.is_valid()) {
        facts.buffers_valid = 1;
        facts.render_target = data->render_buffers->get_render_target().get_id();
        facts.view_count = data->render_buffers->get_view_count();
    }
    const String driver = RenderingServer::get_singleton()->get_current_rendering_driver_name();
    facts.driver = driver == "d3d12" ? RSF_GODOT_DRIVER_D3D12 : driver == "vulkan" ? RSF_GODOT_DRIVER_VULKAN : RSF_GODOT_DRIVER_OTHER;
    return facts;
}
rsf_godot_driver_packet *packet_for(RD *rd, const rsf_godot_pass_record &record,
        RID color, RID depth = RID(), RID motion = RID(), RID output = RID()) {
    rsf_godot_packet_facts facts{};
    facts.device = reinterpret_cast<void *>(rd->get_driver_resource(RD::DRIVER_RESOURCE_LOGICAL_DEVICE));
    if (record.driver == RSF_GODOT_DRIVER_VULKAN) {
        if (record.role == RSF_GAME_RENDER_SR) {
            facts.instance = reinterpret_cast<void *>(rd->get_driver_resource(RD::DRIVER_RESOURCE_TOPMOST_OBJECT));
            facts.physical_device = reinterpret_cast<void *>(rd->get_driver_resource(RD::DRIVER_RESOURCE_PHYSICAL_DEVICE));
        }
        facts.instance_proc = reinterpret_cast<rsf_godot_vk_proc>(RenderingDeviceDriverVulkan::rescaleframe_instance_proc());
        facts.device_proc = reinterpret_cast<rsf_godot_vk_proc>(RenderingDeviceDriverVulkan::rescaleframe_device_proc());
        auto describe = [rd](RID texture) -> rsf_godot_vk_image {
            if (!texture.is_valid()) return {};
            return {rd->get_driver_resource(RD::DRIVER_RESOURCE_TEXTURE, texture),
                uint32_t(rd->get_driver_resource(RD::DRIVER_RESOURCE_TEXTURE_DATA_FORMAT, texture)), 0, 0,
                uint32_t((rd->texture_get_format(texture).usage_bits & RD::TEXTURE_USAGE_CAN_COPY_FROM_BIT) != 0)};
        };
        facts.vk_color = describe(color);
        facts.vk_depth = describe(depth);
        facts.vk_motion = describe(motion);
        facts.vk_output = describe(output);
    } else {
        facts.queue = reinterpret_cast<void *>(rd->get_driver_resource(RD::DRIVER_RESOURCE_COMMAND_QUEUE));
        auto native = [rd](RID texture) -> void * {
            return texture.is_valid() ? reinterpret_cast<void *>(rd->get_driver_resource(RD::DRIVER_RESOURCE_TEXTURE, texture)) : nullptr;
        };
        facts.color = native(color);
        facts.depth = native(depth);
        facts.motion = native(motion);
        facts.output = native(output);
    }
    auto *packet = memnew(rsf_godot_driver_packet);
    rsf_godot_packet_prepare(&record, &facts, packet);
    return packet;
}
}

void ReScaleFrameBridge::_bind_methods() {
    ClassDB::bind_method(D_METHOD("configure", "options", "viewport"), &ReScaleFrameBridge::configure);
    ClassDB::bind_method(D_METHOD("get_status"), &ReScaleFrameBridge::get_status);
    ClassDB::bind_method(D_METHOD("reset_history"), &ReScaleFrameBridge::reset_history);
}
void ReScaleFrameBridge::reset_history() { rsf_godot_reset_request(); }
namespace {
// Godot and driver leaves for host.rs's configure sequence.
struct ConfigureContext {
    const Dictionary *options;
    const rsf_godot_settings *typed;
    RID viewport;
    rsf_godot_settings value{sizeof(value), RSF_GODOT_SETTINGS_ABI_VERSION, 7, 1, 0, 7, 0, 1, 0, 0, 1};
};
uint32_t configure_is_root(void *context) {
    const RID viewport = static_cast<ConfigureContext *>(context)->viewport;
    SceneTree *tree = SceneTree::get_singleton();
    return tree && tree->get_root() && tree->get_root()->get_viewport_rid() == viewport;
}
// Godot's Variant conversions, which differ from the GDExtension's checked integer parser.
const rsf_godot_settings *configure_parse(void *context) {
    auto &request = *static_cast<ConfigureContext *>(context);
    if (request.typed) return request.typed;
    const Dictionary &options = *request.options;
    rsf_godot_settings &value = request.value;
    value.sr_backend = options.get("sr_backend", value.sr_backend);
    value.quality = options.get("quality", value.quality);
    value.dlss_preset = options.get("dlss_preset", value.dlss_preset);
    value.sharpening=rsf_sharpening_defaults();
    value.sharpening.mode=options.get("sharpening_mode",value.sharpening.mode);
    value.sharpening.sharpness=options.get("sharpening_sharpness",value.sharpening.sharpness);
    value.sharpening.mas=options.get("sharpening_mas",value.sharpening.mas);
    value.sharpening.motion_sharpness=options.get("sharpening_motion_sharpness",value.sharpening.motion_sharpness);
    value.sharpening.motion_threshold=options.get("sharpening_motion_threshold",value.sharpening.motion_threshold);
    value.sharpening.motion_scale_limit=options.get("sharpening_motion_scale_limit",value.sharpening.motion_scale_limit);
    value.sharpening.contrast_enabled=options.get("sharpening_contrast_enabled",value.sharpening.contrast_enabled);
    value.sharpening.contrast=options.get("sharpening_contrast",value.sharpening.contrast);
    value.sharpening.depth_scale=options.get("sharpening_depth_scale",value.sharpening.depth_scale);
    value.sharpening.depth_bias=options.get("sharpening_depth_bias",value.sharpening.depth_bias);
    value.sharpening.clamp_output=options.get("sharpening_clamp_output",value.sharpening.clamp_output);
    value.sharpening.debug=options.get("sharpening_debug",value.sharpening.debug);
    value.fg_backend = options.get("fg_backend", value.fg_backend);
    value.fg_mode = options.get("fg_mode", value.fg_mode);
    value.generated_frames = options.get("generated_frames", value.generated_frames);
    value.latency_mode = options.get("latency_mode", value.latency_mode);
    value.frame_limit_us = options.get("frame_limit_us", value.frame_limit_us);
    value.gameplay = options.get("gameplay", value.gameplay);
    return &value;
}
void configure_report_window(void *) {
    native_vk_window(reinterpret_cast<void *>(DisplayServer::get_singleton()->window_get_native_handle(DisplayServerEnums::WINDOW_HANDLE)));
}
uint32_t configure_native(void *, const rsf_godot_settings *settings) { return native_configure(settings) ? 1 : 0; }
void configure_queue_primary(void *context) {
    RenderingServer::get_singleton()->call_on_render_thread(callable_mp_static(set_primary).bind(static_cast<ConfigureContext *>(context)->viewport));
}
void configure_apply_scaling(void *context, uint32_t mode, float scale) {
    const RID viewport = static_cast<ConfigureContext *>(context)->viewport;
    RenderingServer *rs = RenderingServer::get_singleton();
    rs->viewport_set_scaling_3d_mode(viewport, mode == RSF_GODOT_VIEWPORT_NATIVE ? RSE::VIEWPORT_SCALING_3D_MODE_BILINEAR :
        mode == RSF_GODOT_VIEWPORT_SPATIAL ? RSE::VIEWPORT_SCALING_3D_MODE_FSR : RSE::VIEWPORT_SCALING_3D_MODE_FSR2);
    rs->viewport_set_scaling_3d_scale(viewport, scale);
}
bool configure_viewport(RID viewport, const Dictionary *options, const rsf_godot_settings *typed) {
    ConfigureContext context{options, typed, viewport};
    const rsf_godot_configure_facts facts{uint32_t(viewport.is_valid()),
        uint32_t(OS::get_singleton()->get_current_rendering_driver_name() == "vulkan")};
    const rsf_godot_configure_callbacks callbacks{&context, configure_is_root, configure_parse,
        configure_report_window, configure_native, configure_queue_primary, configure_apply_scaling};
    return rsf_godot_host_configure(&facts, &callbacks) != 0;
}
}
bool ReScaleFrameBridge::configure(const Dictionary &options, RID viewport) {
    return configure_viewport(viewport, &options, nullptr);
}
Dictionary ReScaleFrameBridge::get_status() const {
    Dictionary result;
    const bool ready = rsf_godot_host_ready();
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
    RSF_FIELD(latency_owner); RSF_FIELD(input_substitutes); RSF_FIELD(order_violations); RSF_FIELD(derived_boundaries);
    RSF_FIELD(keyboard_inputs); RSF_FIELD(mouse_inputs); RSF_FIELD(controller_inputs); RSF_FIELD(pcl_pings);
#undef RSF_FIELD
    return result;
}

namespace {
// Module loader and driver leaves for host.rs's boot.
uint32_t boot_load() {
    const String library = disk_path(GLOBAL_DEF("rescaleframe/native_library", "res://addons/rescaleframe/bin/ReScaleFrame.Godot.Native.dll"));
    boot_runtime = disk_path(GLOBAL_DEF("rescaleframe/runtime_directory", "res://addons/rescaleframe/runtime"));
    boot_id = GLOBAL_DEF("rescaleframe/project_id", "125d2115-7176-46e1-8806-95b790f01717");
    HMODULE module = LoadLibraryW(reinterpret_cast<const wchar_t *>(library.utf16().get_data()));
    if (!module) { ERR_PRINT("Cannot load ReScaleFrame's native library: " + library + " (Windows error " + itos(GetLastError()) + ")"); return 0; }
    native_module = module;
#define RSF_RESOLVE(name) native_##name = reinterpret_cast<decltype(native_##name)>(GetProcAddress(module, "rsfg_" #name)); if (!native_##name) return 0
    RSF_RESOLVE(boot); RSF_RESOLVE(boot_vulkan); RSF_RESOLVE(render_vulkan);
    RSF_RESOLVE(render_vulkan_handoff); RSF_RESOLVE(vk_host_capabilities);
    RSF_RESOLVE(vk_resolver); RSF_RESOLVE(vk_window); RSF_RESOLVE(vk_present_marker); RSF_RESOLVE(vk_native_present); RSF_RESOLVE(vk_wsi_call);
    RSF_RESOLVE(vk_create); RSF_RESOLVE(vk_images); RSF_RESOLVE(vk_acquire); RSF_RESOLVE(vk_present); RSF_RESOLVE(vk_submitted); RSF_RESOLVE(vk_recording_begin); RSF_RESOLVE(vk_destroy);
    RSF_RESOLVE(vk_drain); RSF_RESOLVE(vk_image_layout); RSF_RESOLVE(vk_foreign_call);
    RSF_RESOLVE(create_swapchain); RSF_RESOLVE(configure); RSF_RESOLVE(status); RSF_RESOLVE(render);
    RSF_RESOLVE(cpu); RSF_RESOLVE(render_begin); RSF_RESOLVE(submitted); RSF_RESOLVE(present); RSF_RESOLVE(shutdown);
#undef RSF_RESOLVE
    native_vk_execution = reinterpret_cast<decltype(native_vk_execution)>(GetProcAddress(module, "rsfg_vk_execution"));
    native_vk_capture_setup = reinterpret_cast<decltype(native_vk_capture_setup)>(GetProcAddress(module, "rsfg_vk_capture_setup"));
    native_vk_capture_release = reinterpret_cast<decltype(native_vk_capture_release)>(GetProcAddress(module, "rsfg_vk_capture_release"));
    return 1;
}
void boot_capabilities(uint32_t flags) { native_vk_host_capabilities(flags); }
uint32_t boot_start(uint32_t vulkan) {
    const CharString path_utf8 = boot_runtime.utf8(), id_utf8 = boot_id.utf8();
    const rsf_godot_setup setup{sizeof(setup), RSF_GODOT_ABI_VERSION, path_utf8.get_data(), "4.7.2", id_utf8.get_data(), log, nullptr};
    const bool started = (vulkan ? native_boot_vulkan(&setup) : native_boot(&setup)) != 0;
    // The native host copies the setup strings; free them as the old stack locals were.
    boot_runtime = String(); boot_id = String();
    return started;
}
void boot_rejected() { ERR_PRINT("ReScaleFrame native host rejected startup; inspect its dependency and initialization diagnostics."); }
uint32_t boot_attach() {
    auto attach = reinterpret_cast<decltype(&rsfg_attach_engine)>(GetProcAddress(native_module, "rsfg_attach_engine"));
    const rsf_godot_engine_services services{sizeof(services), RSF_GODOT_ABI_VERSION, configure_from_extension, reset_from_extension};
    return attach && attach(&services);
}
uint64_t boot_session() {
    rsf_godot_status status{sizeof(status), RSF_GODOT_ABI_VERSION};
    return native_status(&status) ? status.session_id : 0;
}
// Godot and driver leaves for host.rs's shutdown.
void shutdown_release_render() {
    if (auto *rs = RenderingServer::get_singleton()) {
        rs->call_on_render_thread(callable_mp_static(RSFGodot::release_motion));
        rs->sync();
    }
}
uint32_t shutdown_native() {
    if (native_shutdown()) return 1;
    ERR_PRINT("ReScaleFrame native host rejected shutdown.");
    return 0;
}
const rsf_godot_shutdown_callbacks shutdown_callbacks{shutdown_release_render, shutdown_native};
}

namespace RSFGodot {
void boot(bool headless) {
    const bool enabled_setting = GLOBAL_DEF("rescaleframe/enabled", false);
    const rsf_godot_boot_facts facts{uint32_t(enabled_setting), uint32_t(headless), uint32_t(Engine::get_singleton()->is_editor_hint()),
        uint32_t(OS::get_singleton()->get_current_rendering_driver_name() == "vulkan")};
    const rsf_godot_boot_callbacks callbacks{boot_load, boot_capabilities, boot_start, boot_rejected, boot_attach, boot_session, shutdown_native};
    rsf_godot_host_boot(&facts, &callbacks);
}
bool create_swapchain(void *factory, void *queue, void *window, const void *description, void **out) {
    return rsf_godot_host_ready() && SUCCEEDED(native_create_swapchain(factory, queue, window, description, out));
}
bool vulkan_enabled() { return rsf_godot_host_ready() && OS::get_singleton()->get_current_rendering_driver_name() == "vulkan"; }
uint64_t vk_resolver() { return vulkan_enabled() ? reinterpret_cast<uint64_t>(native_vk_resolver()) : 0; }
bool vk_present_marker(uint64_t window, uint32_t begin) { return vulkan_enabled() && native_vk_present_marker(window, begin); }
bool vk_execution_supported() { return vulkan_enabled() && native_vk_execution; }
bool vk_execution(uint32_t role, uint64_t session, uint64_t source, uint64_t window, uint32_t begin) {
    if (!vk_execution_supported()) return false;
    rsf_game_render_pass pass{};
    rsf_godot_execution_pass(role, session, source, window, &pass);
    return native_vk_execution(&pass, begin) != 0;
}

int32_t vk_native_present(uint64_t callback, void* queue, const void* info) {
    if (vulkan_enabled()) return native_vk_native_present(callback, queue, info);
    using Present = int32_t (*)(void*, const void*);
    return reinterpret_cast<Present>(callback)(queue, info);
}
int32_t vk_wsi_call(int32_t (*callback)(void*), void* user) { return vulkan_enabled() ? native_vk_wsi_call(callback, user) : callback(user); }
int32_t vk_foreign_call(int32_t (*callback)(void*), void* user) { return vulkan_enabled() ? native_vk_foreign_call(callback, user) : callback(user); }
bool vk_create(const void *setup, void **owner, uint64_t *chain) {
    return vulkan_enabled() && native_vk_create(static_cast<const rsf_fgvk_setup *>(setup), owner, chain) == 0;
}
void vk_capture_setup(const void *setup) {
    if (vulkan_enabled() && native_vk_capture_setup) native_vk_capture_setup(static_cast<const rsf_fgvk_setup *>(setup));
}
bool vk_capture_release(void *queue_user) {
    return !vulkan_enabled() || !native_vk_capture_release || native_vk_capture_release(queue_user) == 0;
}
int32_t vk_images(void *owner, uint32_t *count, uint64_t *images) { return native_vk_images(owner, count, images); }
uint32_t vk_image_layout(void *owner) { return native_vk_image_layout(owner); }
int32_t vk_acquire(void *owner, uint64_t timeout, uint64_t semaphore, uint64_t fence, uint32_t *index) { return native_vk_acquire(owner, timeout, semaphore, fence, index); }
int32_t vk_present(void *owner, const void *info) { return native_vk_present(owner, info); }
void vk_recording_begin(void* commands) { if (rsf_godot_host_ready()) native_vk_recording_begin(commands); }
void vk_submitted(void *const *commands, uint32_t count) { if (rsf_godot_host_ready()) native_vk_submitted(commands, count); }
bool vk_destroy(void *owner) { return native_vk_destroy(owner) == 0; }
bool vk_drain(void *owner) { return native_vk_drain(owner) == 0; }
void shutdown() { rsf_godot_host_shutdown(&shutdown_callbacks); }
void cpu_begin() {
    if (rsf_godot_host_active()) rsf_godot_cpu_begin(&frame_callbacks);
}
void cpu_sample() {
    if (rsf_godot_host_active()) rsf_godot_cpu_sample(&frame_callbacks);
}
void cpu_simulation_begin() { rsf_godot_cpu_simulation_begin(&frame_callbacks); }
void queue_frame(bool present) {
    if (rsf_godot_host_active()) rsf_godot_cpu_queue(&frame_callbacks, present ? 1u : 0u);
}
void cpu_end() { rsf_godot_cpu_end(&frame_callbacks); }
void input_event(uint32_t kind) {
    if (!rsf_godot_host_active() || !kind || (kind & ~7u)) return;
    rsf_godot_cpu_input(&frame_callbacks, kind);
}
void render_size(RID target, const Vector2i &output, Vector2i &internal) {
    int32_t size[2] = {internal.x, internal.y};
    rsf_godot_host_render_size(target.get_id(), output.x, output.y, size, status_extent);
    internal = Vector2i(size[0], size[1]);
}
void camera_changed(RID target) {
    rsf_godot_camera_changed(rsf_godot_host_active(), target.get_id());
}
void upscale(const RenderDataRD *data, double) {
    rsf_godot_render_frame frame;
    rsf_godot_render_frame_read(&frame);
    const rsf_godot_render_admission facts = admission(data, frame.source_frame_id);
    if (!rsf_godot_sr_admit(&facts)) return;
    Ref<RenderSceneBuffersRD> rb = data->render_buffers;
    RD *rd = RD::get_singleton();
    rb->ensure_upscaled();
    const RID color = rb->get_internal_texture(0), depth = rb->get_depth_texture(0), motion = rb->get_velocity_buffer(false, 0), output = rb->get_upscaled_texture(0);
    if (!color.is_valid() || !depth.is_valid() || !motion.is_valid() || !output.is_valid()) return;
    const RenderSceneDataRD *scene = data->scene_data;
    const Vector2i internal = rb->get_internal_size(), target = rb->get_target_size();
    float origin[3];
    rsf_godot_sr_previous_origin(origin);
    const Vector3 previous_origin(origin[0], origin[1], origin[2]);
    const rsf_godot_sr_facts sr{
        {color.get_id(), internal.x, internal.y, frame.frame_seconds,
            double(previous_origin.distance_to(scene->cam_transform.origin)), frame.sequence_break, 0},
        frame.source_frame_id, rb->get_render_target().get_id(), target.x, target.y, scene->z_near, scene->z_far,
        Math::deg_to_rad(scene->cam_projection.get_fovy(scene->cam_projection.get_fov(), 1.0 / scene->cam_projection.get_aspect())),
        {scene->taa_jitter.x, scene->taa_jitter.y}, {scene->prev_taa_jitter.x, scene->prev_taa_jitter.y},
        frame.gameplay ? RSF_SCREEN_FLIGHT : RSF_SCREEN_MENU, facts.driver, 0};
    rsf_godot_pass_record record;
    rsf_godot_sr_plan(&sr, &record);
    float native_projection[16], view_to_world[16], world_to_view[16];
    matrix(native_projection, scene->cam_projection);
    matrix(view_to_world, Projection(scene->cam_transform));
    matrix(world_to_view, Projection(scene->cam_transform.affine_inverse()));
    rsf_godot_sr_camera(&record, native_projection, view_to_world, world_to_view,
            [](const float *input, float *output) {
                Projection projection;
                for (int column = 0; column < 4; ++column)
                    for (int row = 0; row < 4; ++row) projection.columns[column][row] = input[column * 4 + row];
                matrix(output, projection.inverse());
            });
    RID resolved_depth;
    const RID resolved_motion = resolve_motion(rb, depth, motion, record.camera.clip_to_previous_clip, scene->taa_jitter,
            record.normalized_depth != 0, resolved_depth);
    if (!resolved_motion.is_valid() || !resolved_depth.is_valid()) return;
    rsf_godot_sr_record(&sr, &record);
    RD::CallbackResource resources[] = {
        {color, RD::CALLBACK_RESOURCE_TYPE_TEXTURE, RD::CALLBACK_RESOURCE_USAGE_TEXTURE_SAMPLE},
        {resolved_depth, RD::CALLBACK_RESOURCE_TYPE_TEXTURE, RD::CALLBACK_RESOURCE_USAGE_TEXTURE_SAMPLE},
        {resolved_motion, RD::CALLBACK_RESOURCE_TYPE_TEXTURE, RD::CALLBACK_RESOURCE_USAGE_TEXTURE_SAMPLE},
        {output, RD::CALLBACK_RESOURCE_TYPE_TEXTURE, RD::CALLBACK_RESOURCE_USAGE_COPY_TO}};
    rd->driver_callback_add(execute, packet_for(rd, record, color, resolved_depth, resolved_motion, output),
            VectorView<RD::CallbackResource>(resources, 4));
}
void hudless(const RenderDataRD *data) {
    rsf_godot_render_frame frame;
    rsf_godot_render_frame_read(&frame);
    const rsf_godot_render_admission facts = admission(data, frame.source_frame_id);
    rsf_godot_pass_record record;
    if (!rsf_godot_final_scene_plan(&facts, &record)) return;
    RD *rd = RD::get_singleton();
    // Admission proved these buffers render to the primary target.
    RID color = RendererRD::TextureStorage::get_singleton()->render_target_get_rd_texture_slice(data->render_buffers->get_render_target(), 0);
    if (!color.is_valid()) return;
    // The last recorded SR pass with the final-scene role and no SR resources.
    const bool vulkan = record.driver == RSF_GODOT_DRIVER_VULKAN;
    RD::CallbackResource resource{color, RD::CALLBACK_RESOURCE_TYPE_TEXTURE,
        vulkan ? RD::CALLBACK_RESOURCE_USAGE_TEXTURE_SAMPLE : RD::CALLBACK_RESOURCE_USAGE_ATTACHMENT_COLOR_READ_WRITE};
    rd->driver_callback_add(execute, packet_for(rd, record, color), VectorView<RD::CallbackResource>(&resource, 1));
}
void submitted(void *queue, void *const *commands, uint32_t count) { if (rsf_godot_host_active()) native_submitted(queue, commands, count); }
void present(void *chain) { if (rsf_godot_host_active()) native_present(chain); }
}
