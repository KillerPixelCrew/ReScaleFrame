#include <rescaleframe/native_cpu.h>
// SPDX-License-Identifier: MIT
#include "api.h"
#include "godot_platform.h"
#include "vulkan_control.h"
#include <rescaleframe/native_sr_d3d12.h>
#include <rescaleframe/native_fg_d3d12.h>
#include <rescaleframe/native_latency_vulkan.h>
#include <rescaleframe/native_streamline_vulkan.h>
#include <rescaleframe/gpu_policy.h>
#include <rescaleframe/fg_choice.h>
#include <windows.h>
#include <d3d12.h>
#include <dxgi1_4.h>
#include <wrl/client.h>
#include <cstddef>
#include <mutex>
#include <string>
#include <cstdio>
#include <atomic>
#include "../../../runtime/backends/common/frame_capture12.h"

// Settings, their generation, the D3D12 session identity and paths, the status record and the
// engine-service routing are owned by the Rust control owner (native/control.rs), the D3D12 SR
// context plan, its retirement and recreation, the fence and slot ledger, the pending
// submissions and the Present pass identity by native/d3d12.rs, the Vulkan session by
// native/vulkan.rs. This file keeps the rsfg_* exports as forwarders and the leaves: the device,
// queue and fence references, the Runtime SR, FG and presentation calls, capture, DXGI, Win32
// window calls and the linked Runtime. The Vulkan leaves in the table below live in vulkan.cpp.
static_assert(sizeof(rsf_sharpening_options) == 56);
static_assert(sizeof(rsf_godot_settings) == 100 && offsetof(rsf_godot_settings, sharpening) == 44);
static_assert(sizeof(rsf_godot_status) == 256 && offsetof(rsf_godot_status, sr_result) == 40 &&
    offsetof(rsf_godot_status, evaluated_frames) == 88 && offsetof(rsf_godot_status, presentation_statistics_valid) == 144 &&
    offsetof(rsf_godot_status, window_foreground) == 176 && offsetof(rsf_godot_status, keyboard_inputs) == 192 &&
    offsetof(rsf_godot_status, latency_owner) == 224 && offsetof(rsf_godot_status, derived_boundaries) == 248);
static_assert(sizeof(rsf_godot_setup) == 48 && sizeof(rsf_godot_engine_services) == 24);
static_assert(sizeof(rsf_godot_fg_options) == 20 && sizeof(rsf_godot_fg_install) == 96 &&
    offsetof(rsf_godot_fg_install, fidelityfx_directory) == 56);
static_assert(sizeof(rsf_godot_paths) == 56 && sizeof(rsf_godot_input_facts) == 56 && sizeof(rsf_godot_window_facts) == 16);
static_assert(sizeof(rsf_godot_fg_facts) == 96 && offsetof(rsf_godot_fg_facts, tagged_frames) == 48);
static_assert(sizeof(rsf_godot_latency_facts) == 88);
static_assert(sizeof(rsf_godot_sr_create) == 88 && offsetof(rsf_godot_sr_create, paths) == 32 &&
    sizeof(rsf_godot_sr_context) == 24 && offsetof(rsf_godot_sr_context, desired_width) == 12 &&
    sizeof(rsf_godot_sr_evaluate) == 64);
static_assert(sizeof(rsf_godot_option_field) == 16 && sizeof(rsf_godot_option_value) == 24);
static_assert(sizeof(rsf_godot_platform) == 440 && offsetof(rsf_godot_platform, vulkan_locked) == 88 &&
    offsetof(rsf_godot_platform, latency_facts) == 176 && offsetof(rsf_godot_platform, d3d12_bind) == 184 &&
    offsetof(rsf_godot_platform, sr_evaluate) == 256 && offsetof(rsf_godot_platform, d3d12_release) == 320 &&
    offsetof(rsf_godot_platform, vulkan_presenter_drain) == 328 && offsetof(rsf_godot_platform, vulkan_sr_create) == 360 &&
    offsetof(rsf_godot_platform, vulkan_sr_evaluate) == 400 && offsetof(rsf_godot_platform, vulkan_sr_dump_result) == 432);
static_assert(RSF_SR_AUTO == 7 && RSF_SR_SPATIAL == 6 && RSF_SR_DLSS == 1 && RSF_FG_BACKEND_AUTO == 7 &&
    RSF_FG_OFF == 0 && RSF_FG_FIXED == 1 && RSF_REFLEX_OFF == 0 && RSF_UI_MODE_BACKBUFFER_HUDLESS == 4 &&
    RSF_DLSS_ENGINE_CUSTOM == 0 && RSF_FG_STAT_TOTAL_PRESENTED == 1 && RSF_BACKEND_OK == 0,
    "native/control.rs mirrors these Runtime constants");
static_assert(RSF_SR_NONE == 0 && RSF_BACKEND_ERROR_INVALID_ARGUMENT == -1 && RSF_SCREEN_UNKNOWN == 0 &&
    RSF_SCREEN_MENU == 1 && RSF_SCREEN_FLIGHT == 4 && RSF_GAME_RENDER_SR == 1 && RSF_GAME_RENDER_WINDOW == 9 &&
    RSF_GAME_RENDER_FINAL_SCENE == 10, "native/d3d12.rs mirrors these Runtime constants");

namespace {
using Microsoft::WRL::ComPtr;
ComPtr<ID3D12Device> device;
ComPtr<ID3D12CommandQueue> queue;
ComPtr<ID3D12Fence> fence;
thread_local bool capture_native_primary_present = false;
std::atomic<HWND> input_window{nullptr};
WNDPROC previous_proc = nullptr;
std::atomic<uint64_t> input_session{0};
std::unique_ptr<rsf::FrameCapture12> capture;
LRESULT CALLBACK input_proc(HWND window, UINT message, WPARAM wparam, LPARAM lparam)
{
    LARGE_INTEGER now{},frequency{}; QueryPerformanceCounter(&now); QueryPerformanceFrequency(&frequency);
    rsf_game_cpu_event fact{sizeof(fact),RSF_GAME_CPU_INPUT_EVENT,input_session.load(),0,uint64_t(now.QuadPart),
        uint64_t(frequency.QuadPart),0,0,message,RSF_GAME_CPU_EVENT_VERSION,RSF_GAME_CPU_INPUT_KEYED | RSF_GAME_CPU_WINDOW_MESSAGE};
    if (rsf::godot_vk::booted()) rsf::godot_vk::cpu(&fact); else rsf_fg12_cpu(&fact);
    return previous_proc ? CallWindowProcW(previous_proc, window, message, wparam, lparam) : DefWindowProcW(window, message, wparam, lparam);
}

void log(void*, const char* message)
{
    rsf_godot_log(message);
}
bool wait(uint64_t value)
{
    if (!value || !fence || fence->GetCompletedValue() >= value) return true;
    HANDLE event = CreateEventW(nullptr, FALSE, FALSE, nullptr);
    if (!event) return false;
    const bool okay = SUCCEEDED(fence->SetEventOnCompletion(value, event)) && WaitForSingleObject(event, 15000) == WAIT_OBJECT_0;
    CloseHandle(event);
    return okay;
}
int accept(void*, void* window)
{
    DWORD process = 0;
    GetWindowThreadProcessId(static_cast<HWND>(window), &process);
    wchar_t name[64]{};
    if (process != GetCurrentProcessId() || !GetClassNameW(static_cast<HWND>(window), name, 64) || std::wcscmp(name, L"Engine") != 0) return 0;
    HWND expected = nullptr;
    input_window.compare_exchange_strong(expected, static_cast<HWND>(window));
    return input_window.load() == window;
}
uint32_t vendor_id()
{
    ComPtr<IDXGIFactory4> factory;
    ComPtr<IDXGIAdapter1> adapter;
    DXGI_ADAPTER_DESC1 description{};
    return device && SUCCEEDED(CreateDXGIFactory1(IID_PPV_ARGS(&factory))) &&
        SUCCEEDED(factory->EnumAdapterByLuid(device->GetAdapterLuid(), IID_PPV_ARGS(&adapter))) &&
        SUCCEEDED(adapter->GetDesc1(&description)) ? description.VendorId : 0;
}

// Leaves lent to the Rust owner. None of them decides; each catches its own exceptions.
uint64_t leaf_tick_count() { return GetTickCount64(); }
uint32_t leaf_prepare_capture() try {
    capture = std::make_unique<rsf::FrameCapture12>();
    return 1;
} catch (...) { return 0; }
int32_t leaf_fg_install(const rsf_godot_fg_install* install) try {
    rsf_fg12_setup fg{};
    fg.struct_size = sizeof(fg); fg.backend = install->backend;
    fg.max_generated_frames = install->max_generated_frames; fg.runtime_switching = install->runtime_switching;
    fg.ui_mode = install->ui_mode;
    fg.options = {sizeof(fg.options), install->options.mode, install->options.generated_frames,
        install->options.reflex_mode, install->options.frame_limit_us, install->options.debug};
    fg.depth_inverted = install->depth_inverted; fg.depth_infinite = install->depth_infinite;
    fg.units_to_meters = install->units_to_meters; fg.accept_window = accept; fg.log = log;
    fg.runtime_directory_utf8 = fg.fsr3_directory_utf8 = fg.fsr4_directory_utf8 = install->fidelityfx_directory;
    fg.xess_directory_utf8 = install->xess_directory; fg.streamline_directory_utf8 = install->streamline_directory;
    fg.engine_type = install->engine_type; fg.engine_version_utf8 = install->engine_version;
    fg.project_id_utf8 = install->project_id;
    return rsf_fg12_install_explicit(&fg);
} catch (...) { return RSF_BACKEND_ERROR_FEATURE_FAILED; }
void leaf_fg_options(const rsf_godot_fg_options* value) try {
    const rsf_native_fg_options options{sizeof(options), value->mode, value->generated_frames, value->reflex_mode,
        value->frame_limit_us, value->debug};
    rsf_fg12_options(&options);
} catch (...) {}
uint32_t leaf_dlss_preset_valid(uint32_t preset) { return rsf_dlss_preset_valid(preset) ? 1u : 0u; }
uint32_t leaf_sharpening_valid(const rsf_sharpening_options* options) { return rsf::sharpening_settings::valid(*options) ? 1u : 0u; }
void leaf_sharpening_defaults(rsf_sharpening_options* options) { *options = rsf_sharpening_defaults(); }
uint32_t leaf_fg_backend_known(uint32_t backend) { return rsf_fg_backend_known(backend) ? 1u : 0u; }
void leaf_cpu_counters(rsf_godot_input_facts* facts)
{
    rsf_native_cpu_counters inputs{}; inputs.struct_size = sizeof(inputs); rsf_native_cpu_counters_get(&inputs);
    facts->keyboard = inputs.input_events[0]; facts->mouse = inputs.input_events[1];
    facts->controller = inputs.input_events[2]; facts->pings = inputs.pings;
    facts->substitutes = inputs.substitutes; facts->order_violations = inputs.order_violations;
    facts->derived_boundaries = inputs.derived_frame_ends;
}
void leaf_window_facts(rsf_godot_window_facts* facts)
{
    DWORD foreground = 0;
    GetWindowThreadProcessId(GetForegroundWindow(), &foreground);
    facts->foreground_process_id = foreground;
    facts->current_process_id = GetCurrentProcessId();
    facts->input_window_visible = IsWindowVisible(input_window.load()) ? 1u : 0u;
}
void leaf_fg_facts(rsf_godot_fg_facts* facts)
{
    rsf_native_fg_status fg{}; fg.struct_size = sizeof(fg); rsf_fg12_status(&fg);
    facts->backend = rsf_d3d11_present_backend(); facts->available = fg.available; facts->active = fg.vendor.active;
    facts->switch_result = rsf_d3d11_present_switch_result(); facts->last_result = fg.last_result; facts->reason = fg.reason;
    facts->max_generated_frames = fg.vendor.max_generated_frames; facts->generated_frames = fg.vendor.effective_generated_frames;
    facts->latency_available = fg.vendor.low_latency_available; facts->latency_mode = fg.vendor.effective_reflex;
    facts->tagged_frames = fg.tagged_frames;
    facts->total_presented = fg.vendor.total_presented; facts->generated_presented = fg.vendor.generated_presented;
    facts->statistics_flags = fg.vendor.valid_statistics;
    facts->source_frame_id = fg.source_frame_id;
    facts->provider_generation = rsf_d3d11_present_generation();
    facts->generated_callbacks = fg.vendor.generated_callbacks;
}
uint32_t leaf_latency_facts(rsf_godot_latency_facts* facts)
{
    rsf_native_latency_status latency{sizeof(latency),1};
    if (!rsf_native_latency_status_get(&latency)) return 0;
    facts->inputs.keyboard = latency.events.input_events[0]; facts->inputs.mouse = latency.events.input_events[1];
    facts->inputs.controller = latency.events.input_events[2]; facts->inputs.pings = latency.events.pings;
    facts->inputs.substitutes = latency.events.substitutes; facts->inputs.order_violations = latency.events.order_violations;
    facts->inputs.derived_boundaries = latency.derived_boundaries;
    facts->paced_frames = latency.paced_frames; facts->marker_calls = latency.marker_calls;
    facts->pcl_available = latency.pcl_available; facts->report_available = latency.report_available;
    facts->owner = latency.owner;
    return 1;
}
// D3D12 SR context and submission leaves for native/d3d12.rs.
uint32_t leaf_d3d12_bind(void* owning_device, void* owning_queue)
{
    device = static_cast<ID3D12Device*>(owning_device);
    queue = static_cast<ID3D12CommandQueue*>(owning_queue);
    return SUCCEEDED(device->CreateFence(0, D3D12_FENCE_FLAG_NONE, IID_PPV_ARGS(&fence))) ? 1u : 0u;
}
uint32_t leaf_d3d12_wait(uint64_t value) { return wait(value) ? 1u : 0u; }
uint32_t leaf_d3d12_signal(uint64_t value)
{
    return queue && fence && SUCCEEDED(queue->Signal(fence.Get(), value)) ? 1u : 0u;
}
uint32_t leaf_sr_default_backend() { return rsf_gpu_default_sr_backend(vendor_id(), false); }
uint32_t leaf_fg_default_backend() { return rsf_fg_default_backend(vendor_id(), false); }
uint32_t leaf_present_owner(uint32_t* backend)
{
    if (!rsf_d3d11_present_has_owner()) return 0;
    *backend = rsf_d3d11_present_backend();
    return 1;
}
void leaf_present_request(uint32_t backend) { (void)rsf_d3d11_present_request(backend); }
uint32_t leaf_sr_create(const rsf_godot_sr_create* request, rsf_godot_sr_context* created) try {
    const auto& paths = request->paths;
    rsf_sr12_setup setup{};
    setup.struct_size = sizeof(setup); setup.abi_version = 1; setup.device = device.Get();
    setup.backend = request->backend; setup.quality = request->quality;
    setup.output_width = request->output_width; setup.output_height = request->output_height;
    setup.render_width = request->render_width; setup.render_height = request->render_height;
    setup.inverted_depth = 1; setup.streamline_host = rsf_d3d11_present_host();
    setup.fsr2_directory_utf8 = setup.fsr3_directory_utf8 = setup.fsr4_directory_utf8 = paths.fidelityfx_directory;
    setup.xess_directory_utf8 = paths.xess_directory; setup.log = log;
    setup.dlss = {sizeof(setup.dlss), RSF_DLSS_ABI_VERSION, paths.streamline_interposer, paths.streamline_directory,
        paths.runtime_directory, 0, RSF_DLSS_ENGINE_CUSTOM, paths.engine_version, paths.project_id, 1, log, nullptr,
        request->dlss_preset};
    rsf_sr12* context = nullptr;
    created->result = rsf_sr12_create(&setup, &context);
    created->handle = context;
    if (context) rsf_sr12_plan(context, &created->desired_width, &created->desired_height);
    return 1;
} catch (...) { return 0; }
void leaf_sr_destroy(void* context) { rsf_sr12_destroy(static_cast<rsf_sr12*>(context)); }
uint32_t leaf_sr_evaluate(void* context, void* commands, const rsf_game_render_pass* pass,
    const rsf_godot_sr_evaluate* request, int32_t* result) try {
    auto* sr = static_cast<rsf_sr12*>(context);
    rsf_game_render_pass frame = *pass;
    frame.screen = request->screen;
    (void)rsf_sr12_set_sharpening(sr, &request->sharpening);
    *result = rsf_sr12_evaluate(sr, commands, &frame, request->slot);
    return 1;
} catch (...) { return 0; }
void leaf_sr_refusal_log(const rsf_game_render_pass* pass, uint32_t desired_width, uint32_t desired_height) try {
    const auto& camera = pass->camera;
    char text[256];
    std::snprintf(text, sizeof(text), "Godot SR input refusal: actual=%ux%u output=%ux%u plan=%ux%u device=%p frame=%llu history=%llu",
        camera.render_width, camera.render_height, camera.output_width, camera.output_height,
        desired_width, desired_height, device.Get(), static_cast<unsigned long long>(pass->native_frame),
        static_cast<unsigned long long>(pass->history_key));
    log(nullptr, text);
    void* resources[] = {pass->color_input, pass->depth, pass->motion, pass->color_output};
    for (size_t i = 0; i < 4; ++i) if (resources[i]) {
        auto* resource = static_cast<ID3D12Resource*>(resources[i]);
        const auto description = resource->GetDesc();
        ComPtr<ID3D12Device> owner;
        (void)resource->GetDevice(IID_PPV_ARGS(&owner));
        std::snprintf(text, sizeof(text), "Godot SR resource %zu: %llux%u format=%u samples=%u layers=%u device=%p",
            i, description.Width, description.Height, description.Format, description.SampleDesc.Count,
            description.DepthOrArraySize, owner.Get());
        log(nullptr, text);
    }
} catch (...) {}
uint32_t leaf_capture_sr_begin(const rsf_game_render_pass* pass) try {
    if (capture && capture->wants(pass->source_frame_id)) {
        rsf_frame_record identity{};
        identity.session_id = pass->session_id; identity.frame_id = pass->source_frame_id;
        (void)capture->event(identity, "native-sr-begin", pass->source_frame_id, false, 0);
    }
    return 1;
} catch (...) { return 0; }
uint32_t leaf_final_scene(void* commands, const rsf_game_render_pass* pass, uint32_t slot) try {
    if (capture) {
        (void)capture->flush(slot);
        rsf_frame_record record{};
        record.struct_size = sizeof(record); record.abi_version = RSF_GAME_FRAME_ABI_VERSION;
        record.frame_id = pass->source_frame_id; record.session_id = pass->session_id;
        record.view_id = static_cast<uint32_t>(pass->view_key); record.camera = pass->camera;
        record.resource_generation = pass->resource_generation;
        record.flags = (pass->flags & RSF_GAME_RENDER_RESET) ? RSF_FRAME_FLAG_RESET : 0;
        record.render_width = pass->camera.render_width; record.render_height = pass->camera.render_height;
        record.frame_time_ms = pass->camera.frame_time_seconds * 1000.0f;
        record.output_width = pass->camera.output_width; record.output_height = pass->camera.output_height;
        (void)capture->record(static_cast<ID3D12GraphicsCommandList*>(commands), static_cast<ID3D12Resource*>(pass->color_input),
            D3D12_RESOURCE_STATE_RENDER_TARGET, record, "fg-hudless-color", slot);
    }
    rsf_fg12_hudless(commands, pass);
    return 1;
} catch (...) { return 0; }
void leaf_fg_submitted(uint64_t session, uint64_t frame) { rsf_fg12_submitted(session, frame); }
uint32_t leaf_render_begin(uint64_t session, uint64_t frame) try {
    const auto result = rsf_fg12_render_begin(session, frame);
    if (capture && capture->wants(frame)) {
        rsf_frame_record identity{};
        identity.session_id = session; identity.frame_id = frame;
        (void)capture->event(identity, "render-submit-begin", frame, false, result);
    }
    return 1;
} catch (...) { return 0; }
void leaf_present_window(const rsf_game_render_pass* pass) { rsf_fg12_window(pass); }
uint32_t leaf_fg_end_session(uint64_t session) { return rsf_fg12_end_session(session) ? 1u : 0u; }
uint32_t leaf_d3d12_release(void* context, uint64_t session) try {
    if (capture) (void)capture->flush();
    rsf_sr12_destroy(static_cast<rsf_sr12*>(context));
    rsf_input_dump_cancel(session);
    return 1;
} catch (...) { return 0; }

namespace vk = rsf::godot_vk;
const rsf_godot_platform platform{sizeof(platform), RSF_GODOT_PLATFORM_VERSION, leaf_tick_count, leaf_prepare_capture,
    leaf_fg_install, leaf_fg_options, leaf_dlss_preset_valid, leaf_sharpening_valid, leaf_sharpening_defaults,
    leaf_fg_backend_known, vk::leaf_supported, vk::leaf_latency_facts, vk::leaf_locked, vk::leaf_start, vk::leaf_stop,
    vk::leaf_release, vk::leaf_facts, vk::leaf_presenter_max, vk::leaf_fsr4_fg_probe, vk::leaf_latency_options,
    leaf_cpu_counters, leaf_window_facts, leaf_fg_facts, leaf_latency_facts,
    leaf_d3d12_bind, leaf_d3d12_wait, leaf_d3d12_signal, leaf_sr_default_backend, leaf_fg_default_backend,
    leaf_present_owner, leaf_present_request, leaf_sr_create, leaf_sr_destroy, leaf_sr_evaluate, leaf_sr_refusal_log,
    leaf_capture_sr_begin, leaf_final_scene, leaf_fg_submitted, leaf_render_begin, leaf_present_window,
    leaf_fg_end_session, leaf_d3d12_release, vk::leaf_presenter_drain, vk::leaf_device_idle, vk::leaf_sr_destroy,
    vk::leaf_fsr4_destroy, vk::leaf_sr_create, vk::leaf_fsr4_create, vk::leaf_dlss_preset, vk::leaf_sr_plan,
    vk::leaf_fsr4_plan, vk::leaf_sr_evaluate, vk::leaf_fsr4_evaluate, vk::leaf_sr_fallback, vk::leaf_sr_log,
    vk::leaf_sr_dump_result};
bool booted() { return rsf_godot_booted() != 0; }
}

extern "C" const rsf_godot_platform* rsf_godot_platform_leaves(void) { return &platform; }

extern "C" int rsfg_attach_engine(const rsf_godot_engine_services* services)
{
    return rsf_godot_attach_engine(services);
}
extern "C" int rsfg_configure_engine(const rsf_godot_settings* input, uint64_t viewport_id)
{
    return rsf_godot_configure_engine(&platform, input, viewport_id);
}
extern "C" void rsfg_reset_history()
{
    rsf_godot_reset_history(&platform);
}

extern "C" int rsfg_boot(const rsf_godot_setup* setup) try
{
    return rsf_godot_boot(&platform, setup);
} catch (...) { return 0; }

extern "C" int32_t rsfg_create_swapchain(void* factory, void* queue12, void* window, const void* description, void** out)
{
    return booted() ? rsf_d3d11_present_create_for_hwnd(factory, queue12, window, description, out) : DXGI_ERROR_UNSUPPORTED;
}

extern "C" int rsfg_configure(const rsf_godot_settings* input) try
{
    return rsf_godot_configure(&platform, input);
} catch (...) { return 0; }

extern "C" void rsfg_cpu(const rsf_game_cpu_event* event)
{
    const bool vulkan = rsf::godot_vk::booted();
    if ((!booted() && !vulkan) || !event || event->struct_size < sizeof(*event)) return;
    input_session.store(event->session_id);
    if (event->stage==RSF_GAME_CPU_FRAME_BEGIN) {
        const HWND window=input_window.load();
        if (window && !previous_proc) previous_proc=reinterpret_cast<WNDPROC>(SetWindowLongPtrW(window,GWLP_WNDPROC,reinterpret_cast<LONG_PTR>(input_proc)));
    }
    if (vulkan) rsf::godot_vk::cpu(event); else rsf_fg12_cpu(event);
}

extern "C" int rsfg_render(void* owning_device, void* owning_queue, void* commands, const rsf_game_render_pass* pass) try
{
    if (!booted() || !owning_device || !owning_queue || !commands || !pass || pass->struct_size < sizeof(*pass)) return 0;
    if (rsf_game_depth_domains_requested(&pass->depth_domains)) return 0;
    return rsf_godot_d3d12_render(&platform, owning_device, owning_queue, commands, pass);
} catch (...) { return 0; }

extern "C" void rsfg_render_begin(uint64_t frame) try
{
    if (rsf::godot_vk::booted()) { rsf::godot_vk::render_begin(frame); return; }
    if (!booted()) return;
    rsf_godot_d3d12_render_begin(&platform, frame);
} catch (...) {}

extern "C" void rsfg_submitted(void* owning_queue, void* const* command_lists, uint32_t count)
{
    rsf_godot_d3d12_submitted(&platform, owning_queue, command_lists, count);
}

extern "C" void rsfg_present(void* chain)
{
    if (!booted() || !chain) return;
    rsf_godot_d3d12_present(&platform, chain);
}

extern "C" int rsfg_status(rsf_godot_status* output) try
{
    return rsf_godot_status_get(&platform, output);
} catch (...) { return 0; }

extern "C" int rsfg_settings(rsf_godot_settings* output) try
{
    return rsf_godot_settings_get(&platform, output);
} catch (...) { return 0; }
extern "C" int rsfg_request_latency_ping(void)
{
    return rsf::godot_vk::booted() ? rsf_latencyvk_ping(input_window.load()) : rsf_native_latency_ping(input_window.load());
}
extern "C" void rsfg_vk_window(void* window) {
    if (rsf::godot_vk::booted()) { accept(nullptr, window); rsf::godot_vk::window(window); }
}
extern "C" int rsfg_vk_present_marker(uint64_t window, uint32_t begin) {
    if (!rsf::godot_vk::booted() || !window || reinterpret_cast<uint64_t>(input_window.load()) != window) return 0;
    rsf_latencyvk_present(begin);
    capture_native_primary_present = begin != 0;
    return 1;
}
extern "C" int32_t rsfg_vk_native_present(uint64_t callback, void* owning_queue, const void* info) {
    struct Lease { uint64_t callback; void* queue; const void* info; } lease{callback, owning_queue, info};
    if (capture_native_primary_present) rsf::godot_vk::capture_present(info, 0, 1);
    const auto result = rsf_slvk_without_dlssg([](void* user) -> int32_t {
        const auto& value = *static_cast<Lease*>(user);
        return rsf_latencyvk_queue_present(reinterpret_cast<rsf_vk_function>(value.callback), value.queue, value.info);
    }, &lease);
    if (capture_native_primary_present) rsf::godot_vk::capture_present(info, result, 0);
    return result;
}
extern "C" int32_t rsfg_vk_wsi_call(rsf_vk_wsi_call callback, void* user) {
    struct Lease { rsf_vk_wsi_call callback; void* user; } lease{callback, user};
    return rsf_slvk_without_dlssg([](void* opaque) -> int32_t {
        const auto& value = *static_cast<Lease*>(opaque); return rsf_latencyvk_wsi_call(value.callback, value.user);
    }, &lease);
}
extern "C" int32_t rsfg_vk_foreign_call(rsf_vk_wsi_call callback, void* user) { return rsf_slvk_without_dlssg(callback, user); }

extern "C" int rsfg_shutdown(void) try
{
    const HWND window = input_window.load();
    if (window && previous_proc && reinterpret_cast<WNDPROC>(GetWindowLongPtrW(window, GWLP_WNDPROC)) == input_proc) {
        SetWindowLongPtrW(window, GWLP_WNDPROC, reinterpret_cast<LONG_PTR>(previous_proc));
        previous_proc = nullptr;
    }
    input_window.store(nullptr); input_session.store(0);
    if (rsf::godot_vk::booted()) return rsf_godot_shutdown_vulkan(&platform);
    return rsf_godot_d3d12_shutdown(&platform);
} catch (...) { return 0; }
