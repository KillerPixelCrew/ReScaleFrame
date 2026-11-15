// SPDX-License-Identifier: GPL-3.0-only
#include <rescaleframe/d3d11_host.h>
#include <rescaleframe/native_sr.h>
#include <rescaleframe/native_cpu.h>
#include <rescaleframe/native_scene.h>
#include <rescaleframe/native_window.h>
#include <rescaleframe/native_fg.h>
#include <rescaleframe/d3d11_present_bridge.h>
#include <rescaleframe/dlss_pipeline.h>
#include <rescaleframe/overlay_api.h>
#include <rescaleframe/overlay_input.h>
#include <rescaleframe/gpu_policy.h>
#include <rescaleframe/input_dump.h>
#include <rescaleframe/d3d11_observer.h>
#include <rescaleframe/sr_session.h>
#include <rescaleframe/frame_generation.h>
#include "render_settings_store.h"
#include "auxiliary_sr.h"
#include "sampler_bias.h"
#include <windows.h>
#include <d3d11.h>
#include <dxgi.h>
#include <wrl/client.h>
#include <atomic>
#include <memory>
#include <mutex>
#include <string>
#include <cstdio>
#include <cmath>

namespace {
using Microsoft::WRL::ComPtr;
std::string utf8(const std::wstring& value) {
    const auto size = WideCharToMultiByte(CP_UTF8, 0, value.data(), int(value.size()), nullptr, 0, nullptr, nullptr);
    std::string result(size_t(size), '\0');
    if (size) WideCharToMultiByte(CP_UTF8, 0, value.data(), int(value.size()), result.data(), size, nullptr, nullptr);
    return result;
}
struct Host {
    std::mutex state;
    std::mutex presentation_guard;
    std::unique_ptr<AuxiliarySurfaces> auxiliaries;
    SamplerBias sampler_bias;
    rsf_game_host_services upstream{};
    std::wstring ini, section;
    std::string root, identity, fingerprint, version, project;
    std::string streamline, fidelityfx, xess, captures;
    rsf_d3d11_host_setup setup{};
    rsf_render_settings desired{sizeof(rsf_render_settings), RSF_RENDER_FRONTEND_ABI_VERSION};
    rsf_game_render_config planned{sizeof(rsf_game_render_config)};
    rsf_overlay_api overlay{sizeof(rsf_overlay_api), RSF_OVERLAY_API_VERSION};
    rsf_overlay_sr_report report{sizeof(rsf_overlay_sr_report)};
    ComPtr<IDXGISwapChain> chain;
    ComPtr<ID3D11Device> device;
    rsf_frame_record capture_record{};
    uint64_t capture_view{};
    uint64_t capture_native_frame{};
    bool capture_pending{};
    uint32_t execution_thread{}, backend{}, quality{}, preset{};
    uint32_t output_width{}, output_height{};
    bool configured{}, scene_bound{}, observer{}, attempted{}, running{}, reset{true};
    bool bridge_installed{};
    std::atomic<bool> bridge_bound{};
    std::atomic<bool> capture{};
    std::atomic<bool> active{};
    std::atomic<bool> transport{true}, fidelity{false};
    std::atomic<uint32_t> requested_generation{1};
    uint32_t applied_generation{};
    uint64_t evaluated{}, refused{}, logged{};
    int32_t last_result{RSF_ERROR_NOT_READY};
    int32_t present_result{RSF_DLSS_OK};
    uint64_t present_pairs{};
    uint32_t ownership_reports{};
    std::atomic<uint32_t> cpu_reports{};
    static void log(void* user, const char* text) {
        auto& self = *static_cast<Host*>(user);
        if (self.upstream.log) self.upstream.log(self.upstream.user, text);
    }
    static void changed(void* user, const rsf_render_settings* settings) {
        auto& self = *static_cast<Host*>(user);
        std::lock_guard lock(self.state); self.desired = *settings; ++self.requested_generation;
    }
    static int sr_report(void* user, rsf_overlay_sr_report* output) {
        auto& self = *static_cast<Host*>(user);
        if (!output || output->struct_size < sizeof(*output)) return 0;
        std::lock_guard lock(self.state); *output = self.report; return 1;
    }
    static void cpu(void* user, const rsf_game_cpu_event* event) {
        auto& self = *static_cast<Host*>(user);
        // Installation/preparation alone does not prove the engine retained the facade.
        if (self.setup.presentation_bridge && !self.bridge_bound.load(std::memory_order_acquire)) return;
        if (self.bridge_installed) {
            const auto accepted = rsf_native_cpu_dispatch(event, [](void*, const rsf_game_cpu_event* fact) {
                rsf_native_fg_cpu(fact);
            }, nullptr);
            if (event && self.cpu_reports.fetch_add(1) < 24) {
                char text[180]{}; std::snprintf(text, sizeof(text),
                    "D3D11 CPU fact stage=%u source=%llu accepted=%d flags=0x%x",
                    event->stage, static_cast<unsigned long long>(event->source_frame_id), accepted, event->flags);
                log(&self, text);
            }
            return; // This host owns normalization; do not dispatch the same fact upstream twice.
        }
        if (self.upstream.cpu_event) self.upstream.cpu_event(self.upstream.user, event);
        else (void)rsf_native_cpu_event(event);
    }
    static int accept_window(void* user, void* window) {
        auto& self = *static_cast<Host*>(user);
        return self.setup.accept_window && self.setup.accept_window(self.setup.presentation_user, window);
    }
    void install_bridge() {
        if (!setup.presentation_bridge) return;
        rsf_d3d11_present_setup bridge{}; bridge.struct_size = sizeof(bridge);
        bridge.runtime_directory_utf8 = streamline.c_str(); bridge.log = log; bridge.user = this;
        bridge.accept_window = accept_window; bridge.present_event = present;
        bridge.prepare = rsf_native_fg_prepare; bridge.retire = rsf_native_fg_retire;
        bridge.latency_event = [](void*, const rsf_observer_present_event* event) { rsf_native_fg_present(event); };
        const bool rendered = (setup.capabilities & RSF_D3D11_HOST_CAP_RENDERED_SOURCE) != 0;
        rsf_native_fg_rendered_source_set(rendered);
        if (rendered) bridge.allowed_backend_choices = (1u << RSF_FG_BACKEND_FSR3) | (1u << RSF_FG_BACKEND_FSR4);
        bridge.backend = 0; bridge.runtime_switching = 1; bridge.max_generated_frames = 15;
        bridge.depth_inverted = bridge.depth_infinite = setup.depth_inverted;
        bridge.units_to_meters = setup.units_to_meters; bridge.ui_mode = RSF_UI_MODE_BACKBUFFER_HUDLESS;
        bridge.fsr3_directory_utf8 = bridge.fsr4_directory_utf8 = fidelityfx.c_str();
        bridge.xess_directory_utf8 = xess.c_str(); bridge.streamline_directory_utf8 = streamline.c_str();
        bridge.engine_type = setup.engine_type; bridge.engine_version_utf8 = version.c_str(); bridge.project_id_utf8 = project.c_str();
        bridge_installed = (setup.presentation_bridge == 2 ? rsf_d3d11_present_prepare(&bridge) :
            rsf_d3d11_present_install(&bridge)) != 0;
        if (bridge_installed) {
            rsf_native_fg_set_log(log, this);
            const rsf_native_fg_options off{sizeof(off), RSF_FG_OFF, 1, RSF_REFLEX_OFF, 0, 0};
            rsf_native_fg_options_set(&off);
        }
        log(this, bridge_installed ? (setup.presentation_bridge == 2 ?
            "D3D11 callback host prepared outer creation root; facade ownership awaits engine binding." :
            "D3D11 callback host installed cold presentation bridge; facade ownership awaits engine binding.") :
            "D3D11 callback host presentation bridge refused; native presentation retained.");
    }
    static void passes(void* user, void* raw_context, const rsf_game_render_pass* pass, uint32_t begin) {
        auto& self = *static_cast<Host*>(user);
        if (!pass || pass->struct_size < sizeof(*pass) || begin > 1 || pass->session_id != self.upstream.session_id) return;
        std::unique_lock presentation(self.presentation_guard, std::try_to_lock);
        if (!presentation.owns_lock() || !self.configured || self.execution_thread != GetCurrentThreadId() ||
            (begin && !self.active.load(std::memory_order_acquire))) return;
        // Native ownership facts remain useful before a generation provider is enabled.
        if (pass->role == RSF_GAME_RENDER_FRAME && self.bridge_installed) rsf_native_fg_frame(pass, begin);
        else if (pass->role == RSF_GAME_RENDER_SUBMISSION && self.bridge_installed) rsf_native_fg_submission(pass, begin);
        else if (pass->role == RSF_GAME_RENDER_FG_INPUTS) {
            bool requested{}; { std::lock_guard lock(self.state); requested = self.desired.fg_enabled != 0; }
            if (requested && begin && self.bridge_installed && (self.setup.capabilities & RSF_D3D11_HOST_CAP_FG_INPUTS) && raw_context &&
                pass->swapchain == self.chain.Get()) {
                auto* context = static_cast<ID3D11DeviceContext*>(raw_context);
                ComPtr<ID3D11Device> device; context->GetDevice(&device);
                if (device.Get() == self.device.Get() && context->GetType() == D3D11_DEVICE_CONTEXT_IMMEDIATE)
                    (void)rsf_native_sr_capture_fg(context, pass, &self.setup.motion);
            }
        }
        else if (pass->role == RSF_GAME_RENDER_FINAL_SCENE) {
            if (!raw_context || !pass->scene_surface) return;
            auto* context = static_cast<ID3D11DeviceContext*>(raw_context);
            if (context->GetType() != D3D11_DEVICE_CONTEXT_IMMEDIATE) return;
            ComPtr<ID3D11Device> device; context->GetDevice(&device);
            if (device.Get() != self.device.Get()) return;
            static_cast<ID3D11Texture2D*>(pass->scene_surface)->GetDevice(&device);
            if (device.Get() != self.device.Get()) return;
            const auto accepted = rsf_native_scene_pass(pass, begin);
            if (accepted && !begin && self.bridge_installed) rsf_native_fg_final(context, pass);
        }
        else if (pass->role == RSF_GAME_RENDER_WINDOW && pass->swapchain == self.chain.Get())
            (void)rsf_native_window_scope(pass, begin);
    }
    static int config(void* user, rsf_game_render_config* out) {
        if (!out || out->struct_size < sizeof(*out)) return 0;
        auto& self = *static_cast<Host*>(user); std::lock_guard lock(self.state);
        *out = self.planned;
        out->enabled = out->enabled && self.desired.sr_enabled && self.active.load(std::memory_order_acquire);
        return 1;
    }
    static void feedback(void* user, const rsf_game_render_feedback* event) {
        auto& self = *static_cast<Host*>(user);
        if (!event || event->struct_size < sizeof(*event) || event->session_id != self.upstream.session_id) return;
        std::lock_guard lock(self.state);
        if (event->reason) {
            self.report.effective_backend = 0;
            std::snprintf(self.report.reason, sizeof(self.report.reason), "Native input admission refused (reason %u)", event->reason);
        }
    }
    static void options(void* user, int32_t* values, uint32_t count) {
        auto& self = *static_cast<Host*>(user);
        if (count) values[0] = 0;
        if (count > 1) values[1] = self.transport.load();
        if (count > 2) values[2] = self.fidelity.load();
    }
    static int action(void* user, uint32_t index, int32_t value) {
        auto& self = *static_cast<Host*>(user);
        if (index > 2 || value < 0 || value > 1) return 0;
        if (!index) { if (!value) return 0; self.capture.store(true, std::memory_order_release); return 1; }
        if (!WritePrivateProfileStringW(self.section.c_str(), index == 1 ? L"ColourTransport" : L"ColourCorrection",
                value ? L"1" : L"0", self.ini.c_str())) return 0;
        (index == 1 ? self.transport : self.fidelity).store(value != 0, std::memory_order_release);
        ++self.requested_generation; return 1;
    }
    static void present(void* user, const rsf_observer_present_event* event) {
        auto& self = *static_cast<Host*>(user);
        std::unique_lock presentation(self.presentation_guard, std::try_to_lock);
        if (!presentation.owns_lock()) return;
        if (!self.active.load(std::memory_order_acquire) || !event || (event->flags & DXGI_PRESENT_TEST) ||
            GetCurrentThreadId() != self.execution_thread || event->swapchain != self.chain.Get()) return;
        (void)rsf_native_window_present(event);
        if (event->completed && self.ownership_reports < 4) {
            rsf_native_present_record observed{}; observed.struct_size = sizeof(observed);
            if (rsf_native_window_last_present(&observed) && observed.matched) {
                ++self.ownership_reports;
                char message[240]{}; std::snprintf(message, sizeof(message),
                    "D3D11 native ownership source=%llu submission=%llu native_frame=%llu scene=%u direct=%u accepted=%u; generation still requires complete inputs and policy",
                    static_cast<unsigned long long>(observed.source_frame_id),
                    static_cast<unsigned long long>(observed.scene_submission_id),
                    static_cast<unsigned long long>(observed.scene_native_frame), observed.scene_matched,
                    observed.scene_direct_present, observed.accepted);
                log(&self, message);
            }
        }
        if (self.running) {
            const auto result = rsf_dlss_native_present(event->swapchain, event->sync_interval, event->flags, !event->completed);
            if (event->completed) ++self.present_pairs;
            if (result != self.present_result || (event->completed && self.present_pairs <= 2)) {
                char message[140]{}; std::snprintf(message, sizeof(message), "D3D11 host native Present bookkeeping result=%d completed=%u pairs=%llu",
                    result, event->completed, static_cast<unsigned long long>(self.present_pairs));
                log(&self, message);
            }
            self.present_result = result;
        }
        if (self.capture_pending && rsf_input_dump_wants(&self.capture_record)) {
            if (!(self.capture_record.flags & RSF_FRAME_FLAG_AMBIGUOUS_ID)) {
                rsf_game_render_pass window{}; window.struct_size = sizeof(window);
                if (!rsf_native_window_match(event->swapchain, &window) ||
                    window.session_id != self.capture_record.session_id || window.source_frame_id != self.capture_record.frame_id ||
                    window.native_frame != self.capture_native_frame || window.view_key != self.capture_view ||
                    rsf_native_scene_present_path(&window) == RSF_NATIVE_SCENE_PRESENT_NONE) {
                    rsf_input_dump_complete(&self.capture_record, RSF_ERROR_NOT_READY); self.capture_pending = false;
                }
            }
        }
        if (self.capture_pending && rsf_input_dump_wants(&self.capture_record)) {
            if (!event->completed) {
                ComPtr<ID3D11DeviceContext> context; self.device->GetImmediateContext(&context);
                ComPtr<ID3D11Texture2D> buffer;
                if (SUCCEEDED(self.chain->GetBuffer(0, IID_PPV_ARGS(&buffer))))
                    (void)rsf_input_dump_texture_record(context.Get(), &self.capture_record, "final_color",
                        "Actual native Present colour before shared overlay", buffer.Get(), RSF_DUMP_VIEW_HDR, 1);
            } else {
                rsf_input_dump_complete(&self.capture_record, event->result); self.capture_pending = false;
            }
        }
        if (event->completed) return;
        rsf_overlay_input_poll_toggle();
        (void)self.overlay.present(event->swapchain, nullptr);
    }
    bool initialize(ID3D11DeviceContext* context, const rsf_game_render_pass& pass) {
        if (configured) return true;
        if (setup.presentation_bridge && (!bridge_installed || !rsf_d3d11_present_is_owner(pass.swapchain))) {
            log(this, "D3D11 host refused presentation binding: engine chain is not the final application facade.");
            return false;
        }
        context->GetDevice(&device);
        ComPtr<IDXGIDevice> dxgi; ComPtr<IDXGIAdapter> adapter; DXGI_ADAPTER_DESC desc{};
        if (FAILED(device.As(&dxgi)) || FAILED(dxgi->GetAdapter(&adapter)) || FAILED(adapter->GetDesc(&desc))) return false;
        const uint32_t selected = rsf_gpu_select_sr_backend(desc.VendorId, desc.VendorId == 0x1414, 0x3eu);
        if (!selected || rsf_get_overlay_api(&overlay) != RSF_OK) return false;
        rsf_overlay_service_setup panel{}; panel.struct_size = sizeof(panel); panel.abi_version = RSF_OVERLAY_API_VERSION;
        panel.identity_utf8 = identity.c_str(); panel.configuration_path = ini.c_str(); panel.configuration_section = section.c_str();
        panel.defaults = {sizeof(rsf_render_settings), RSF_RENDER_FRONTEND_ABI_VERSION,
            1, selected, 0, 0, RSF_FG_BACKEND_AUTO, 1, RSF_REFLEX_OFF, 0, 1, 0};
        panel.sr_backend_choices = 0x3e; panel.sr_quality_choices = setup.quality_choices;
        const bool timeline = bridge_installed && (setup.capabilities & RSF_D3D11_HOST_CAP_INPUT_TIMELINE);
        const bool rendered = bridge_installed && (setup.capabilities & RSF_D3D11_HOST_CAP_RENDERED_SOURCE);
        const bool generation = (timeline || rendered) && (setup.capabilities & RSF_D3D11_HOST_CAP_FG_INPUTS);
        panel.fg_backend_choices = generation ? ((1u << RSF_FG_BACKEND_AUTO) | (1u << RSF_FG_BACKEND_DLSS) |
            (1u << RSF_FG_BACKEND_FSR3) | (1u << RSF_FG_BACKEND_FSR4) | (1u << RSF_FG_BACKEND_XESS)) : 0;
        if (rendered) {
            panel.fg_backend_choices = generation ? ((1u << RSF_FG_BACKEND_FSR3) | (1u << RSF_FG_BACKEND_FSR4)) : 0;
            panel.defaults.fg_backend = RSF_FG_BACKEND_FSR3;
        }
        panel.latency_choices = timeline ? ((1u << RSF_REFLEX_OFF) | (1u << RSF_REFLEX_ON) |
            (1u << RSF_REFLEX_BOOST)) : (1u << RSF_REFLEX_OFF);
        panel.max_generated_frames = generation ? 15u : 1u;
        if (rendered) log(this, "D3D11 rendered-source FG: input timing unavailable; latency Off, no pre-input pacing.");
        else if (!timeline) log(this, "D3D11 host refuses FG/latency controls: native input timeline capability unavailable.");
        else if (!generation) log(this, "D3D11 host refuses FG controls: native FG input lease capability unavailable.");
        panel.dlss_preset_available = 1; panel.log = log; panel.sr_report = sr_report; panel.settings_changed = changed; panel.user = this;
        transport.store(GetPrivateProfileIntW(section.c_str(), L"ColourTransport", 1, ini.c_str()) != 0);
        fidelity.store(GetPrivateProfileIntW(section.c_str(), L"ColourCorrection", 0, ini.c_str()) != 0);
        const rsf_overlay_service_option advanced[]{
            {"Capture next SR inputs", RSF_OVERLAY_SERVICE_OPTION_TOGGLE, 0, 0, 1, 1, 1, 0},
            {"DLSS colour transport", RSF_OVERLAY_SERVICE_OPTION_TOGGLE, int32_t(transport.load()), 0, 1, 1, 1, 0},
            {"SR colour correction", RSF_OVERLAY_SERVICE_OPTION_TOGGLE, int32_t(fidelity.load()), 0, 1, 1, 1, 0}};
        panel.option_count = 3; panel.options = advanced; panel.options_read = options; panel.option_changed = action;
        if (overlay.configure(&panel) != RSF_OK || overlay.settings_get(&desired) != RSF_OK) return false;
        execution_thread = GetCurrentThreadId(); chain = static_cast<IDXGISwapChain*>(pass.swapchain);
        rsf_observer_options observe{}; observe.struct_size = sizeof(observe); observe.abi_version = RSF_OBSERVER_ABI_VERSION;
        observe.format = UINT32_MAX; observe.minimum_width = UINT32_MAX; observe.capacity = 1;
        observe.log = log; observe.log_user = this; observe.on_present_event = present; observe.on_present_event_user = this;
        if (rsf_d3d11_present_is_owner(pass.swapchain)) {
            observer = true;
            log(this, "D3D11 callback host uses bridge application Present events; native DXGI interception is unnecessary.");
        } else observer = rsf_observer_install(&observe) == RSF_OBSERVER_OK;
        if (!observer) log(this, "D3D11 callback host: Present observation refused; SR activation suspended.");
        rsf_input_dump_configure(captures.c_str(), identity.c_str(), fingerprint.c_str(), log, this);
        std::snprintf(report.reason, sizeof(report.reason), "Waiting for verified scene inputs");
        configured = true;
        if (observer && setup.presentation_bridge) bridge_bound.store(true, std::memory_order_release);
        log(this, "D3D11 callback host configured; FG/latency controls follow declared capabilities and native eligibility.");
        return observer;
    }
    void capture_inputs(void* context, const rsf_game_render_pass& pass, rsf_frame_record& record) {
        record.struct_size = sizeof(record); record.abi_version = RSF_GAME_FRAME_ABI_VERSION;
        record.frame_id = pass.source_frame_id ? pass.source_frame_id : pass.native_frame;
        record.session_id = pass.session_id; record.view_id = uint32_t(pass.view_key);
        record.flags = RSF_FRAME_FLAG_NO_FG | (pass.source_frame_id ? 0u : RSF_FRAME_FLAG_AMBIGUOUS_ID);
        record.render_width = pass.camera.render_width; record.render_height = pass.camera.render_height;
        record.output_width = pass.camera.output_width; record.output_height = pass.camera.output_height;
        record.camera = pass.camera; record.frame_time_ms = pass.camera.frame_time_seconds * 1000;
        if (capture.exchange(false, std::memory_order_acq_rel)) rsf_input_dump_request();
        rsf_input_dump_poll();
        if (!rsf_input_dump_begin_record(&record, backend)) return;
        rsf_input_dump_pass_metadata(&pass);
        rsf_input_dump_pass(context, &pass, 1);
        (void)rsf_input_dump_texture_record(context, &record, "native_color", "Native pre-tonemap scene colour", pass.color_input, RSF_DUMP_VIEW_HDR, 1);
        (void)rsf_input_dump_texture_record(context, &record, "native_depth", "Native scene device depth", pass.depth, RSF_DUMP_VIEW_DEPTH, 1);
        (void)rsf_input_dump_texture_record(context, &record, "native_motion", "Native motion in adapter-declared encoding", pass.motion, RSF_DUMP_VIEW_DENSE_MOTION, 8);
    }
    void prepare_pipeline(const rsf_game_render_pass& pass, const rsf_render_settings& settings) {
        if (settings.sr_enabled) {
            if (!running && !attempted) {
                attempted = true;
                rsf_dlss_pipeline_setup create{}; create.struct_size = sizeof(create); create.abi_version = RSF_DLSS_PIPELINE_ABI_VERSION;
                create.streamline_directory_utf8 = streamline.c_str(); create.require_signature = 1;
                create.output_width = pass.camera.output_width; create.output_height = pass.camera.output_height;
                create.quality = settings.sr_quality; create.motion = setup.motion; create.log = log; create.log_user = this;
                create.engine = setup.engine_type; create.engine_version_utf8 = version.c_str(); create.project_id_utf8 = project.c_str();
                create.fsr2_directory_utf8 = create.fsr3_directory_utf8 = create.fsr4_directory_utf8 = fidelityfx.c_str();
                create.xess_directory_utf8 = xess.c_str(); create.view_space_to_meters = setup.units_to_meters;
                create.standard_depth = !setup.depth_inverted;
                last_result = rsf_dlss_pipeline_start(device.Get(), &create); running = last_result == RSF_DLSS_PIPELINE_OK;
                backend = RSF_SR_DLSS; quality = settings.sr_quality; output_width = create.output_width; output_height = create.output_height;
            }
            if (running && present_result == RSF_DLSS_OK) {
                if (output_width != pass.camera.output_width || output_height != pass.camera.output_height) {
                    last_result = rsf_dlss_pipeline_resize_output(pass.camera.output_width, pass.camera.output_height, nullptr, nullptr);
                    if (!last_result) { output_width = pass.camera.output_width; output_height = pass.camera.output_height; reset = true; }
                }
                if (backend != settings.sr_backend) {
                    last_result = rsf_dlss_pipeline_select_backend(settings.sr_backend, nullptr, nullptr);
                    if (!last_result) { backend = settings.sr_backend; reset = true; }
                }
                if (quality != settings.sr_quality) {
                    last_result = rsf_dlss_pipeline_set_quality(settings.sr_quality, nullptr, nullptr);
                    if (!last_result) { quality = settings.sr_quality; reset = true; }
                }
                if (preset != settings.dlss_preset) {
                    last_result = rsf_dlss_pipeline_set_preset(settings.dlss_preset);
                    if (!last_result) { preset = settings.dlss_preset; reset = true; }
                }
                const auto sharpen = rsf::render_settings::sharpening(settings);
                (void)rsf_dlss_pipeline_set_sharpening(&sharpen);
            }
        }
        rsf_dlss_pipeline_status status{}; status.struct_size = sizeof(status);
        const bool ready = running && rsf_dlss_pipeline_get_status(&status) == RSF_DLSS_PIPELINE_OK &&
            status.running && status.output_width == pass.camera.output_width && status.output_height == pass.camera.output_height &&
            quality == settings.sr_quality && backend == settings.sr_backend && present_result == RSF_DLSS_OK;
        std::lock_guard lock(state);
        planned = {sizeof(planned), uint32_t(ready && settings.sr_enabled), pass.camera.output_width, pass.camera.output_height,
            ready ? status.render_width : pass.camera.output_width, ready ? status.render_height : pass.camera.output_height};
    }
    int32_t process(ID3D11DeviceContext* context, const rsf_game_render_pass& pass,
        const rsf_d3d11_auxiliary_pass* auxiliary = nullptr) {
        std::unique_lock presentation(presentation_guard, std::try_to_lock);
        if (!presentation.owns_lock()) return RSF_ERROR_BUSY;
        if (!active.load(std::memory_order_acquire)) return RSF_ERROR_NOT_READY;
        if (auxiliaries && auxiliaries->pending()) return RSF_ERROR_BUSY;
        // Bind ownership from a verified scene lease, never from an early loading callback.
        if ((pass.flags & RSF_GAME_RENDER_RECONFIGURE) && !scene_bound) return RSF_ERROR_NOT_READY;
        if (!initialize(context, pass) || !observer || execution_thread != GetCurrentThreadId()) return RSF_ERROR_NOT_READY;
        ComPtr<ID3D11Device> current; context->GetDevice(&current);
        if (current.Get() != device.Get() || pass.swapchain != chain.Get() || context->GetType() != D3D11_DEVICE_CONTEXT_IMMEDIATE)
            return RSF_ERROR_NOT_READY;
        scene_bound = true;
        rsf_render_settings settings{}; { std::lock_guard lock(state); settings = desired; }
        const auto generation = requested_generation.load();
        if (generation != applied_generation) { attempted = false; reset = true; applied_generation = generation; }
        rsf_dlss_pipeline_set_colour_transport(transport.load(std::memory_order_acquire));
        rsf_dlss_pipeline_set_colour_correction(fidelity.load(std::memory_order_acquire));
        prepare_pipeline(pass, settings);
        const bool configure_only = (pass.flags & RSF_GAME_RENDER_RECONFIGURE) != 0;
        if (configure_only && !pass.camera_valid) return RSF_ERROR_NOT_READY;
        rsf_frame_record record{}; capture_inputs(context, pass, record);
        if (auxiliary && rsf_input_dump_wants(&record)) for (uint32_t i = 0; i < auxiliary->surface_count; ++i) {
            const auto& a = auxiliary->surfaces[i];
            if (a.source_name) (void)rsf_input_dump_texture_record(context, &record, a.source_name,
                "Original auxiliary before reconstruction", a.texture, RSF_DUMP_VIEW_RAW, 1);
        }
        int32_t result = RSF_ERROR_NOT_READY;
        if (settings.sr_enabled) {
            if (running && present_result == RSF_DLSS_OK && !configure_only) {
                auto evaluated_pass = pass;
                if (reset) evaluated_pass.flags |= RSF_GAME_RENDER_RESET;
                if (auxiliary && !auxiliaries) auxiliaries = std::make_unique<AuxiliarySurfaces>();
                const bool prepared = !auxiliary || auxiliaries->prepare(context, evaluated_pass, *auxiliary, log, this);
                result = prepared ? rsf_native_sr_evaluate(context, &evaluated_pass) : RSF_ERROR_NOT_READY;
                if (!result && auxiliary && !auxiliaries->commit()) result = RSF_ERROR_NOT_READY;
                if (result && auxiliary) (void)auxiliaries->restore();
                if (!result) { reset = false; ++evaluated; } else { reset = true; ++refused; }
            } else { result = configure_only ? RSF_ERROR_NOT_READY : running ? present_result : last_result; reset = true; }
        } else reset = true;
        if (rsf_input_dump_wants(&record)) {
            if (!result) (void)rsf_input_dump_texture_record(context, &record, "sr_reinserted", "Reconstructed linear colour before native HDR", pass.color_output, RSF_DUMP_VIEW_HDR, 1);
            if (!result && auxiliary) for (uint32_t i = 0; i < auxiliary->surface_count; ++i) {
                const auto& a = auxiliary->surfaces[i];
                if (a.name) (void)rsf_input_dump_texture_record(context, &record, a.name,
                    "Reconstructed auxiliary before native post-processing", a.texture,
                    a.kind == RSF_D3D11_AUXILIARY_DEPTH_STENCIL ? RSF_DUMP_VIEW_DEPTH : RSF_DUMP_VIEW_RAW, 1);
            }
            rsf_input_dump_result(&record, "sr", backend, result);
            rsf_input_dump_pass(context, &pass, 0);
            capture_record = record; capture_view = pass.view_key; capture_native_frame = pass.native_frame; capture_pending = true;
        }
        {
            std::lock_guard lock(state); report = {sizeof(report)};
            report.effective_backend = !result ? backend : 0; report.effective_quality = quality;
            report.render_width = pass.camera.render_width; report.render_height = pass.camera.render_height;
            report.output_width = pass.camera.output_width; report.output_height = pass.camera.output_height;
            report.frames_evaluated = uint32_t(evaluated); report.frames_refused = uint32_t(refused);
            report.last_result = result; report.quality_locked = setup.quality_choices == 1;
            report.inputs = RSF_OVERLAY_SR_INPUT_COLOR | RSF_OVERLAY_SR_INPUT_DEPTH | RSF_OVERLAY_SR_INPUT_MOTION | RSF_OVERLAY_SR_INPUT_JITTER;
            report.jitter_pixels[0] = pass.camera.jitter_pixels[0]; report.jitter_pixels[1] = pass.camera.jitter_pixels[1];
            if (configure_only) std::snprintf(report.reason, sizeof(report.reason), "Renderer preparation only; native temporal processing retained");
            else if (result) std::snprintf(report.reason, sizeof(report.reason), "SR result %d; native temporal processing retained", result);
        }
        if (result != last_result || logged < 3) {
            ++logged;
            char message[160]{}; std::snprintf(message, sizeof(message), "D3D11 host SR result=%d evaluated=%llu refused=%llu native_frame=%llu",
                result, static_cast<unsigned long long>(evaluated), static_cast<unsigned long long>(refused), static_cast<unsigned long long>(pass.native_frame));
            log(this, message);
        }
        last_result = result; return result;
    }
};
std::mutex creation;
std::atomic<Host*> owner{};
int32_t process(void* user, void* context, const rsf_game_render_pass* pass) noexcept try {
    if (!context || !pass || pass->struct_size < sizeof(*pass) || pass->role != RSF_GAME_RENDER_SR || !pass->swapchain ||
        (!pass->camera_valid && !(pass->flags & RSF_GAME_RENDER_RECONFIGURE)) ||
        !pass->camera.output_width || !pass->camera.output_height) return RSF_ERROR_INVALID_ARGUMENT;
    return static_cast<Host*>(user)->process(static_cast<ID3D11DeviceContext*>(context), *pass);
} catch (...) { return RSF_ERROR_NOT_READY; }
}
extern "C" RSF_RUNTIME_API int32_t rsf_d3d11_host_create(const rsf_d3d11_host_setup* setup, rsf_game_host_services* services) try
{
    if (!setup || setup->struct_size < sizeof(*setup) || !services || services->struct_size < sizeof(*services)) return RSF_ERROR_INVALID_ARGUMENT;
    if (setup->abi_version != RSF_D3D11_HOST_ABI_VERSION || services->abi_version != RSF_GAME_ABI_VERSION) return RSF_ERROR_ABI_MISMATCH;
    if (!setup->configuration_path || !setup->configuration_section || !setup->runtime_directory || !setup->identity_utf8 ||
        !setup->executable_sha256 || !setup->engine_version_utf8 || !setup->project_id_utf8 || !services->session_id ||
        !std::isfinite(setup->units_to_meters) || setup->units_to_meters <= 0 || !setup->quality_choices || (setup->quality_choices & ~0x1fu) ||
        setup->motion.struct_size < sizeof(setup->motion) || !setup->motion.scale_x || !setup->motion.scale_y ||
        !std::isfinite(setup->motion.scale_x) || !std::isfinite(setup->motion.scale_y) ||
        setup->presentation_bridge > 2 || (setup->presentation_bridge && !setup->accept_window) ||
        (setup->capabilities & ~(RSF_D3D11_HOST_CAP_FG_INPUTS | RSF_D3D11_HOST_CAP_INPUT_TIMELINE | RSF_D3D11_HOST_CAP_RENDERED_SOURCE)) ||
        (setup->capabilities && !setup->presentation_bridge) ||
        ((setup->capabilities & RSF_D3D11_HOST_CAP_RENDERED_SOURCE) &&
            (setup->capabilities & RSF_D3D11_HOST_CAP_INPUT_TIMELINE)) ||
        services->render_pass || services->render_config || services->process_upscale || services->present || services->scope_state ||
        services->submit || services->process_latency || services->render_feedback) return RSF_ERROR_INVALID_ARGUMENT;
    std::lock_guard lock(creation); if (owner.load()) return RSF_ERROR_BUSY;
    auto self = std::make_unique<Host>(); self->setup = *setup; self->upstream = *services;
    self->ini = setup->configuration_path; self->section = setup->configuration_section; self->root = utf8(setup->runtime_directory);
    self->identity = setup->identity_utf8; self->fingerprint = setup->executable_sha256;
    self->version = setup->engine_version_utf8; self->project = setup->project_id_utf8;
    // Numeric metadata and process-lived policy callbacks survive; strings are copied.
    self->setup.configuration_path = self->setup.configuration_section = self->setup.runtime_directory = nullptr;
    self->setup.identity_utf8 = self->setup.executable_sha256 = self->setup.engine_version_utf8 = self->setup.project_id_utf8 = nullptr;
    self->streamline = self->root + "/streamline"; self->fidelityfx = self->root + "/fidelityfx";
    self->xess = self->root + "/xess"; self->captures = self->root + "/captures";
    self->install_bridge();
    *services = {sizeof(*services), RSF_GAME_ABI_VERSION, self->upstream.session_id, Host::log, nullptr, self.get(), Host::config, Host::cpu};
    services->process_upscale = process; services->render_feedback = Host::feedback;
    services->render_pass = Host::passes;
    owner.store(self.release(), std::memory_order_release); return RSF_OK;
} catch (...) { return RSF_ERROR_NOT_READY; }
extern "C" RSF_RUNTIME_API uint32_t rsf_d3d11_host_request_capture(void*) {
    auto* self = owner.load(std::memory_order_acquire); if (!self) return uint32_t(RSF_ERROR_NOT_READY);
    self->capture.store(true, std::memory_order_release); return RSF_OK;
}
extern "C" RSF_RUNTIME_API int32_t rsf_d3d11_host_set_active(void* user, uint32_t active) {
    auto* self = owner.load(std::memory_order_acquire);
    if (!self || user != self || active > 1) return RSF_ERROR_INVALID_ARGUMENT;
    if (self->active.exchange(active != 0, std::memory_order_acq_rel) != (active != 0)) ++self->requested_generation;
    return RSF_OK;
}
extern "C" RSF_RUNTIME_API int32_t rsf_d3d11_host_wrap_native_swapchain(void* user,
    const rsf_d3d11_native_chain* request) try
{
    auto* self = owner.load(std::memory_order_acquire);
    if (!self || user != self || !request || request->struct_size < sizeof(*request)) return RSF_ERROR_INVALID_ARGUMENT;
    if (request->abi_version != RSF_D3D11_HOST_ABI_VERSION) return RSF_ERROR_ABI_MISMATCH;
    if (!request->device || !request->immediate_context || !request->swapchain || !*request->swapchain)
        return RSF_ERROR_INVALID_ARGUMENT;
    if (self->setup.presentation_bridge != 2 || !self->bridge_installed) return RSF_ERROR_NOT_READY;
    std::unique_lock presentation(self->presentation_guard, std::try_to_lock);
    if (!presentation.owns_lock() || self->configured || self->scene_bound) return RSF_ERROR_NOT_READY;
    auto* context = static_cast<ID3D11DeviceContext*>(request->immediate_context);
    if (context->GetType() != D3D11_DEVICE_CONTEXT_IMMEDIATE) return RSF_ERROR_INVALID_ARGUMENT;
    ComPtr<ID3D11Device> context_device; context->GetDevice(&context_device);
    ComPtr<IUnknown> expected, observed;
    if (FAILED(static_cast<IUnknown*>(request->device)->QueryInterface(IID_PPV_ARGS(&expected))) ||
        FAILED(context_device.As(&observed)) || expected.Get() != observed.Get()) return RSF_ERROR_INVALID_ARGUMENT;
    auto* native = static_cast<IDXGISwapChain*>(*request->swapchain);
    void* facade = nullptr;
    const auto result = rsf_d3d11_present_wrap_native_chain(request->device, native, &facade);
    if (FAILED(result) || !facade) {
        char text[192]{}; std::snprintf(text, sizeof(text), "D3D11 outer creation facade refused HRESULT=0x%08lx; native outputs preserved",
            static_cast<unsigned long>(result)); Host::log(self, text); return RSF_ERROR_NOT_READY;
    }
    if (!rsf_d3d11_present_is_owner(facade)) {
        static_cast<IDXGISwapChain*>(facade)->Release();
        Host::log(self, "D3D11 outer creation refused non-owner facade; native outputs preserved.");
        return RSF_ERROR_NOT_READY;
    }
    void* native_identity = nullptr; void* facade_identity = nullptr;
    ComPtr<IUnknown> native_owner, facade_owner;
    (void)native->QueryInterface(IID_PPV_ARGS(&native_owner));
    (void)static_cast<IDXGISwapChain*>(facade)->QueryInterface(IID_PPV_ARGS(&facade_owner));
    native_identity = native_owner.Get(); facade_identity = facade_owner.Get();
    char identity[320]{}; std::snprintf(identity, sizeof(identity),
        "D3D11 outer creation identity native=%p unknown=%p Present=%p facade=%p unknown=%p Present=%p owner=%d",
        static_cast<void*>(native), native_identity, (*reinterpret_cast<void***>(native))[8], facade,
        facade_identity, (*reinterpret_cast<void***>(facade))[8], rsf_d3d11_present_is_owner(facade));
    Host::log(self, identity);
    *request->swapchain = facade;
    native->Release();
    Host::log(self, "D3D11 outer native creation returned final application facade; native creation HRESULT preserved by adapter.");
    return RSF_OK;
} catch (...) { return RSF_ERROR_NOT_READY; }
extern "C" RSF_RUNTIME_API int32_t rsf_d3d11_host_bind_presentation(void* user,
    const rsf_d3d11_presentation_binding* binding) try
{
    auto* self = owner.load(std::memory_order_acquire);
    if (!self || user != self || !binding || binding->struct_size < sizeof(*binding) || !binding->swapchain)
        return RSF_ERROR_INVALID_ARGUMENT;
    if (binding->abi_version != RSF_D3D11_HOST_ABI_VERSION) return RSF_ERROR_ABI_MISMATCH;
    if (!self->active.load(std::memory_order_acquire)) return RSF_ERROR_NOT_READY;
    std::unique_lock presentation(self->presentation_guard, std::try_to_lock);
    if (!presentation.owns_lock()) return RSF_ERROR_BUSY;
    if (self->configured) {
        if (!self->observer || self->chain.Get() != binding->swapchain) return RSF_ERROR_NOT_READY;
        const auto thread = GetCurrentThreadId();
        if (self->execution_thread != thread) {
            // Startup presentation can transfer from a loading worker to the main
            // renderer. Serialize UI work and admit that transfer before SR owns it.
            if (self->running || self->scene_bound) return RSF_ERROR_NOT_READY;
            char message[180]{}; std::snprintf(message, sizeof(message),
                "D3D11 host presentation owner transferred %u -> %lu on the same native chain before scene admission.",
                self->execution_thread, static_cast<unsigned long>(thread));
            Host::log(self, message); self->execution_thread = thread;
        }
        return RSF_OK;
    }
    auto* chain = static_cast<IDXGISwapChain*>(binding->swapchain);
    ComPtr<ID3D11Device> device;
    if (FAILED(chain->GetDevice(IID_PPV_ARGS(&device)))) return RSF_ERROR_INVALID_ARGUMENT;
    DXGI_SWAP_CHAIN_DESC desc{};
    if (FAILED(chain->GetDesc(&desc)) || !desc.OutputWindow) return RSF_ERROR_INVALID_ARGUMENT;
    ComPtr<ID3D11DeviceContext> context; device->GetImmediateContext(&context);
    rsf_game_render_pass lease{}; lease.swapchain = chain;
    if (!self->initialize(context.Get(), lease)) return RSF_ERROR_NOT_READY;
    Host::log(self, "D3D11 host presentation/window bound; overlay available before scene admission, SR SDK not started.");
    return RSF_OK;
}
catch (...) { return RSF_ERROR_NOT_READY; }
extern "C" RSF_RUNTIME_API int32_t rsf_d3d11_host_upscale_auxiliaries(void* user, void* context,
    const rsf_d3d11_auxiliary_pass* request) try
{
    auto* self = owner.load(std::memory_order_acquire);
    if (!self || user != self || !context || !request || request->struct_size < sizeof(*request)) return RSF_ERROR_INVALID_ARGUMENT;
    if (request->abi_version != RSF_D3D11_HOST_ABI_VERSION) return RSF_ERROR_ABI_MISMATCH;
    if (!request->pass || request->pass->struct_size < sizeof(*request->pass) || request->pass->role != RSF_GAME_RENDER_SR ||
        !request->pass->camera_valid || !request->pass->swapchain || (request->pass->flags & RSF_GAME_RENDER_RECONFIGURE) ||
        !request->surface_count || request->surface_count > 8 || !request->surfaces || request->pass->session_id != self->upstream.session_id)
        return RSF_ERROR_INVALID_ARGUMENT;
    for (uint32_t i = 0; i < request->surface_count; ++i)
        if (request->surfaces[i].struct_size < sizeof(rsf_d3d11_auxiliary_surface)) return RSF_ERROR_INVALID_ARGUMENT;
    return self->process(static_cast<ID3D11DeviceContext*>(context), *request->pass, request);
}
catch (...) { return RSF_ERROR_NOT_READY; }
extern "C" RSF_RUNTIME_API int32_t rsf_d3d11_host_restore_auxiliaries(void* user, const rsf_d3d11_auxiliary_scope* scope) try
{
    auto* self = owner.load(std::memory_order_acquire);
    if (!self || user != self || !scope || scope->struct_size < sizeof(*scope)) return RSF_ERROR_INVALID_ARGUMENT;
    if (scope->abi_version != RSF_D3D11_HOST_ABI_VERSION) return RSF_ERROR_ABI_MISMATCH;
    std::unique_lock presentation(self->presentation_guard, std::try_to_lock);
    if (!presentation.owns_lock()) return RSF_ERROR_BUSY;
    if (!self->auxiliaries || !self->auxiliaries->matches(*scope)) return RSF_ERROR_NOT_READY;
    return self->auxiliaries->restore() ? RSF_OK : RSF_ERROR_NOT_READY;
}
catch (...) { return RSF_ERROR_NOT_READY; }
extern "C" RSF_RUNTIME_API int32_t rsf_d3d11_host_bind_sampler_bias(void* user, void* raw_context,
    rsf_d3d11_sampler_bias* request) try
{
    auto* self = owner.load(std::memory_order_acquire);
    if (!self || user != self || !request || request->struct_size < sizeof(*request)) return RSF_ERROR_INVALID_ARGUMENT;
    if (request->abi_version != RSF_D3D11_HOST_ABI_VERSION) return RSF_ERROR_ABI_MISMATCH;
    std::unique_lock presentation(self->presentation_guard, std::try_to_lock);
    if (!presentation.owns_lock()) return RSF_ERROR_BUSY;
    if (!self->scene_bound || self->execution_thread != GetCurrentThreadId() ||
        request->session_id != self->upstream.session_id || (!self->active && request->slot_mask)) return RSF_ERROR_NOT_READY;
    return self->sampler_bias.apply(static_cast<ID3D11DeviceContext*>(raw_context), *request) ? RSF_OK : RSF_ERROR_NOT_READY;
}
catch (...) { return RSF_ERROR_NOT_READY; }
extern "C" RSF_RUNTIME_API int32_t rsf_d3d11_host_capture_texture(void* user, void* raw_context,
    const rsf_d3d11_host_capture* request) try
{
    auto* self = owner.load(std::memory_order_acquire);
    if (!self || user != self || !raw_context || !request || request->struct_size < sizeof(*request))
        return RSF_ERROR_INVALID_ARGUMENT;
    if (request->abi_version != RSF_D3D11_HOST_ABI_VERSION) return RSF_ERROR_ABI_MISMATCH;
    if (!request->texture || !request->name || !request->description || request->view > RSF_DUMP_VIEW_DENSE_MOTION ||
        !std::isfinite(request->scale) || request->scale < 0) return RSF_ERROR_INVALID_ARGUMENT;
    std::unique_lock presentation(self->presentation_guard, std::try_to_lock);
    if (!presentation.owns_lock()) return RSF_ERROR_BUSY;
    // Do not touch borrowed graphics pointers before verifying this exact capture lease.
    if (!self->active.load(std::memory_order_acquire) || !self->configured ||
        self->execution_thread != GetCurrentThreadId() || !self->capture_pending ||
        request->session_id != self->capture_record.session_id || request->native_frame != self->capture_native_frame ||
        request->view_key != self->capture_view || !rsf_input_dump_wants(&self->capture_record)) return RSF_ERROR_NOT_READY;
    auto* context = static_cast<ID3D11DeviceContext*>(raw_context);
    if (context->GetType() != D3D11_DEVICE_CONTEXT_IMMEDIATE) return RSF_ERROR_INVALID_ARGUMENT;
    ComPtr<ID3D11Device> device; context->GetDevice(&device);
    if (device.Get() != self->device.Get()) return RSF_ERROR_INVALID_ARGUMENT;
    return rsf_input_dump_texture_record(context, &self->capture_record, request->name, request->description,
        request->texture, request->view, request->scale) ? RSF_OK : RSF_ERROR_NOT_READY;
}
catch (...) { return RSF_ERROR_NOT_READY; }
