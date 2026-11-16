// SPDX-License-Identifier: MIT
#include <rescaleframe/d3d11_host.h>
#include <rescaleframe/native_sr.h>
#include <rescaleframe/native_fg.h>
#include <cstdio>
#include <cstddef>
static_assert(sizeof(rsf_d3d11_reduced_frame) == 56);
static_assert(alignof(rsf_d3d11_reduced_frame) == 8);
static_assert(offsetof(rsf_d3d11_reduced_frame,token) == 16);
static_assert(offsetof(rsf_d3d11_reduced_frame,owner) == 24);
static_assert(offsetof(rsf_d3d11_reduced_frame,surfaces) == 40);
static_assert(offsetof(rsf_d3d11_reduced_frame,temporal_result) == 48);
static_assert(sizeof(rsf_d3d11_chain_dispatch) == 72);
static_assert(offsetof(rsf_d3d11_chain_dispatch, receiver) == 16);
static_assert(offsetof(rsf_d3d11_chain_dispatch, args) == 24);
static_assert(offsetof(rsf_d3d11_chain_dispatch, result) == 64);
#include <limits>
namespace {
unsigned errors{}, cpu_calls{}, logs{};
int cookie{};
void check(bool ok, const char* message) { if (!ok) { ++errors; std::printf("FAIL %s\n", message); } }
void cpu(void* user, const rsf_game_cpu_event* event) { check(user == &cookie && event->session_id == 91, "CPU upstream user and event preserved"); ++cpu_calls; }
void log(void* user, const char*) { check(user == &cookie, "Log upstream user preserved"); ++logs; }
}
int main() {
    static_assert(sizeof(rsf_d3d11_capture_control)==16);
    rsf_d3d11_capture_control capture_control{sizeof(capture_control),RSF_D3D11_CAPTURE_CONTROL_VERSION,RSF_D3D11_CAPTURE_SEQUENCE,1};
    check(rsf_d3d11_host_capture_control(nullptr)==RSF_ERROR_INVALID_ARGUMENT,"null capture_control control refused");
    --capture_control.struct_size;
    check(rsf_d3d11_host_capture_control(&capture_control)==RSF_ERROR_INVALID_ARGUMENT,"short capture_control control refused");
    ++capture_control.struct_size; ++capture_control.abi_version;
    check(rsf_d3d11_host_capture_control(&capture_control)==RSF_ERROR_ABI_MISMATCH,"capture_control control version checked");
    --capture_control.abi_version; capture_control.value=0;
    check(rsf_d3d11_host_capture_control(&capture_control)==RSF_ERROR_INVALID_ARGUMENT,"empty sequence request refused");
    capture_control.value=1;
    check(rsf_d3d11_host_capture_control(&capture_control)==RSF_ERROR_NOT_READY,"capture_control requires live host");

    rsf_d3d11_host_setup setup{}; setup.struct_size = sizeof(setup); setup.abi_version = RSF_D3D11_HOST_ABI_VERSION;
    setup.configuration_path = L"unused.ini"; setup.configuration_section = L"Test"; setup.runtime_directory = L"unused";
    setup.identity_utf8 = "contract"; setup.executable_sha256 = "unused"; setup.engine_version_utf8 = "1";
    setup.project_id_utf8 = "668392a0-ecfe-4d0c-9f58-c2838c5ca308";
    setup.units_to_meters = 1; setup.quality_choices = 1;
    setup.motion.struct_size = sizeof(setup.motion); setup.motion.scale_x = setup.motion.scale_y = 1;
    rsf_game_host_services host{sizeof(host), RSF_GAME_ABI_VERSION, 91, log, nullptr, &cookie, nullptr, cpu};
    check(rsf_d3d11_host_create(nullptr, &host) == RSF_ERROR_INVALID_ARGUMENT, "Null setup refused");
    --setup.struct_size; check(rsf_d3d11_host_create(&setup, &host) == RSF_ERROR_INVALID_ARGUMENT, "Short setup refused"); ++setup.struct_size;
    ++setup.abi_version; check(rsf_d3d11_host_create(&setup, &host) == RSF_ERROR_ABI_MISMATCH, "Setup ABI checked"); --setup.abi_version;
    --host.struct_size; check(rsf_d3d11_host_create(&setup, &host) == RSF_ERROR_INVALID_ARGUMENT, "Short services refused"); ++host.struct_size;
    ++host.abi_version; check(rsf_d3d11_host_create(&setup, &host) == RSF_ERROR_ABI_MISMATCH, "Services ABI checked"); --host.abi_version;
    setup.colour_transport_default = 2;
    check(rsf_d3d11_host_create(&setup, &host) == RSF_ERROR_INVALID_ARGUMENT, "Unknown colour transport default refused");
    setup.colour_transport_default = 0; // existing callers preserve the enabled default
    setup.units_to_meters = std::numeric_limits<float>::quiet_NaN();
    check(rsf_d3d11_host_create(&setup, &host) == RSF_ERROR_INVALID_ARGUMENT, "Nonfinite units refused"); setup.units_to_meters = 1;
    setup.motion.scale_x = 0; check(rsf_d3d11_host_create(&setup, &host) == RSF_ERROR_INVALID_ARGUMENT, "Zero decode scale refused"); setup.motion.scale_x = 1;
    setup.quality_choices = 0; check(rsf_d3d11_host_create(&setup, &host) == RSF_ERROR_INVALID_ARGUMENT, "Empty capabilities refused"); setup.quality_choices = 1;
    setup.presentation_bridge = 1;
    check(rsf_d3d11_host_create(&setup, &host) == RSF_ERROR_INVALID_ARGUMENT,
        "Cold presentation requires an adapter window filter before any hooks are installed");
    setup.presentation_bridge = 4;
    check(rsf_d3d11_host_create(&setup, &host) == RSF_ERROR_INVALID_ARGUMENT, "Unknown presentation mode refused");
    setup.presentation_bridge = 0;
    setup.capabilities = 8;
    check(rsf_d3d11_host_create(&setup, &host) == RSF_ERROR_INVALID_ARGUMENT, "Unknown capability refused");
    setup.capabilities = RSF_D3D11_HOST_CAP_FG_INPUTS;
    check(rsf_d3d11_host_create(&setup, &host) == RSF_ERROR_INVALID_ARGUMENT, "FG facts require presentation bridge");
    setup.capabilities = RSF_D3D11_HOST_CAP_INPUT_TIMELINE;
    check(rsf_d3d11_host_create(&setup, &host) == RSF_ERROR_INVALID_ARGUMENT, "Input timeline requires presentation bridge");
    setup.capabilities = RSF_D3D11_HOST_CAP_RENDERED_SOURCE;
    check(rsf_d3d11_host_create(&setup, &host) == RSF_ERROR_INVALID_ARGUMENT, "Rendered source requires presentation bridge");
    setup.capabilities = 0;
    rsf_frame_record rendered{}; rendered.struct_size = sizeof(rendered);
    rendered.source_provenance = RSF_FRAME_SOURCE_RENDERED;
    check(!rsf_frame_has_rendered_source(&rendered), "Rendered source requires distinct timestamp and frequency");
    rendered.source_qpc = 10; rendered.source_qpc_frequency = 1000;
    check(rsf_frame_has_rendered_source(&rendered), "Rendered source admits unknown physical input timestamp");
    rendered.input_qpc = 10;
    check(!rsf_frame_has_rendered_source(&rendered), "Rendered provenance cannot claim a physical input timestamp");
    rsf_frame_record gameplay{}; gameplay.struct_size = sizeof(gameplay); gameplay.screen = RSF_SCREEN_GAMEPLAY;
    check(rsf_frame_allows_fg(&gameplay), "Generic gameplay eligible for FG screen policy");
    gameplay.flags = RSF_FRAME_FLAG_NO_FG;
    check(!rsf_frame_allows_fg(&gameplay), "Gameplay cannot override NO_FG");
    rsf_game_render_pass outer{sizeof(outer)}; outer.source_frame_id = 42;
    check(rsf_native_fg_scene_swap(&outer) == 1, "Scene scope saved");
    rsf_game_render_pass inner{sizeof(inner)}; inner.source_frame_id = 43;
    check(rsf_native_fg_scene_swap(&inner) == 1 && inner.source_frame_id == 42, "Nested scene saved exactly");
    check(rsf_native_fg_scene_swap(&inner) == 1 && inner.source_frame_id == 43, "Nested scene restored exactly");
    check(rsf_native_fg_scene_swap(&outer) == 1 && outer.source_frame_id == 42, "Outer scene restored exactly");
    rsf_game_render_pass inputs{}; inputs.struct_size = sizeof(inputs); inputs.role = RSF_GAME_RENDER_FG_INPUTS;
    auto* invalid_context = reinterpret_cast<void*>(uintptr_t(1));
    check(rsf_native_sr_capture_fg(invalid_context, &inputs, &setup.motion) == RSF_ERROR_NOT_READY,
        "Unidentified FG lease refused before graphics dereference");
    --inputs.struct_size;
    check(rsf_native_sr_capture_fg(invalid_context, &inputs, &setup.motion) == RSF_ERROR_INVALID_ARGUMENT,
        "Short FG lease refused before graphics dereference");
    auto upstream = host;
    check(rsf_d3d11_host_create(&setup, &host) == RSF_OK, "Graphics-free creation succeeds without SDK directories");
    check(host.session_id == 91 && host.user != &cookie && host.process_upscale && host.render_config && host.render_feedback && host.render_pass &&
        !host.present && !host.submit && !host.process_latency, "Only admitted host services published; no pacing or generation");
    check(rsf_d3d11_host_create(&setup, &upstream) == RSF_ERROR_BUSY, "Second process owner refused");
    check(rsf_d3d11_host_set_active(&cookie, 1) == RSF_ERROR_INVALID_ARGUMENT, "Only the returned owner controls activation");
    check(rsf_d3d11_host_set_active(host.user, 1) == RSF_OK && rsf_d3d11_host_set_active(host.user, 0) == RSF_OK, "Thread-safe activation and suspension");
    rsf_d3d11_reduced_frame recovery{sizeof(recovery),RSF_D3D11_REDUCED_FRAME_ABI_VERSION};
    void* unreadable=reinterpret_cast<void*>(uintptr_t(1));
    check(rsf_d3d11_host_reduced_frame(&cookie,unreadable,&recovery)==RSF_ERROR_INVALID_ARGUMENT,"recovery validates host before graphics");
    --recovery.struct_size;
    check(rsf_d3d11_host_reduced_frame(host.user,unreadable,&recovery)==RSF_ERROR_INVALID_ARGUMENT,"recovery validates size before graphics");
    ++recovery.struct_size; ++recovery.abi_version;
    check(rsf_d3d11_host_reduced_frame(host.user,unreadable,&recovery)==RSF_ERROR_ABI_MISMATCH,"independent recovery negotiation");
    --recovery.abi_version; recovery.operation=5;
    check(rsf_d3d11_host_reduced_frame(host.user,unreadable,&recovery)==RSF_ERROR_INVALID_ARGUMENT,"unknown recovery operation refused");
    recovery.operation=0; recovery.reserved=1;
    check(rsf_d3d11_host_reduced_frame(host.user,unreadable,&recovery)==RSF_ERROR_INVALID_ARGUMENT,"reserved recovery bits refused");
    recovery.reserved=0; recovery.owner=static_cast<const rsf_game_render_pass*>(unreadable);
    check(rsf_d3d11_host_reduced_frame(host.user,unreadable,&recovery)==RSF_ERROR_NOT_READY,"unconfigured recovery owner refused before resource reads");
    rsf_d3d11_auxiliary_scope cleanup{sizeof(cleanup),RSF_D3D11_HOST_ABI_VERSION,91,1,1};
    check(rsf_d3d11_host_restore_auxiliaries(&cookie,&cleanup)==RSF_ERROR_INVALID_ARGUMENT,"Auxiliary cleanup verifies host user");
    --cleanup.struct_size;
    check(rsf_d3d11_host_restore_auxiliaries(host.user,&cleanup)==RSF_ERROR_INVALID_ARGUMENT,"Auxiliary cleanup verifies scope size");
    ++cleanup.struct_size; ++cleanup.abi_version;
    check(rsf_d3d11_host_restore_auxiliaries(host.user,&cleanup)==RSF_ERROR_ABI_MISMATCH,"Auxiliary cleanup verifies ABI before empty acknowledgement");
    --cleanup.abi_version;
    check(rsf_d3d11_host_restore_auxiliaries(host.user,&cleanup)==RSF_ERROR_NOT_READY,"Unbound graphics owner cannot acknowledge empty cleanup");
    rsf_game_cpu_event event{}; event.struct_size = sizeof(event); event.session_id = 91;
    host.cpu_event(host.user, &event); host.log(host.user, "contract");
    check(cpu_calls == 1 && logs == 1, "Callbacks forwarded exactly once");
    rsf_game_render_config config{sizeof(config)};
    check(host.render_config(host.user, &config) == 1 && !config.enabled && !config.output_width, "Cold config does not invent graphics dimensions");
    rsf_game_render_pass early{}; early.struct_size = sizeof(early); early.role = RSF_GAME_RENDER_SR;
    early.flags = RSF_GAME_RENDER_RECONFIGURE; early.swapchain = reinterpret_cast<void*>(uintptr_t(1));
    early.camera.output_width = early.camera.output_height = 64;
    check(rsf_d3d11_host_set_active(host.user, 1) == RSF_OK, "Activate for early planning refusal");
    check(host.process_upscale(host.user, reinterpret_cast<void*>(uintptr_t(1)), &early) == RSF_ERROR_NOT_READY,
        "Early loading-style planning must not consume a graphics context or bind an execution owner");
    check(host.render_config(host.user, &config) == 1 && !config.enabled && !config.output_width,
        "Refused cold planning publishes no invented size");
    check(host.process_upscale(host.user, nullptr, nullptr) == RSF_ERROR_INVALID_ARGUMENT, "Null graphics lease refused");
    check(rsf_d3d11_host_request_capture(nullptr) == RSF_OK, "Any-thread capture request does no graphics work");
    early.role = RSF_GAME_RENDER_FINAL_SCENE; early.session_id = 91; early.scene_surface = reinterpret_cast<void*>(uintptr_t(1));
    host.render_pass(host.user, reinterpret_cast<void*>(uintptr_t(1)), &early, 1);
    host.render_pass(host.user, reinterpret_cast<void*>(uintptr_t(1)), &early, 0);
    check(host.render_config(host.user, &config) == 1 && !config.enabled,
        "Cold native pass facts do not touch graphics or activate generation");
    auto* fake = reinterpret_cast<void*>(uintptr_t(1));
    rsf_d3d11_chain_dispatch call{}; call.struct_size = sizeof(call); call.abi_version = RSF_D3D11_HOST_ABI_VERSION;
    call.receiver = fake; call.method = RSF_D3D11_DISPATCH_RELEASE; call.handled = 1; call.result.references = 123;
    check(rsf_d3d11_host_dispatch_chain(&cookie,&call) == RSF_ERROR_INVALID_ARGUMENT && call.handled == 1,
        "Typed chain dispatch checks host before touching outputs");
    --call.struct_size;
    check(rsf_d3d11_host_dispatch_chain(host.user,&call) == RSF_ERROR_INVALID_ARGUMENT, "Short typed dispatch refused");
    ++call.struct_size; ++call.abi_version;
    check(rsf_d3d11_host_dispatch_chain(host.user,&call) == RSF_ERROR_ABI_MISMATCH, "Typed dispatch ABI checked");
    --call.abi_version; call.method = 0;
    check(rsf_d3d11_host_dispatch_chain(host.user,&call) == RSF_ERROR_INVALID_ARGUMENT, "Unknown method cannot forward");
    for (uint32_t method = RSF_D3D11_DISPATCH_RELEASE; method <= RSF_D3D11_DISPATCH_RESIZE_BUFFERS1; ++method) {
        call.method = method; call.handled = 1;
        call.args.resize_buffers1.node_masks = static_cast<const uint32_t*>(fake);
        call.args.resize_buffers1.present_queues = static_cast<void* const*>(fake);
        check(rsf_d3d11_host_dispatch_chain(host.user,&call) == RSF_OK && !call.handled && call.result.references == 123,
            "Every unowned typed method preserves result without argument or receiver dereference");
    }
    rsf_d3d11_present_dispatch dispatch{sizeof(dispatch),RSF_D3D11_HOST_ABI_VERSION,fake,2,0x80,1,123};
    check(rsf_d3d11_host_dispatch_present(&cookie,&dispatch) == RSF_ERROR_INVALID_ARGUMENT &&
        dispatch.handled == 1 && dispatch.native_result == 123, "Present dispatch validates owner before outputs");
    --dispatch.struct_size;
    check(rsf_d3d11_host_dispatch_present(host.user,&dispatch) == RSF_ERROR_INVALID_ARGUMENT,
        "Short Present dispatch refused before receiver dereference");
    ++dispatch.struct_size; ++dispatch.abi_version;
    check(rsf_d3d11_host_dispatch_present(host.user,&dispatch) == RSF_ERROR_ABI_MISMATCH,
        "Present dispatch ABI checked");
    --dispatch.abi_version;
    check(rsf_d3d11_host_dispatch_present(host.user,&dispatch) == RSF_OK && !dispatch.handled &&
        dispatch.native_result == 123 && dispatch.receiver == fake && dispatch.sync_interval == 2 && dispatch.flags == 0x80,
        "Unowned Present receiver is not dereferenced and preserves native result and arguments");
    rsf_d3d11_creation_scope scope{sizeof(scope), RSF_D3D11_HOST_ABI_VERSION, fake, 0};
    check(rsf_d3d11_host_creation_begin(&cookie,&scope) == RSF_ERROR_INVALID_ARGUMENT, "Creation scope owner checked");
    --scope.struct_size;
    check(rsf_d3d11_host_creation_begin(host.user,&scope) == RSF_ERROR_INVALID_ARGUMENT, "Short creation scope refused");
    ++scope.struct_size; ++scope.abi_version;
    check(rsf_d3d11_host_creation_begin(host.user,&scope) == RSF_ERROR_ABI_MISMATCH, "Creation scope ABI checked");
    --scope.abi_version;
    check(rsf_d3d11_host_creation_begin(host.user,&scope) == RSF_ERROR_NOT_READY && !scope.token,
        "Unscoped host preserves scope output without dereferencing native pointers");
    check(rsf_d3d11_host_creation_end(host.user,&scope,nullptr) == RSF_ERROR_NOT_READY,
        "Unscoped host cannot close another creation transaction");
    void* original_chain = fake;
    rsf_d3d11_native_chain native_creation_request{sizeof(native_creation_request), RSF_D3D11_HOST_ABI_VERSION, fake, fake, &original_chain};
    check(rsf_d3d11_host_wrap_native_swapchain(&cookie, &native_creation_request) == RSF_ERROR_INVALID_ARGUMENT,
        "Only returned host owner wraps outer native creation");
    --native_creation_request.struct_size;
    check(rsf_d3d11_host_wrap_native_swapchain(host.user, &native_creation_request) == RSF_ERROR_INVALID_ARGUMENT,
        "Short outer creation call rejected before graphics access");
    ++native_creation_request.struct_size; ++native_creation_request.abi_version;
    check(rsf_d3d11_host_wrap_native_swapchain(host.user, &native_creation_request) == RSF_ERROR_ABI_MISMATCH,
        "Outer creation call ABI checked");
    --native_creation_request.abi_version;
    check(rsf_d3d11_host_wrap_native_swapchain(host.user, &native_creation_request) == RSF_ERROR_NOT_READY && original_chain == fake,
        "Host without outer-root bridge preserves native outputs before graphics access");
    rsf_d3d11_presentation_binding binding{sizeof(binding), RSF_D3D11_HOST_ABI_VERSION, fake};
    check(rsf_d3d11_host_bind_presentation(&cookie, &binding) == RSF_ERROR_INVALID_ARGUMENT, "Presentation owner checked");
    --binding.struct_size;
    check(rsf_d3d11_host_bind_presentation(host.user, &binding) == RSF_ERROR_INVALID_ARGUMENT, "Presentation binding size checked");
    ++binding.struct_size; ++binding.abi_version;
    check(rsf_d3d11_host_bind_presentation(host.user, &binding) == RSF_ERROR_ABI_MISMATCH, "Presentation binding ABI checked");
    --binding.abi_version;
    check(rsf_d3d11_host_set_active(host.user, 0) == RSF_OK &&
        rsf_d3d11_host_bind_presentation(host.user, &binding) == RSF_ERROR_NOT_READY,
        "Suspended binding never dereferences graphics pointers");
    check(rsf_d3d11_host_set_active(host.user, 1) == RSF_OK, "Restore active host");
    rsf_d3d11_host_capture capture{sizeof(capture), RSF_D3D11_HOST_ABI_VERSION, 91, 1, 2, "aux", "Auxiliary", fake, 0, 1};
    check(rsf_d3d11_host_capture_texture(&cookie, fake, &capture) == RSF_ERROR_INVALID_ARGUMENT, "Capture owner checked");
    --capture.struct_size;
    check(rsf_d3d11_host_capture_texture(host.user, fake, &capture) == RSF_ERROR_INVALID_ARGUMENT, "Capture size checked");
    ++capture.struct_size; ++capture.abi_version;
    check(rsf_d3d11_host_capture_texture(host.user, fake, &capture) == RSF_ERROR_ABI_MISMATCH, "Capture ABI checked");
    --capture.abi_version;
    check(rsf_d3d11_host_capture_texture(host.user, fake, &capture) == RSF_ERROR_NOT_READY,
        "An armed request without a matching scene capture must not dereference graphics pointers");
    return errors ? 1 : 0;
}
