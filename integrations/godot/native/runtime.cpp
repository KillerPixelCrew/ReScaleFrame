// SPDX-License-Identifier: GPL-3.0-only
#include "api.h"
#include "vulkan_control.h"
#include <rescaleframe/native_sr_d3d12.h>
#include <rescaleframe/native_fg_d3d12.h>
#include <rescaleframe/native_latency_vulkan.h>
#include <rescaleframe/gpu_policy.h>
#include <rescaleframe/fg_choice.h>
#include <windows.h>
#include <d3d12.h>
#include <dxgi1_4.h>
#include <wrl/client.h>
#include <array>
#include <mutex>
#include <string>
#include <vector>
#include <algorithm>
#include <cstdio>
#include <atomic>
#include "../../../runtime/backends/common/frame_capture12.h"

namespace {
using Microsoft::WRL::ComPtr;
std::mutex guard;
rsf_godot_settings settings{sizeof(settings), 1, RSF_SR_AUTO, 1, 0, RSF_FG_BACKEND_AUTO, RSF_FG_OFF, 1, RSF_REFLEX_OFF, 0, 1};
rsf_godot_status status{sizeof(status), 1};
uint64_t settings_generation = 1, context_generation = 0;
std::string root, streamline, fsr, xess, version, project;
rsf_game_log_fn logger = nullptr;
void* logger_user = nullptr;
bool booted = false;
rsf_sr12* context = nullptr;
ComPtr<ID3D12Device> device;
ComPtr<ID3D12CommandQueue> queue;
ComPtr<ID3D12Fence> fence;
uint64_t submitted = 0;
uint64_t current_render_frame = 0;
std::array<uint64_t, 3> slots{};
uint32_t next_slot = 0, context_backend = 0, desired_width = 0, desired_height = 0;
uint32_t output_width = 0, output_height = 0;
rsf_backend_result context_result = RSF_BACKEND_OK;
rsf_game_render_pass rendered{};
struct Pending {
    void* commands;
    uint32_t slot;
    uint64_t session, frame;
    bool hudless;
};
std::vector<Pending> pending;
rsf_godot_engine_services engine_services{};
std::atomic<HWND> input_window{nullptr};
WNDPROC previous_proc = nullptr;
std::atomic<uint32_t> ping_message{0};
std::atomic<bool> ping_pending{false};
std::array<std::atomic<uint64_t>, 4> input_counts{};
std::unique_ptr<rsf::FrameCapture12> capture;
LRESULT CALLBACK input_proc(HWND window, UINT message, WPARAM wparam, LPARAM lparam)
{
    if (message >= 0xc000u && message == ping_message.load()) ping_pending.store(true);
    return previous_proc ? CallWindowProcW(previous_proc, window, message, wparam, lparam) : DefWindowProcW(window, message, wparam, lparam);
}

void log(void*, const char* message)
{
    if (logger) logger(logger_user, message);
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
}

extern "C" int rsfg_attach_engine(const rsf_godot_engine_services* services)
{
    if (!services || services->struct_size < sizeof(*services) || services->abi_version != RSF_GODOT_ABI_VERSION ||
        !services->configure || !services->reset_history) return 0;
    engine_services = *services;
    return 1;
}
extern "C" int rsfg_configure_engine(const rsf_godot_settings* input, uint64_t viewport_id)
{
    return (booted || rsf::godot_vk::booted()) && engine_services.configure ? engine_services.configure(input, viewport_id) : 0;
}
extern "C" void rsfg_reset_history()
{
    if ((booted || rsf::godot_vk::booted()) && engine_services.reset_history) engine_services.reset_history();
}

extern "C" int rsfg_boot(const rsf_godot_setup* setup) try
{
    if (!setup || setup->struct_size < sizeof(*setup) || setup->abi_version != RSF_GODOT_ABI_VERSION ||
        !setup->runtime_directory || !setup->engine_version || !setup->project_id) return 0;
    if (booted) return 1;
    capture = std::make_unique<rsf::FrameCapture12>();
    root = setup->runtime_directory;
    streamline = root + "/streamline"; fsr = root + "/fidelityfx"; xess = root + "/xess";
    version = setup->engine_version; project = setup->project_id;
    logger = setup->log; logger_user = setup->log_user;
    status.session_id = GetTickCount64() + 1;
    rsf_fg12_setup fg{};
    fg.struct_size = sizeof(fg); fg.backend = RSF_FG_BACKEND_AUTO;
    fg.max_generated_frames = UINT32_MAX; fg.runtime_switching = 1;
    fg.ui_mode = RSF_UI_MODE_BACKBUFFER_HUDLESS;
    fg.options = {sizeof(fg.options), RSF_FG_OFF, 1, RSF_REFLEX_OFF, 0, 0};
    fg.depth_inverted = 1; fg.units_to_meters = 1; fg.accept_window = accept; fg.log = log;
    fg.runtime_directory_utf8 = fg.fsr3_directory_utf8 = fg.fsr4_directory_utf8 = fsr.c_str();
    fg.xess_directory_utf8 = xess.c_str(); fg.streamline_directory_utf8 = streamline.c_str();
    fg.engine_type = RSF_DLSS_ENGINE_CUSTOM; fg.engine_version_utf8 = version.c_str(); fg.project_id_utf8 = project.c_str();
    status.fg_result = rsf_fg12_install_explicit(&fg);
    booted = status.fg_result == RSF_BACKEND_OK;
    return booted ? 1 : 0;
} catch (...) { return 0; }

extern "C" int32_t rsfg_create_swapchain(void* factory, void* queue12, void* window, const void* description, void** out)
{
    return booted ? rsf_d3d11_present_create_for_hwnd(factory, queue12, window, description, out) : DXGI_ERROR_UNSUPPORTED;
}

extern "C" int rsfg_configure(const rsf_godot_settings* input) try
{
    if (!input || input->struct_size < sizeof(*input) || input->abi_version != RSF_GODOT_ABI_VERSION ||
        input->sr_backend > RSF_SR_AUTO || input->quality > 5 || !rsf_dlss_preset_valid(input->dlss_preset) ||
        input->fg_mode > RSF_FG_FIXED || input->latency_mode > 2 ||
        !input->generated_frames || input->generated_frames > 15 || input->gameplay > 1 ||
        !(rsf_fg_backend_known(input->fg_backend) || input->fg_backend == RSF_FG_BACKEND_AUTO) ||
        (input->sr_backend == RSF_SR_DLSS && input->quality == 5)) return 0;
    if (rsf::godot_vk::booted()) return rsf::godot_vk::configure(input);
    {
        std::lock_guard lock(guard);
        if (settings.sr_backend != input->sr_backend || settings.quality != input->quality || settings.dlss_preset != input->dlss_preset)
            ++settings_generation;
        settings = *input;
    }
    const rsf_native_fg_options options{sizeof(options), input->fg_mode, input->generated_frames, input->latency_mode, input->frame_limit_us, 0};
    rsf_fg12_options(&options);
    return 1;
} catch (...) { return 0; }

extern "C" void rsfg_cpu(const rsf_game_cpu_event* event)
{
    const bool vulkan = rsf::godot_vk::booted();
    if ((!booted && !vulkan) || !event || event->struct_size < sizeof(*event)) return;
    if (vulkan) rsf::godot_vk::cpu(event);
    if (event->stage == RSF_GAME_CPU_FRAME_BEGIN) {
        const HWND window = input_window.load();
        if (window && !previous_proc) previous_proc = reinterpret_cast<WNDPROC>(SetWindowLongPtrW(window, GWLP_WNDPROC, reinterpret_cast<LONG_PTR>(input_proc)));
        rsf_streamline_latency_status latency{}; latency.struct_size = sizeof(latency);
        if (vulkan) ping_message.store(rsf::godot_vk::ping_message());
        else if (rsf_streamline_host_latency_status(rsf_d3d11_present_host(), &latency) == RSF_BACKEND_OK)
            ping_message.store(latency.pcl_message_id);
    }
    if (event->stage == RSF_GAME_CPU_INPUT_EVENT) {
        const bool accepted = vulkan ? rsf::godot_vk::input(event->source_frame_id, event->input_kind, event->message_id) != 0 :
            rsf_d3d11_present_input(event->source_frame_id, event->input_kind, event->message_id) == RSF_BACKEND_OK;
        if (!(event->input_kind & ~7u) && accepted) {
            for (uint32_t i = 0; i < 3; ++i) if (event->input_kind & (1u << i)) ++input_counts[i];
        }
        return;
    }
    if (event->stage == RSF_GAME_CPU_INPUT_SAMPLE && ping_pending.exchange(false)) {
        const bool accepted = vulkan ? rsf::godot_vk::input(event->source_frame_id, 0, ping_message.load()) != 0 :
            rsf_d3d11_present_input(event->source_frame_id, 0, ping_message.load()) == RSF_BACKEND_OK;
        if (accepted) ++input_counts[3];
    }
    if (vulkan) return;
    rsf_fg12_cpu(event);
    if (event->stage == RSF_GAME_CPU_FRAME_END && !event->render_expected) {
        if (auto* host = rsf_d3d11_present_host()) rsf_streamline_host_abort(host, event->source_frame_id);
    }
}

extern "C" int rsfg_render(void* owning_device, void* owning_queue, void* commands, const rsf_game_render_pass* pass) try
{
    if (!booted || !owning_device || !owning_queue || !commands || !pass || pass->struct_size < sizeof(*pass)) return 0;
    if (!device) {
        device = static_cast<ID3D12Device*>(owning_device);
        queue = static_cast<ID3D12CommandQueue*>(owning_queue);
        if (FAILED(device->CreateFence(0, D3D12_FENCE_FLAG_NONE, IID_PPV_ARGS(&fence)))) return 0;
    }
    if (device.Get() != owning_device || queue.Get() != owning_queue) return 0;
    rendered = *pass;
    // Present retains copied identity, never the callback's borrowed texture handles.
    rendered.color_input = rendered.color_output = rendered.depth = rendered.motion = rendered.exposure = nullptr;
    rendered.color_output_readable = rendered.ui_input = rendered.swapchain = rendered.scene_surface = rendered.sampled_texture = nullptr;
    if (pass->role == RSF_GAME_RENDER_SR && capture && capture->wants(pass->source_frame_id)) {
        rsf_frame_record identity{};
        identity.session_id = pass->session_id; identity.frame_id = pass->source_frame_id;
        (void)capture->event(identity, "native-sr-begin", pass->source_frame_id, false, 0);
    }
    if (pass->role == RSF_GAME_RENDER_FINAL_SCENE) {
        const uint32_t slot = next_slot ? (next_slot - 1) % 3 : 0;
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
        rsf_fg12_hudless(commands, pass);
        pending.push_back({commands, slot, pass->session_id, pass->source_frame_id, true});
        return 1;
    }
    rsf_godot_settings options;
    uint64_t generation;
    {
        std::lock_guard lock(guard);
        options = settings;
        generation = settings_generation;
    }
    uint32_t fg = options.fg_backend;
    if (fg == RSF_FG_BACKEND_AUTO) fg = rsf_fg_default_backend(vendor_id(), false);
    if (rsf_d3d11_present_has_owner() && fg != rsf_d3d11_present_backend())
        (void)rsf_d3d11_present_request(fg);
    const auto& camera = pass->camera;
    if (context && (generation != context_generation || camera.output_width != output_width || camera.output_height != output_height ||
        (!context_backend && (camera.render_width != desired_width || camera.render_height != desired_height)))) {
        if (!wait(submitted)) return 0;
        rsf_sr12_destroy(context); context = nullptr;
    }
    rsf_backend_result result = RSF_BACKEND_OK;
    if (!context) {
        uint32_t backend = options.sr_backend;
        if (backend == RSF_SR_AUTO) backend = rsf_gpu_default_sr_backend(vendor_id(), false);
        if (backend == RSF_SR_SPATIAL) backend = RSF_SR_NONE;
        rsf_sr12_setup setup{};
        setup.struct_size = sizeof(setup); setup.abi_version = 1; setup.device = device.Get();
        setup.backend = backend; setup.quality = options.quality;
        setup.output_width = camera.output_width; setup.output_height = camera.output_height;
        setup.render_width = camera.render_width; setup.render_height = camera.render_height;
        setup.inverted_depth = 1; setup.streamline_host = rsf_d3d11_present_host();
        setup.fsr2_directory_utf8 = setup.fsr3_directory_utf8 = setup.fsr4_directory_utf8 = fsr.c_str();
        setup.xess_directory_utf8 = xess.c_str(); setup.log = log;
        const std::string interposer = streamline + "/sl.interposer.dll";
        setup.dlss = {sizeof(setup.dlss), RSF_DLSS_ABI_VERSION, interposer.c_str(), streamline.c_str(), root.c_str(),
            0, RSF_DLSS_ENGINE_CUSTOM, version.c_str(), project.c_str(), 1, log, nullptr, options.dlss_preset};
        result = rsf_sr12_create(&setup, &context);
        context_result = result;
        if (!context && backend) {
            setup.backend = 0;
            (void)rsf_sr12_create(&setup, &context);
            backend = 0;
        }
        if (!context) return 0;
        context_backend = backend; context_generation = generation;
        output_width = camera.output_width; output_height = camera.output_height;
        rsf_sr12_plan(context, &desired_width, &desired_height);
    }
    const uint32_t slot = next_slot++ % 3;
    if (!wait(slots[slot])) return 0;
    rsf_game_render_pass frame = *pass;
    frame.screen = options.gameplay && pass->screen == RSF_SCREEN_FLIGHT ? RSF_SCREEN_FLIGHT : RSF_SCREEN_MENU;
    const auto evaluated = rsf_sr12_evaluate(context, commands, &frame, slot);
    static uint32_t diagnostics = 0;
    if (evaluated == RSF_BACKEND_ERROR_INVALID_ARGUMENT && diagnostics++ < 4) {
        char text[256];
        std::snprintf(text, sizeof(text), "Godot SR input refusal: actual=%ux%u output=%ux%u plan=%ux%u device=%p frame=%llu history=%llu",
            camera.render_width, camera.render_height, camera.output_width, camera.output_height,
            desired_width, desired_height, device.Get(), static_cast<unsigned long long>(frame.native_frame),
            static_cast<unsigned long long>(frame.history_key));
        log(nullptr, text);
        void* resources[] = {frame.color_input, frame.depth, frame.motion, frame.color_output};
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
    }
    // The driver submission callback signals this pending slot after recording is submitted.
    pending.push_back({commands, slot, pass->session_id, pass->source_frame_id, false});
    std::lock_guard lock(guard);
    status.requested_sr = options.sr_backend;
    // FSR1 is evaluated by Godot before this callback. The native context only
    // normalizes its depth/motion; it must not overwrite Godot's spatial result.
    status.active_sr = evaluated == RSF_BACKEND_OK ?
        (options.sr_backend == RSF_SR_SPATIAL ? uint32_t(RSF_SR_SPATIAL) : context_backend) : 0;
    status.sr_result = context_result != RSF_BACKEND_OK ? context_result : evaluated;
    status.render_width = desired_width; status.render_height = desired_height;
    status.output_width = output_width; status.output_height = output_height;
    if (evaluated == RSF_BACKEND_OK) ++status.evaluated_frames;
    else ++status.refused_frames;
    return evaluated == RSF_BACKEND_OK ? 1 : 0;
} catch (...) { return 0; }

extern "C" void rsfg_render_begin(uint64_t frame) try
{
    if (rsf::godot_vk::booted()) { rsf::godot_vk::render_begin(frame); return; }
    if (!booted) return;
    current_render_frame = frame;
    if (!frame) return;
    const auto result = rsf_fg12_render_begin(status.session_id, frame);
    if (capture && capture->wants(frame)) {
        rsf_frame_record identity{};
        identity.session_id = status.session_id; identity.frame_id = frame;
        (void)capture->event(identity, "render-submit-begin", frame, false, result);
    }
} catch (...) { current_render_frame = 0; }

extern "C" void rsfg_submitted(void* owning_queue, void* const* command_lists, uint32_t count)
{
    if (!queue || queue.Get() != owning_queue || !fence || !command_lists || !count) return;
    auto matched = [command_lists, count](const Pending& item) {
        return std::find(command_lists, command_lists + count, item.commands) != command_lists + count;
    };
    if (std::none_of(pending.begin(), pending.end(), matched)) return;
    if (FAILED(queue->Signal(fence.Get(), submitted + 1))) return;
    ++submitted;
    for (const auto& item : pending) if (matched(item)) {
        if (item.slot != UINT32_MAX) slots[item.slot] = submitted;
        // HUD-less colour depends on the SR output in the engine graph. Its actual
        // submission establishes both captures; recording completion establishes neither.
        if (item.hudless) rsf_fg12_submitted(item.session, item.frame);
    }
    std::erase_if(pending, matched);
}

extern "C" void rsfg_present(void* chain)
{
    if (!booted || !chain || !current_render_frame) return;
    rsf_game_render_pass pass{};
    if (rendered.source_frame_id == current_render_frame) pass = rendered;
    else {
        { std::lock_guard lock(guard); status.active_sr = 0; }
        pass.struct_size = sizeof(pass); pass.session_id = status.session_id;
        pass.source_frame_id = pass.native_frame = current_render_frame;
        pass.screen = RSF_SCREEN_UNKNOWN;
    }
    pass.role = RSF_GAME_RENDER_WINDOW; pass.swapchain = chain;
    rsf_fg12_window(&pass);
}

extern "C" int rsfg_status(rsf_godot_status* output) try
{
    if (!output || output->struct_size < sizeof(*output) || output->abi_version != RSF_GODOT_ABI_VERSION) return 0;
    if (rsf::godot_vk::booted()) {
        if (!rsf::godot_vk::status(output)) return 0;
        output->keyboard_inputs = input_counts[0].load(); output->mouse_inputs = input_counts[1].load();
        output->controller_inputs = input_counts[2].load(); output->pcl_pings = input_counts[3].load();
        DWORD foreground = 0; GetWindowThreadProcessId(GetForegroundWindow(), &foreground);
        output->foreground_process_id = foreground; output->window_foreground = foreground == GetCurrentProcessId();
        output->window_visible = IsWindowVisible(input_window.load()) ? 1u : 0u;
        return 1;
    }
    if (!booted) return 0;
    rsf_godot_status value;
    { std::lock_guard lock(guard); value = status; }
    rsf_native_fg_status fg{}; fg.struct_size = sizeof(fg); rsf_fg12_status(&fg);
    value.fg_backend = rsf_d3d11_present_backend(); value.fg_available = fg.available; value.fg_active = fg.vendor.active;
    const auto switch_result = rsf_d3d11_present_switch_result();
    value.fg_result = switch_result != RSF_BACKEND_OK ? switch_result : fg.last_result; value.reason = fg.reason;
    value.max_generated_frames = fg.vendor.max_generated_frames; value.generated_frames = fg.vendor.effective_generated_frames;
    value.latency_available = fg.vendor.low_latency_available; value.latency_mode = fg.vendor.effective_reflex;
    value.tagged_frames = fg.tagged_frames;
    value.total_presented = fg.vendor.total_presented; value.generated_presented = fg.vendor.generated_presented;
    value.presentation_statistics_valid = (fg.vendor.valid_statistics & RSF_FG_STAT_TOTAL_PRESENTED) ? 1 : 0;
    value.presentation_statistics_flags = fg.vendor.valid_statistics;
    value.source_frame_id = fg.source_frame_id;
    value.provider_generation = rsf_d3d11_present_generation();
    value.generated_callbacks = fg.vendor.generated_callbacks;
    DWORD foreground = 0;
    GetWindowThreadProcessId(GetForegroundWindow(), &foreground);
    value.foreground_process_id = foreground;
    value.window_foreground = foreground == GetCurrentProcessId();
    value.window_visible = IsWindowVisible(input_window.load()) ? 1u : 0u;
    value.keyboard_inputs = input_counts[0].load(); value.mouse_inputs = input_counts[1].load();
    value.controller_inputs = input_counts[2].load(); value.pcl_pings = input_counts[3].load();
    rsf_streamline_latency_status latency{}; latency.struct_size = sizeof(latency);
    if (rsf_streamline_host_latency_status(rsf_d3d11_present_host(), &latency) == RSF_BACKEND_OK) {
        value.sleep_calls = latency.sleep_calls; value.marker_calls = latency.marker_calls;
        value.pcl_available = latency.pcl_message_id >= 0xc000u;
        value.latency_report_available = latency.latency_report_available;
    }
    *output = value;
    return 1;
} catch (...) { return 0; }

extern "C" int rsfg_settings(rsf_godot_settings* output) try
{
    if (!output || output->struct_size < sizeof(*output) || output->abi_version != RSF_GODOT_ABI_VERSION) return 0;
    if (rsf::godot_vk::booted()) return rsf::godot_vk::settings(output);
    std::lock_guard lock(guard);
    *output = settings;
    return 1;
} catch (...) { return 0; }
extern "C" int rsfg_request_latency_ping(void)
{
    const uint32_t message = ping_message.load();
    const HWND window = input_window.load();
    return (booted || rsf::godot_vk::booted()) && window && message >= 0xc000u && PostMessageW(window, message, 0, 0) ? 1 : 0;
}
extern "C" void rsfg_vk_window(void* window) {
    if (rsf::godot_vk::booted()) accept(nullptr, window);
}
extern "C" int rsfg_vk_present_marker(uint64_t window, uint32_t begin) {
    if (!rsf::godot_vk::booted() || !window || reinterpret_cast<uint64_t>(input_window.load()) != window) return 0;
    rsf_latencyvk_present(begin);
    return 1;
}
extern "C" int32_t rsfg_vk_native_present(uint64_t callback, void* owning_queue, const void* info) {
    return rsf_latencyvk_queue_present(reinterpret_cast<rsf_vk_function>(callback), owning_queue, info);
}
extern "C" int32_t rsfg_vk_wsi_call(rsf_vk_wsi_call callback, void* user) { return rsf_latencyvk_wsi_call(callback, user); }

extern "C" int rsfg_shutdown(void) try
{
    const HWND window = input_window.load();
    if (window && previous_proc && reinterpret_cast<WNDPROC>(GetWindowLongPtrW(window, GWLP_WNDPROC)) == input_proc) {
        SetWindowLongPtrW(window, GWLP_WNDPROC, reinterpret_cast<LONG_PTR>(previous_proc));
        previous_proc = nullptr;
    }
    input_window.store(nullptr); ping_pending.store(false);
    ping_message.store(0);
    if (rsf::godot_vk::booted()) return rsf::godot_vk::shutdown();
    if (!wait(submitted) || !rsf_fg12_end_session(status.session_id)) return 0;
    if (capture) (void)capture->flush();
    rsf_sr12_destroy(context); context = nullptr;
    return 1;
} catch (...) { return 0; }
