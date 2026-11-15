// SPDX-License-Identifier: GPL-3.0-only
#include <rescaleframe/d3d11_host.h>
#include <rescaleframe/native_sr.h>
#include <rescaleframe/native_cpu.h>
#include <rescaleframe/dlss_pipeline.h>
#include <rescaleframe/overlay_api.h>
#include <rescaleframe/gpu_policy.h>
#include <rescaleframe/input_dump.h>
#include <rescaleframe/d3d11_observer.h>
#include <rescaleframe/sr_session.h>
#include <rescaleframe/frame_generation.h>
#include "render_settings_store.h"
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
    rsf_game_host_services upstream{};
    std::wstring ini, section;
    std::string root, identity, fingerprint, version, project;
    std::string streamline, fidelityfx, xess, captures;
    rsf_d3d11_host_setup setup{};
    rsf_render_settings desired{sizeof(rsf_render_settings), RSF_RENDER_FRONTEND_ABI_VERSION};
    rsf_overlay_api overlay{sizeof(rsf_overlay_api), RSF_OVERLAY_API_VERSION};
    rsf_overlay_sr_report report{sizeof(rsf_overlay_sr_report)};
    ComPtr<IDXGISwapChain> chain;
    ComPtr<ID3D11Device> device;
    rsf_frame_record capture_record{};
    bool capture_pending{};
    uint32_t execution_thread{}, backend{}, quality{}, preset{};
    uint32_t output_width{}, output_height{};
    bool configured{}, observer{}, attempted{}, running{}, reset{true};
    std::atomic<bool> capture{};
    std::atomic<bool> active{};
    std::atomic<bool> transport{true}, fidelity{false};
    std::atomic<uint32_t> requested_generation{1};
    uint32_t applied_generation{};
    uint64_t evaluated{}, refused{}, logged{};
    int32_t last_result{RSF_ERROR_NOT_READY};
    int32_t present_result{RSF_DLSS_OK};
    uint64_t present_pairs{};
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
        if (self.upstream.cpu_event) self.upstream.cpu_event(self.upstream.user, event);
        else (void)rsf_native_cpu_event(event);
    }
    static int config(void* user, rsf_game_render_config* out) {
        if (!out || out->struct_size < sizeof(*out)) return 0;
        auto& self = *static_cast<Host*>(user); std::lock_guard lock(self.state);
        *out = {sizeof(*out), self.desired.sr_enabled, self.report.output_width, self.report.output_height,
            self.report.render_width, self.report.render_height}; return 1;
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
        if (!self.active.load(std::memory_order_acquire) || !event || (event->flags & DXGI_PRESENT_TEST) ||
            GetCurrentThreadId() != self.execution_thread || event->swapchain != self.chain.Get()) return;
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
        (void)self.overlay.present(event->swapchain, nullptr);
    }
    bool initialize(ID3D11DeviceContext* context, const rsf_game_render_pass& pass) {
        if (configured) return true;
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
        panel.latency_choices = 1u << RSF_REFLEX_OFF; panel.max_generated_frames = 1;
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
        observer = rsf_observer_install(&observe) == RSF_OBSERVER_OK;
        if (!observer) log(this, "D3D11 callback host: Present observation refused; SR activation suspended.");
        rsf_input_dump_configure(captures.c_str(), identity.c_str(), fingerprint.c_str(), log, this);
        configured = true;
        log(this, "D3D11 callback host configured; generation/pacing disabled until engine ownership is established.");
        return observer;
    }
    void capture_inputs(void* context, const rsf_game_render_pass& pass, rsf_frame_record& record) {
        record.struct_size = sizeof(record); record.abi_version = RSF_GAME_FRAME_ABI_VERSION;
        record.frame_id = pass.native_frame; record.session_id = pass.session_id; record.view_id = uint32_t(pass.view_key);
        record.flags = RSF_FRAME_FLAG_NO_FG | RSF_FRAME_FLAG_AMBIGUOUS_ID;
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
    int32_t process(ID3D11DeviceContext* context, const rsf_game_render_pass& pass) {
        if (!active.load(std::memory_order_acquire)) return RSF_ERROR_NOT_READY;
        if (!initialize(context, pass) || !observer || execution_thread != GetCurrentThreadId()) return RSF_ERROR_NOT_READY;
        ComPtr<ID3D11Device> current; context->GetDevice(&current);
        if (current.Get() != device.Get() || pass.swapchain != chain.Get() || context->GetType() != D3D11_DEVICE_CONTEXT_IMMEDIATE)
            return RSF_ERROR_NOT_READY;
        rsf_render_settings settings{}; { std::lock_guard lock(state); settings = desired; }
        const auto generation = requested_generation.load();
        if (generation != applied_generation) { attempted = false; reset = true; applied_generation = generation; }
        rsf_dlss_pipeline_set_colour_transport(transport.load(std::memory_order_acquire));
        rsf_dlss_pipeline_set_colour_correction(fidelity.load(std::memory_order_acquire));
        rsf_frame_record record{}; capture_inputs(context, pass, record);
        int32_t result = RSF_ERROR_NOT_READY;
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
                auto evaluated_pass = pass;
                if (reset) evaluated_pass.flags |= RSF_GAME_RENDER_RESET;
                result = rsf_native_sr_evaluate(context, &evaluated_pass);
                if (!result) { reset = false; ++evaluated; } else { reset = true; ++refused; }
            } else { result = running ? present_result : last_result; reset = true; }
        } else reset = true;
        if (rsf_input_dump_wants(&record)) {
            if (!result) (void)rsf_input_dump_texture_record(context, &record, "sr_reinserted", "Reconstructed linear colour before native HDR", pass.color_output, RSF_DUMP_VIEW_HDR, 1);
            rsf_input_dump_result(&record, "sr", backend, result);
            rsf_input_dump_pass(context, &pass, 0);
            capture_record = record; capture_pending = true;
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
            if (result) std::snprintf(report.reason, sizeof(report.reason), "SR result %d; native temporal processing retained", result);
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
    if (!context || !pass || pass->struct_size < sizeof(*pass) || !pass->camera_valid || !pass->swapchain) return RSF_ERROR_INVALID_ARGUMENT;
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
        services->render_pass || services->render_config || services->process_upscale || services->present || services->scope_state ||
        services->submit || services->process_latency || services->render_feedback) return RSF_ERROR_INVALID_ARGUMENT;
    std::lock_guard lock(creation); if (owner.load()) return RSF_ERROR_BUSY;
    auto self = std::make_unique<Host>(); self->setup = *setup; self->upstream = *services;
    self->ini = setup->configuration_path; self->section = setup->configuration_section; self->root = utf8(setup->runtime_directory);
    self->identity = setup->identity_utf8; self->fingerprint = setup->executable_sha256;
    self->version = setup->engine_version_utf8; self->project = setup->project_id_utf8;
    // Only numeric configuration is read from setup after this call. Keep no borrowed strings.
    self->setup.configuration_path = self->setup.configuration_section = self->setup.runtime_directory = nullptr;
    self->setup.identity_utf8 = self->setup.executable_sha256 = self->setup.engine_version_utf8 = self->setup.project_id_utf8 = nullptr;
    self->streamline = self->root + "/streamline"; self->fidelityfx = self->root + "/fidelityfx";
    self->xess = self->root + "/xess"; self->captures = self->root + "/captures";
    *services = {sizeof(*services), RSF_GAME_ABI_VERSION, self->upstream.session_id, Host::log, nullptr, self.get(), Host::config, Host::cpu};
    services->process_upscale = process; services->render_feedback = Host::feedback;
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
