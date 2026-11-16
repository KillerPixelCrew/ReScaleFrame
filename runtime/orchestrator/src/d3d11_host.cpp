// SPDX-License-Identifier: MIT
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
#include "native_sr_completion.h"
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
#include <cstring>
#include <cmath>
#include <array>
#include <cstddef>

namespace {
using Microsoft::WRL::ComPtr;
bool same_object(IUnknown* left, IUnknown* right) {
    if (!left || !right) return false;
    ComPtr<IUnknown> a,b;
    return SUCCEEDED(left->QueryInterface(IID_PPV_ARGS(&a))) &&
        SUCCEEDED(right->QueryInterface(IID_PPV_ARGS(&b))) && a.Get() == b.Get();
}
std::string utf8(const std::wstring& value) {
    const auto size = WideCharToMultiByte(CP_UTF8, 0, value.data(), int(value.size()), nullptr, 0, nullptr, nullptr);
    std::string result(size_t(size), '\0');
    if (size) WideCharToMultiByte(CP_UTF8, 0, value.data(), int(value.size()), result.data(), size, nullptr, nullptr);
    return result;
}
static_assert(sizeof(rsf_d3d11_reduced_frame) == 56);
static_assert(offsetof(rsf_d3d11_reduced_frame, token) == 16);
static_assert(offsetof(rsf_d3d11_reduced_frame, owner) == 24);
static_assert(offsetof(rsf_d3d11_reduced_frame, surfaces) == 40);
static_assert(offsetof(rsf_d3d11_reduced_frame, temporal_result) == 48);
struct ReducedFrame {
    AuxiliarySurfaces products;
    rsf_game_render_pass owner{};
    std::array<rsf_d3d11_auxiliary_surface, 10> surfaces{};
    std::array<ComPtr<ID3D11Texture2D>, 11> leases;
    ComPtr<ID3D11DeviceContext> context;
    ComPtr<IDXGISwapChain> chain;
    uint64_t token{};
    uint32_t generation{}, count{}, flags{}, disposition{};
    int32_t temporal_result{RSF_ERROR_NOT_READY};
    bool pending{}, attempted{}, ordered{true}, temporal{}, sealed{};
    bool matches(const rsf_game_render_pass& p, bool crop = true) const {
        const auto& a = owner;
        return p.role == a.role && p.session_id == a.session_id && p.source_frame_id == a.source_frame_id &&
            p.submission_id == a.submission_id && p.native_frame == a.native_frame && p.view_key == a.view_key &&
            p.family_key == a.family_key && p.scope_id == a.scope_id && p.pass_key == a.pass_key &&
            p.viewport_key == a.viewport_key && p.window_key == a.window_key && p.rhi_viewport_key == a.rhi_viewport_key &&
            p.resource_generation == a.resource_generation && p.swapchain == a.swapchain &&
            p.color_input == a.color_input && p.color_output == a.color_output &&
            p.color_output_readable == a.color_output_readable &&
            !std::memcmp(p.output_rect,a.output_rect,sizeof(a.output_rect)) &&
            (!crop || !std::memcmp(p.render_rect,a.render_rect,sizeof(a.render_rect)));
    }
    bool temporal_surfaces(const rsf_d3d11_auxiliary_pass* request) const {
        if (!count) return !request || !request->surface_count;
        if (!request || request->surface_count != count || !request->surfaces) return false;
        for (uint32_t i = 0; i < count; ++i) {
            const auto& p = request->surfaces[i]; const auto& a = surfaces[i];
            if (p.struct_size < sizeof(p) || p.texture != a.texture || p.kind != a.kind || p.flags != a.flags ||
                std::memcmp(p.output_rect,a.output_rect,sizeof(a.output_rect))) return false;
        }
        return true;
    }
    void output(rsf_d3d11_reduced_frame& r) const {
        r.token = token; r.disposition = disposition; r.result_flags = flags; r.temporal_result = temporal_result;
    }
    void release() {
        for (auto& resource : leases) resource.Reset();
        pending = false; // Keep exact context/chain identity for idempotent acknowledgement.
    }
};
struct Host {
    std::mutex state;
    std::mutex presentation_guard;
    std::unique_ptr<AuxiliarySurfaces> auxiliaries;
    std::unique_ptr<ReducedFrame> reduced;
    uint64_t next_reduced_token{1};
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
    ComPtr<IDXGISwapChain> creation_outer;
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
    uint32_t ownership_reports{}, capture_refusal_terms{};
    std::atomic<uint32_t> cpu_reports{}, cpu_sample_reports{}, cpu_refusal_stages{};
    std::atomic<uint32_t> cpu_stage_reports[RSF_GAME_CPU_SOURCE_ABORT + 1]{};
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
            // Keep startup receipts, plus bounded late stage samples and the first refusal
            // of each stage. Read no extension fields until its declared size/version allows it.
            const bool source_event = event && event->struct_size >= sizeof(*event) &&
                event->stage >= RSF_GAME_CPU_SOURCE_RESERVE && event->stage <= RSF_GAME_CPU_SOURCE_ABORT;
            const bool qualified = event && event->struct_size >= sizeof(*event) &&
                event->stage <= RSF_GAME_CPU_SOURCE_ABORT &&
                (source_event ? event->event_version == RSF_GAME_CPU_EVENT_VERSION_SOURCE :
                    event->event_version <= RSF_GAME_CPU_EVENT_VERSION);
            if (qualified) {
                const bool initial = self.cpu_reports.fetch_add(1) < 24;
                const uint32_t stage_bit = 1u << event->stage;
                const bool first_refusal = !accepted && !(self.cpu_refusal_stages.fetch_or(stage_bit) & stage_bit);
                const bool first_stage = self.cpu_stage_reports[event->stage].fetch_add(1) < 4;
                // Reserve eleven of the 64 late rows for independent first-stage refusals.
                const bool sampled = !initial && !first_refusal &&
                    (first_stage || (event->source_frame_id && event->source_frame_id % 128 == 0)) &&
                    self.cpu_sample_reports.fetch_add(1) < 53;
                if (initial || first_refusal || sampled) {
                    uint64_t native_generation{}, poll_begin{};
                    uint32_t abort_reason{};
                    if (source_event && event->struct_size >= RSF_GAME_CPU_SOURCE_EVENT_SIZE_1) {
                        rsf_game_cpu_source_event source{};
                        std::memcpy(&source,event,RSF_GAME_CPU_SOURCE_EVENT_SIZE_1);
                        native_generation = source.native_generation; abort_reason = source.reason;
                        if (event->struct_size >= sizeof(source)) {
                            std::memcpy(&source,event,sizeof(source)); poll_begin = source.poll_begin_qpc;
                        }
                    }
                    char text[320]{}; std::snprintf(text, sizeof(text),
                        "D3D11 CPU fact session=%llu source=%llu stage=%u accepted=%d flags=0x%x version=%u size=%u native_generation=%llu poll_begin=%llu abort_reason=%u",
                        static_cast<unsigned long long>(event->session_id), static_cast<unsigned long long>(event->source_frame_id),
                        event->stage, accepted, event->flags, event->event_version, event->struct_size,
                        static_cast<unsigned long long>(native_generation), static_cast<unsigned long long>(poll_begin), abort_reason);
                    log(&self, text);
                }
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
            setup.presentation_bridge == 3 ? rsf_d3d11_present_install_scoped(&bridge) : rsf_d3d11_present_install(&bridge)) != 0;
        if (bridge_installed) {
            rsf_native_fg_set_log(log, this);
            const rsf_native_fg_options off{sizeof(off), RSF_FG_OFF, 1, RSF_REFLEX_OFF, 0, 0};
            rsf_native_fg_options_set(&off);
        }
        log(this, bridge_installed ? (setup.presentation_bridge == 3 ?
            "D3D11 callback host installed scoped creation hooks; outer canonical ownership awaits native return." : setup.presentation_bridge == 2 ?
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
                if (same_object(device.Get(),self.device.Get()) && context->GetType() == D3D11_DEVICE_CONTEXT_IMMEDIATE)
                    (void)rsf_native_sr_capture_fg(context, pass, &self.setup.motion);
            }
        }
        else if (pass->role == RSF_GAME_RENDER_FINAL_SCENE) {
            if (!raw_context || !pass->scene_surface) return;
            auto* context = static_cast<ID3D11DeviceContext*>(raw_context);
            if (context->GetType() != D3D11_DEVICE_CONTEXT_IMMEDIATE) return;
            ComPtr<ID3D11Device> device; context->GetDevice(&device);
            if (!same_object(device.Get(),self.device.Get())) return;
            static_cast<ID3D11Texture2D*>(pass->scene_surface)->GetDevice(&device);
            if (!same_object(device.Get(),self.device.Get())) return;
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
        uint32_t event_thread{};
        if (!self.active.load(std::memory_order_acquire) || !event || (event->flags & DXGI_PRESENT_TEST) ||
            (event_thread = GetCurrentThreadId()) != self.execution_thread ||
            (self.bridge_installed && !rsf_d3d11_present_is_owner(event->swapchain)) ||
            !same_object(static_cast<IUnknown*>(event->swapchain),self.chain.Get())) return;
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
                uint32_t reached{}, passed{}, scene_path = UINT32_MAX;
                const auto term = [&](uint32_t bit, bool ok) { reached |= bit; if (ok) passed |= bit; return ok; };
                // Preserve the original short circuit: no WINDOW or scene query is added.
                if (!term(1,rsf_native_window_match(event->swapchain, &window) != 0) ||
                    !term(2,window.session_id == self.capture_record.session_id) ||
                    !term(4,window.source_frame_id == self.capture_record.frame_id) ||
                    !term(8,window.native_frame == self.capture_native_frame) || !term(16,window.view_key == self.capture_view) ||
                    !term(32,(scene_path = rsf_native_scene_present_path(&window)) != RSF_NATIVE_SCENE_PRESENT_NONE)) {
                    const auto failed = reached & ~passed;
                    // At most one receipt for each of the six first failing terms.
                    if (!(self.capture_refusal_terms & failed)) {
                        self.capture_refusal_terms |= failed;
                        char text[720]{}; std::snprintf(text,sizeof(text),
                            "D3D11 capture Present refused reached=0x%x passed=0x%x failed=0x%x capture_session=%llu source=%llu native=%llu view=%llu window_session=%llu source=%llu native=%llu view=%llu completed=%u thread=%u scene_path=%u",
                            reached,passed,failed,static_cast<unsigned long long>(self.capture_record.session_id),
                            static_cast<unsigned long long>(self.capture_record.frame_id),static_cast<unsigned long long>(self.capture_native_frame),
                            static_cast<unsigned long long>(self.capture_view),static_cast<unsigned long long>(window.session_id),
                            static_cast<unsigned long long>(window.source_frame_id),static_cast<unsigned long long>(window.native_frame),
                            static_cast<unsigned long long>(window.view_key),event->completed,event_thread,scene_path);
                        log(&self,text);
                    }
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
        transport.store(GetPrivateProfileIntW(section.c_str(), L"ColourTransport", setup.colour_transport_default ? 0 : 1, ini.c_str()) != 0);
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
    void capture_inputs(void* context, const rsf_game_render_pass& pass, rsf_frame_record& record, void* original_colour = nullptr) {
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
        (void)rsf_input_dump_texture_record(context, &record, "native_color", "Native pre-tonemap scene colour", original_colour ? original_colour : pass.color_input, RSF_DUMP_VIEW_HDR, 1);
        if (pass.temporal_mask)
            (void)rsf_input_dump_texture_record(context, &record, "native_temporal_mask",
                "Native numeric temporal rejection mask before weighted normalization", pass.temporal_mask, RSF_DUMP_VIEW_RAW, 1);
        (void)rsf_input_dump_texture_record(context, &record, "native_depth", "Native scene device depth", pass.depth, RSF_DUMP_VIEW_DEPTH, 1);
        (void)rsf_input_dump_texture_record(context, &record, "native_motion", "Native motion in adapter-declared encoding", pass.motion, RSF_DUMP_VIEW_DENSE_MOTION, 8);
    }
    bool prepare_pipeline(const rsf_game_render_pass& pass, const rsf_render_settings& settings) {
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
        return ready;
    }
    int32_t process(ID3D11DeviceContext* context, const rsf_game_render_pass& pass,
        const rsf_d3d11_auxiliary_pass* auxiliary = nullptr) {
        std::unique_lock presentation(presentation_guard, std::try_to_lock);
        if (!presentation.owns_lock()) return RSF_ERROR_BUSY;
        if (!active.load(std::memory_order_acquire)) return RSF_ERROR_NOT_READY;
        const bool recovery = reduced && reduced->pending;
        if (recovery && (!reduced->sealed || !reduced->matches(pass) ||
            (pass.flags & RSF_GAME_RENDER_RECONFIGURE) || reduced->attempted)) return RSF_ERROR_BUSY;
        if (auxiliaries && auxiliaries->pending()) return RSF_ERROR_BUSY;
        if (recovery && (!reduced->temporal_surfaces(auxiliary) ||
            reduced->context.Get() != context || reduced->generation != requested_generation.load(std::memory_order_acquire))) return RSF_ERROR_NOT_READY;
        // Bind ownership from a verified scene lease, never from an early loading callback.
        if ((pass.flags & RSF_GAME_RENDER_RECONFIGURE) && !scene_bound) return RSF_ERROR_NOT_READY;
        if (!initialize(context, pass) || !observer || execution_thread != GetCurrentThreadId()) return RSF_ERROR_NOT_READY;
        ComPtr<ID3D11Device> current; context->GetDevice(&current);
        if (!same_object(current.Get(),device.Get()) || pass.swapchain != chain.Get() || context->GetType() != D3D11_DEVICE_CONTEXT_IMMEDIATE)
            return RSF_ERROR_NOT_READY;
        // Reject foreign temporal resources before auxiliary preparation or capture.
        void* borrowed[]{pass.color_input,pass.color_output,pass.color_output_readable,pass.depth,pass.motion,pass.exposure,pass.temporal_mask};
        for (void* texture : borrowed) if (texture) {
            ComPtr<ID3D11Device> resource_device;
            static_cast<ID3D11Texture2D*>(texture)->GetDevice(&resource_device);
            if (!same_object(resource_device.Get(),device.Get())) return RSF_ERROR_INVALID_ARGUMENT;
        }
        scene_bound = true;
        rsf_render_settings settings{}; { std::lock_guard lock(state); settings = desired; }
        const auto generation = requested_generation.load();
        const bool unchanged_configuration = generation == applied_generation && running &&
            settings.sr_enabled && backend == settings.sr_backend && quality == settings.sr_quality &&
            preset == settings.dlss_preset && output_width == pass.camera.output_width &&
            output_height == pass.camera.output_height;
        if (!recovery && generation != applied_generation) { attempted = false; reset = true; applied_generation = generation; }
        if (!recovery) {
            rsf_dlss_pipeline_set_colour_transport(transport.load(std::memory_order_acquire));
            rsf_dlss_pipeline_set_colour_correction(fidelity.load(std::memory_order_acquire));
        }
        const bool pipeline_ready = recovery ? running : prepare_pipeline(pass, settings);
        const bool configure_only = (pass.flags & RSF_GAME_RENDER_RECONFIGURE) != 0;
        // Planning returns no output. An unchanged successful plan also has no new
        // evaluation result to publish, including when the last evaluation failed.
        if (configure_only && pipeline_ready && unchanged_configuration) return RSF_ERROR_NOT_READY;
        rsf_frame_record record{};
        // A capture request belongs to an actual render lease, never its size plan.
        if (!configure_only) capture_inputs(context, pass, record);
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
                const bool prepared = recovery ? (!auxiliary || reduced->products.temporal_prepare(*auxiliary)) :
                    (!auxiliary || auxiliaries->prepare(context, evaluated_pass, *auxiliary, log, this));
                if (recovery) { reduced->attempted = true; reduced->flags |= RSF_D3D11_REDUCED_TEMPORAL_ATTEMPTED; }
                result = prepared ? rsf_native_sr_evaluate(context, &evaluated_pass) : RSF_ERROR_NOT_READY;
                if (recovery) {
                    reduced->ordered = !prepared || rsf_native_sr_native_destinations_ordered();
                    if (!result && !reduced->products.commit(true)) {
                        result = RSF_ERROR_NOT_READY;
                        // A partial native commit remains an owned cleanup obligation.
                        reduced->ordered = false;
                    }
                    reduced->temporal_result = result; reduced->temporal = result == RSF_OK;
                } else {
                    if (!result && auxiliary && !auxiliaries->commit()) result = RSF_ERROR_NOT_READY;
                    if (result && auxiliary && !auxiliaries->restore()) result = RSF_ERROR_BUSY;
                }
                if (!result) { reset = false; ++evaluated; } else { reset = true; ++refused; }
            } else {
                result = configure_only ? RSF_ERROR_NOT_READY : running ? present_result : last_result;
                // An unchanged, successful plan does not evaluate or invalidate temporal history.
                // Genuine reconfiguration keeps its pending reset; failed preparation still resets.
                if (!configure_only || !pipeline_ready) reset = true;
            }
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
        setup->colour_transport_default > 1 || setup->presentation_bridge > 3 || (setup->presentation_bridge && !setup->accept_window) ||
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
extern "C" RSF_RUNTIME_API int32_t rsf_d3d11_host_dispatch_present(void* user,
    rsf_d3d11_present_dispatch* request) {
    auto* self = owner.load(std::memory_order_acquire);
    if (!self || user != self || !request || request->struct_size < sizeof(*request) || !request->receiver)
        return RSF_ERROR_INVALID_ARGUMENT;
    if (request->abi_version != RSF_D3D11_HOST_ABI_VERSION) return RSF_ERROR_ABI_MISMATCH;
    rsf_d3d11_chain_dispatch call{}; call.struct_size = sizeof(call); call.abi_version = request->abi_version;
    call.method = RSF_D3D11_DISPATCH_PRESENT; call.receiver = request->receiver;
    call.args.present = {request->sync_interval,request->flags};
    const auto status = rsf_d3d11_host_dispatch_chain(user,&call);
    if (status != RSF_OK) return status;
    request->handled = call.handled;
    if (call.handled) request->native_result = call.result.hresult;
    return RSF_OK;
}
extern "C" RSF_RUNTIME_API int32_t rsf_d3d11_host_dispatch_chain(void* user, rsf_d3d11_chain_dispatch* request) {
    auto* self = owner.load(std::memory_order_acquire);
    if (!self || user != self || !request || request->struct_size < sizeof(*request) || !request->receiver)
        return RSF_ERROR_INVALID_ARGUMENT;
    if (request->abi_version != RSF_D3D11_HOST_ABI_VERSION) return RSF_ERROR_ABI_MISMATCH;
    if (request->method < RSF_D3D11_DISPATCH_RELEASE || request->method > RSF_D3D11_DISPATCH_RESIZE_BUFFERS1)
        return RSF_ERROR_INVALID_ARGUMENT;
    request->handled = 0;
    if (self->bridge_installed) rsf_d3d11_chain_dispatch_owned(request);
    return RSF_OK;
}
extern "C" RSF_RUNTIME_API int32_t rsf_d3d11_host_creation_begin(void* user, rsf_d3d11_creation_scope* scope) try {
    auto* self = owner.load(std::memory_order_acquire);
    if (!self || user != self || !scope || scope->struct_size < sizeof(*scope)) return RSF_ERROR_INVALID_ARGUMENT;
    if (scope->abi_version != RSF_D3D11_HOST_ABI_VERSION) return RSF_ERROR_ABI_MISMATCH;
    if (scope->token || !scope->window) return RSF_ERROR_INVALID_ARGUMENT;
    std::unique_lock lock(self->presentation_guard, std::try_to_lock);
    if (!lock.owns_lock()) return RSF_ERROR_BUSY;
    if (self->setup.presentation_bridge != 3 || !self->bridge_installed || self->configured || self->creation_outer ||
        !self->setup.accept_window(self->setup.presentation_user,scope->window)) return RSF_ERROR_NOT_READY;
    return SUCCEEDED(rsf_d3d11_present_creation_begin(scope->window,&scope->token)) ? RSF_OK : RSF_ERROR_NOT_READY;
} catch (...) { return RSF_ERROR_NOT_READY; }
extern "C" RSF_RUNTIME_API int32_t rsf_d3d11_host_creation_end(void* user, const rsf_d3d11_creation_scope* scope,
    const rsf_d3d11_native_chain* result) try {
    auto* self = owner.load(std::memory_order_acquire);
    if (!self || user != self || !scope || scope->struct_size < sizeof(*scope)) return RSF_ERROR_INVALID_ARGUMENT;
    if (scope->abi_version != RSF_D3D11_HOST_ABI_VERSION) return RSF_ERROR_ABI_MISMATCH;
    const auto result_error = result && result->struct_size < sizeof(*result) ? RSF_ERROR_INVALID_ARGUMENT :
        result && result->abi_version != RSF_D3D11_HOST_ABI_VERSION ? RSF_ERROR_ABI_MISMATCH : RSF_OK;
    if (self->setup.presentation_bridge != 3 || !self->bridge_installed) return RSF_ERROR_NOT_READY;
    // End must always close the owning transaction, even if another callback is busy.
    std::lock_guard lock(self->presentation_guard);
    if (result_error != RSF_OK) {
        (void)rsf_d3d11_present_creation_end(scope->window,scope->token,nullptr,nullptr,nullptr);
        return result_error;
    }
    auto* outer = result && result->swapchain ? static_cast<IDXGISwapChain*>(*result->swapchain) : nullptr;
    const auto admitted = rsf_d3d11_present_creation_end(scope->window,scope->token,
        result ? result->device : nullptr,result ? result->immediate_context : nullptr,outer);
    if (FAILED(admitted)) return RSF_ERROR_NATIVE_REFUSED;
    self->creation_outer = outer; // Keep the explicitly verified alias alive; never replace it.
    return RSF_OK;
} catch (...) { return RSF_ERROR_NOT_READY; }
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
    // Drop our diagnostic identity reference before observing the caller's
    // release. A remaining count belongs to another owner, not this temporary.
    native_owner.Reset();
    const auto remaining = native->Release();
    char released[192]{}; std::snprintf(released, sizeof(released),
        "D3D11 outer native chain caller reference released identity=%p reported_remaining=%lu; no external references consumed",
        native_identity, static_cast<unsigned long>(remaining));
    Host::log(self, released);
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
extern "C" RSF_RUNTIME_API int32_t rsf_d3d11_host_reduced_frame(void* user, void* raw_context,
    rsf_d3d11_reduced_frame* request) try
{
    auto* self = owner.load(std::memory_order_acquire);
    if (!self || user != self || !request || request->struct_size < sizeof(*request)) return RSF_ERROR_INVALID_ARGUMENT;
    if (request->abi_version != RSF_D3D11_REDUCED_FRAME_ABI_VERSION) return RSF_ERROR_ABI_MISMATCH;
    if (request->operation > RSF_D3D11_REDUCED_CANCEL || request->reserved) return RSF_ERROR_INVALID_ARGUMENT;
    std::unique_lock presentation(self->presentation_guard, std::try_to_lock);
    if (!presentation.owns_lock()) return RSF_ERROR_BUSY;
    // Session/thread/token guards precede all borrowed graphics dereferences.
    if (!self->configured || self->execution_thread != GetCurrentThreadId() || !raw_context ||
        !request->owner || request->owner->struct_size < sizeof(*request->owner) ||
        request->owner->session_id != self->upstream.session_id) return RSF_ERROR_NOT_READY;
    request->disposition = RSF_D3D11_REDUCED_NONE; request->result_flags = 0;
    request->temporal_result = RSF_ERROR_NOT_READY;
    const auto& pass = *request->owner;
    const bool reserve = request->operation == RSF_D3D11_REDUCED_RESERVE;
    if (!reserve) {
        if (!self->reduced || !request->token || request->token != self->reduced->token ||
            raw_context != self->reduced->context.Get() ||
            !self->reduced->matches(pass, request->operation != RSF_D3D11_REDUCED_SEAL)) return RSF_ERROR_NOT_READY;
    } else if (request->token || (self->reduced && self->reduced->pending) ||
        (self->auxiliaries && self->auxiliaries->pending())) return RSF_ERROR_BUSY;
    auto* context = static_cast<ID3D11DeviceContext*>(raw_context);
    if (reserve) {
        if (!self->active.load(std::memory_order_acquire) || !self->scene_bound || !self->running ||
            pass.role != RSF_GAME_RENDER_SR || !pass.source_frame_id || !pass.native_frame || !pass.view_key ||
            pass.swapchain != self->chain.Get() || !pass.color_input || !pass.color_output ||
            (pass.flags & RSF_GAME_RENDER_RECONFIGURE) || request->surface_count > 8 ||
            (request->surface_count && !request->surfaces) || !self->next_reduced_token) return RSF_ERROR_NOT_READY;
        rsf_game_render_config plan{};
        { std::lock_guard lock(self->state); plan = self->planned; }
        const auto generation = self->requested_generation.load(std::memory_order_acquire);
        if (!plan.enabled || generation != self->applied_generation ||
            pass.render_rect[0] < 0 || pass.render_rect[1] < 0 ||
            pass.render_rect[2] <= pass.render_rect[0] || pass.render_rect[3] <= pass.render_rect[1] ||
            uint32_t(pass.render_rect[2]-pass.render_rect[0]) != plan.render_width ||
            uint32_t(pass.render_rect[3]-pass.render_rect[1]) != plan.render_height ||
            pass.output_rect[0] < 0 || pass.output_rect[1] < 0 ||
            pass.output_rect[2] <= pass.output_rect[0] || pass.output_rect[3] <= pass.output_rect[1] ||
            uint32_t(pass.output_rect[2]-pass.output_rect[0]) != plan.output_width ||
            uint32_t(pass.output_rect[3]-pass.output_rect[1]) != plan.output_height) return RSF_ERROR_NOT_READY;
        ComPtr<ID3D11Device> device; context->GetDevice(&device);
        ComPtr<ID3D11DeviceContext> immediate; self->device->GetImmediateContext(&immediate);
        if (context != immediate.Get() || !same_object(device.Get(),self->device.Get()) ||
            context->GetType() != D3D11_DEVICE_CONTEXT_IMMEDIATE || FAILED(device->GetDeviceRemovedReason())) return RSF_ERROR_NOT_READY;
        auto* private_output = static_cast<IUnknown*>(rsf_dlss_pipeline_output_texture());
        if (private_output && (same_object(static_cast<IUnknown*>(pass.color_input),private_output) ||
            same_object(static_cast<IUnknown*>(pass.color_output),private_output) ||
            same_object(static_cast<IUnknown*>(pass.color_output_readable),private_output))) return RSF_ERROR_INVALID_ARGUMENT;
        if (!self->reduced) self->reduced = std::make_unique<ReducedFrame>();
        auto& ticket = *self->reduced;
        ticket.owner = pass; ticket.count = request->surface_count;
        // Only copied scalar identity and explicit resource leases survive the callback.
        ticket.owner.depth = ticket.owner.motion = ticket.owner.exposure = ticket.owner.temporal_mask = nullptr;
        ticket.owner.ui_input = ticket.owner.scene_surface = ticket.owner.sampled_texture = nullptr;
        ticket.owner.camera_valid = 0; ticket.owner.depth_domains = {};
        for (uint32_t i = 0; i < ticket.count; ++i) {
            const auto& a = request->surfaces[i];
            if (a.struct_size < sizeof(a) || a.kind > RSF_D3D11_AUXILIARY_COLOUR ||
                (a.flags & ~RSF_D3D11_AUXILIARY_RESTORE_CONTENTS) || !a.texture || a.texture == pass.color_input || a.texture == pass.color_output ||
                a.texture == pass.color_output_readable ||
                (private_output && same_object(static_cast<IUnknown*>(a.texture),private_output))) return RSF_ERROR_INVALID_ARGUMENT;
            ticket.surfaces[i] = a; ticket.surfaces[i].name = ticket.surfaces[i].source_name = nullptr;
        }
        uint32_t total = ticket.count;
        const auto append_main = [&](void* destination) {
            auto& a = ticket.surfaces[total++]; a = {}; a.struct_size = sizeof(a);
            a.kind = RSF_D3D11_AUXILIARY_COLOUR; a.texture = destination;
            for (unsigned j = 0; j < 4; ++j) { a.source_rect[j] = float(pass.render_rect[j]); a.output_rect[j] = pass.output_rect[j]; }
        };
        append_main(pass.color_output);
        if (pass.color_output_readable && pass.color_output_readable != pass.color_output) append_main(pass.color_output_readable);
        const rsf_d3d11_auxiliary_pass inputs{sizeof(inputs),RSF_D3D11_HOST_ABI_VERSION,&pass,total,ticket.surfaces.data()};
        if (!ticket.products.reserve(context,pass,inputs,Host::log,self,true)) return RSF_ERROR_NOT_READY;
        ticket.context = context; ticket.chain = self->chain; ticket.generation = generation;
        ticket.leases[0] = static_cast<ID3D11Texture2D*>(pass.color_input);
        for (uint32_t i = 0; i < total; ++i) ticket.leases[i+1] = static_cast<ID3D11Texture2D*>(ticket.surfaces[i].texture);
        ticket.token = self->next_reduced_token++; ticket.pending = true; ticket.sealed = ticket.attempted = ticket.temporal = false;
        ticket.ordered = true; ticket.flags = 0; ticket.temporal_result = RSF_ERROR_NOT_READY;
        ticket.disposition = RSF_D3D11_REDUCED_RESERVED; ticket.output(*request); return RSF_OK;
    }
    auto& ticket = *self->reduced;
    // A retired ticket remains an exact idempotent acknowledgement until next RESERVE.
    if (!ticket.pending) {
        ticket.output(*request);
        const bool repeated = request->operation == RSF_D3D11_REDUCED_RETIRE ||
            (request->operation == RSF_D3D11_REDUCED_FINISH && (ticket.flags & RSF_D3D11_REDUCED_FULL_OUTPUT_DEFINED)) ||
            (request->operation == RSF_D3D11_REDUCED_CANCEL && ticket.disposition == RSF_D3D11_REDUCED_CANCELLED);
        return repeated ? RSF_OK : RSF_ERROR_NOT_READY;
    }
    const auto incomplete = [&](uint32_t disposition) {
        ticket.disposition = disposition; ticket.flags |= RSF_D3D11_REDUCED_CLEANUP_REQUIRED;
        if (!(ticket.flags & RSF_D3D11_REDUCED_FULL_OUTPUT_DEFINED))
            ticket.flags |= RSF_D3D11_REDUCED_REQUIRED_RECOVERY_FAILED;
        {
            std::lock_guard lock(self->state);
            if (!(ticket.flags & RSF_D3D11_REDUCED_FULL_OUTPUT_DEFINED)) self->report.effective_backend = 0;
            std::snprintf(self->report.reason,sizeof(self->report.reason),"Reduced-frame cleanup pending; output_defined=%u",
                unsigned((ticket.flags & RSF_D3D11_REDUCED_FULL_OUTPUT_DEFINED) != 0));
        }
        ticket.output(*request); return RSF_ERROR_BUSY;
    };
    if (FAILED(self->device->GetDeviceRemovedReason())) return incomplete(RSF_D3D11_REDUCED_UNSAFE);
    if (request->operation == RSF_D3D11_REDUCED_CANCEL) {
        if (ticket.sealed || !ticket.products.cancel()) return RSF_ERROR_NOT_READY;
        ticket.disposition = RSF_D3D11_REDUCED_CANCELLED;
        ticket.flags = RSF_D3D11_REDUCED_NEXT_REDUCTION_ALLOWED; ticket.output(*request);
        ticket.release(); return RSF_OK;
    }
    if (request->operation == RSF_D3D11_REDUCED_SEAL) {
        if (ticket.sealed ||
            request->surface_count != ticket.count || (ticket.count && !request->surfaces)) return RSF_ERROR_NOT_READY;
        auto refined = ticket.surfaces;
        for (uint32_t i = 0; i < ticket.count; ++i) refined[i] = request->surfaces[i];
        uint32_t total = ticket.count + 1;
        if (pass.color_output_readable && pass.color_output_readable != pass.color_output) ++total;
        for (uint32_t i = ticket.count; i < total; ++i)
            for (unsigned j = 0; j < 4; ++j) refined[i].source_rect[j] = float(pass.render_rect[j]);
        const rsf_d3d11_auxiliary_pass inputs{sizeof(inputs),RSF_D3D11_HOST_ABI_VERSION,&pass,total,refined.data()};
        if (!ticket.products.refine(inputs)) return RSF_ERROR_INVALID_ARGUMENT;
        std::memcpy(ticket.owner.render_rect,pass.render_rect,sizeof(pass.render_rect));
        for (uint32_t i = 0; i < total; ++i) {
            ticket.surfaces[i] = refined[i]; ticket.surfaces[i].name = ticket.surfaces[i].source_name = nullptr;
        }
        // Failure after snapshot entry cannot be cancelled as an unused reservation.
        ticket.sealed = true;
        if (!ticket.products.seal()) {
            ticket.ordered = false; return incomplete(RSF_D3D11_REDUCED_UNSAFE);
        }
        ticket.disposition = RSF_D3D11_REDUCED_SEALED; ticket.output(*request); return RSF_OK;
    }
    if (!ticket.sealed) return RSF_ERROR_NOT_READY;
    if (request->operation == RSF_D3D11_REDUCED_FINISH) {
        if (ticket.flags & RSF_D3D11_REDUCED_FULL_OUTPUT_DEFINED) { ticket.output(*request); return RSF_OK; }
        if (!ticket.ordered || (self->auxiliaries && self->auxiliaries->pending())) {
            return incomplete(RSF_D3D11_REDUCED_UNSAFE);
        }
        if (!ticket.temporal && (!ticket.products.spatial_prepare() || !ticket.products.commit())) {
            ticket.ordered = false; return incomplete(RSF_D3D11_REDUCED_CLEANUP_PENDING);
        }
        ticket.flags |= RSF_D3D11_REDUCED_FULL_OUTPUT_DEFINED | RSF_D3D11_REDUCED_CLEANUP_REQUIRED |
            (ticket.temporal ? RSF_D3D11_REDUCED_TEMPORAL_OUTPUT : RSF_D3D11_REDUCED_SPATIAL_OUTPUT);
        ticket.disposition = ticket.temporal ? RSF_D3D11_REDUCED_TEMPORAL : RSF_D3D11_REDUCED_SPATIAL;
        if (!ticket.temporal) {
            self->reset = true;
            std::lock_guard lock(self->state); self->report.effective_backend = 0;
            self->report.last_result = ticket.temporal_result;
            self->report.inputs = RSF_OVERLAY_SR_INPUT_COLOR;
            self->report.render_width = uint32_t(pass.render_rect[2]-pass.render_rect[0]);
            self->report.render_height = uint32_t(pass.render_rect[3]-pass.render_rect[1]);
            self->report.output_width = uint32_t(pass.output_rect[2]-pass.output_rect[0]);
            self->report.output_height = uint32_t(pass.output_rect[3]-pass.output_rect[1]);
            std::snprintf(self->report.reason,sizeof(self->report.reason),"Spatial reduced-frame recovery; temporal result %d attempted=%u",
                ticket.temporal_result,unsigned(ticket.attempted));
        }
        if (!ticket.temporal) {
            rsf_frame_record record{};
            if (self->capture_pending && self->capture_native_frame == pass.native_frame &&
                self->capture_view == pass.view_key && self->capture_record.frame_id == pass.source_frame_id)
                record = self->capture_record;
            else {
                auto raster = ticket.owner;
                raster.camera.render_width = uint32_t(raster.render_rect[2]-raster.render_rect[0]);
                raster.camera.render_height = uint32_t(raster.render_rect[3]-raster.render_rect[1]);
                raster.camera.output_width = uint32_t(raster.output_rect[2]-raster.output_rect[0]);
                raster.camera.output_height = uint32_t(raster.output_rect[3]-raster.output_rect[1]);
                self->capture_inputs(context,raster,record,ticket.products.original_colour());
            }
            if (rsf_input_dump_wants(&record)) {
                (void)rsf_input_dump_texture_record(context,&record,"spatial_recovered",
                    "Numeric-nearest recovery, not temporal SR acceptance",pass.color_output,RSF_DUMP_VIEW_HDR,1);
                rsf_input_dump_result(&record,"spatial_recovery",0,RSF_OK);
                self->capture_record = record; self->capture_native_frame = pass.native_frame;
                self->capture_view = pass.view_key; self->capture_pending = true;
            }
        }
        ticket.output(*request); return RSF_OK;
    }
    if (!(ticket.flags & RSF_D3D11_REDUCED_FULL_OUTPUT_DEFINED)) return RSF_ERROR_BUSY;
    if (!ticket.products.restore()) {
        return incomplete(RSF_D3D11_REDUCED_CLEANUP_PENDING);
    }
    ticket.flags &= ~RSF_D3D11_REDUCED_CLEANUP_REQUIRED;
    ticket.flags |= RSF_D3D11_REDUCED_NEXT_REDUCTION_ALLOWED; ticket.output(*request);
    ticket.release(); return RSF_OK;
}
catch (...) { return RSF_ERROR_BUSY; }
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
    if (!self->configured || self->execution_thread != GetCurrentThreadId() ||
        scope->session_id != self->upstream.session_id) return RSF_ERROR_NOT_READY;
    if (self->reduced && self->reduced->pending) return RSF_ERROR_BUSY;
    // An unsuccessful upscale may already have rolled back its preparation.
    // Acknowledge that empty lease only on the verified session/graphics owner.
    if (!self->auxiliaries || !self->auxiliaries->pending()) return RSF_OK;
    if (!self->auxiliaries->matches(*scope)) return RSF_ERROR_NOT_READY;
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
    if (!same_object(device.Get(),self->device.Get())) return RSF_ERROR_INVALID_ARGUMENT;
    return rsf_input_dump_texture_record(context, &self->capture_record, request->name, request->description,
        request->texture, request->view, request->scale) ? RSF_OK : RSF_ERROR_NOT_READY;
}
catch (...) { return RSF_ERROR_NOT_READY; }
