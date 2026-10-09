// SPDX-License-Identifier: GPL-3.0-only
#include <rescaleframe/unity_sr_host.h>
#include <rescaleframe/native_sr_d3d12.h>
#include <rescaleframe/native_fg_d3d12.h>
#include <rescaleframe/fg_choice.h>
#include <rescaleframe/plugin_session.h>
#include <rescaleframe/overlay_d3d12.h>
#include <rescaleframe/overlay_fg_stats.h>
#include <rescaleframe/gpu_policy.h>
#include <rescaleframe/sha256_file.h>
#include <rescaleframe/sr_session.h>
#include <rescaleframe/unity_config.h>
#include <windows.h>
#include <d3d12.h>
#include <dxgi1_4.h>
#include <wrl/client.h>
#include <array>
#include <memory>
#include <mutex>
#include <string>
#include <cstdio>

namespace {
struct Host {
    std::mutex state, logging;
    HANDLE log_file = INVALID_HANDLE_VALUE;
    rsf_plugin_session* plugin = nullptr;
    rsf_sr12* sr = nullptr;
    rsf_sr12* inputs = nullptr;
    uint32_t input_width = 0, input_height = 0;
    std::string streamline, fsr2, fsr3, fsr4, xess;
    rsf_game_render_config plan{};
    std::array<void*, 3> command_lists{};
    uint32_t backend = RSF_SR_DLSS, quality = 1;
    uint32_t requested_backend = RSF_SR_DLSS, requested_quality = 1;
    int (*policy)(uint32_t, uint32_t) = nullptr;
    bool dirty = false, enabled = true, show_performance = false;
    bool automatic = true;
    int32_t last_result = 0;
    uint64_t overlay_frames = 0;
    uint32_t width = 0, height = 0;
    bool attempted = false;
    uint64_t accepted = 0, refused = 0;
    // The chosen generation provider is saved for the next start only once the switch has landed.
    bool save_pending = false;
    uint32_t save_backend = 0;
    ~Host() { if (log_file != INVALID_HANDLE_VALUE) CloseHandle(log_file); }
};
std::mutex lifecycle;
std::unique_ptr<Host> host;
const char* sr_name(uint32_t backend)
{
    switch (backend) {
    case RSF_SR_DLSS: return "DLSS";
    case RSF_SR_FSR2: return "FSR2";
    case RSF_SR_FSR3: return "FSR3";
    case RSF_SR_FSR4: return "FSR4";
    case RSF_SR_XESS: return "XeSS";
    case RSF_SR_SPATIAL: return "FSR1";
    default: return "Off";
    }
}
// Every backend the plugin policy accepts, as the overlay's bit-per-ID choices mask.
constexpr uint32_t kSrChoices = (1u << RSF_SR_DLSS) | (1u << RSF_SR_FSR2) | (1u << RSF_SR_FSR3) |
    (1u << RSF_SR_FSR4) | (1u << RSF_SR_XESS) | (1u << RSF_SR_SPATIAL);
// The plugin only learns whether the engine upscales spatially itself, never the vendor.
bool apply_policy(const Host& self, uint32_t backend, uint32_t quality)
{
    return self.policy && self.policy(backend == RSF_SR_SPATIAL ? 1u : 0u, quality);
}
void log(void* owner, const char* message)
{
    auto& self = *static_cast<Host*>(owner);
    std::lock_guard<std::mutex> lock(self.logging);
    if (self.log_file == INVALID_HANDLE_VALUE || !message) return;
    DWORD bytes = 0;
    WriteFile(self.log_file, message, static_cast<DWORD>(std::strlen(message)), &bytes, nullptr);
    WriteFile(self.log_file, "\r\n", 2, &bytes, nullptr);
}
int configuration(void* owner, rsf_game_render_config* output)
{
    auto& self = *static_cast<Host*>(owner);
    if (!output || output->struct_size < sizeof(*output)) return 0;
    std::lock_guard<std::mutex> lock(self.state);
    *output = self.plan; output->struct_size = sizeof(*output); return 1;
}
void render(void* owner, void* command_list, const rsf_game_render_pass* pass, uint32_t begin)
{
    if (!command_list || !pass || pass->struct_size < sizeof(*pass)) return;
    if (!begin) {
        if (pass->role == RSF_GAME_RENDER_SR || pass->role == RSF_GAME_RENDER_FG_INPUTS || pass->role == RSF_GAME_RENDER_FINAL_SCENE)
            rsf_fg12_submitted(pass->session_id, pass->source_frame_id);
        return;
    }
    if (pass->role == RSF_GAME_RENDER_FINAL_SCENE) { rsf_fg12_hudless(command_list, pass); return; }
    auto& self = *static_cast<Host*>(owner);
    std::unique_lock<std::mutex> lock(self.state);
    if (pass->role == RSF_GAME_RENDER_WINDOW) {
        rsf_fg12_window(pass);
        auto* queue = static_cast<ID3D12CommandQueue*>(command_list);
        Microsoft::WRL::ComPtr<ID3D12Device> device;
        if (FAILED(queue->GetDevice(IID_PPV_ARGS(&device)))) return;
        if (self.save_pending) {
            if (rsf_d3d11_present_switch_result()) {
                self.save_pending = false;
                log(&self, "Generation provider switch failed; the saved choice is unchanged.");
            } else if (rsf_d3d11_present_backend() == self.save_backend) {
                self.save_pending = false;
                rsf_fg_choice_save(self.save_backend);
            }
        }
        rsf_overlay_stats stats{}; stats.struct_size = sizeof(stats);
        stats.backend_loaded = 1; stats.backend_supported = self.plan.enabled;
        stats.backend_name = sr_name(self.backend); stats.backend = self.backend;
        stats.sr_backend_choices = kSrChoices;
        stats.requested_backend = self.requested_backend; stats.quality = self.quality;
        stats.enabled = self.enabled; stats.last_result = self.last_result;
        stats.last_switch_result = self.last_result;
        stats.refusal_reason = self.dirty ? "Applying requested configuration after GPU completion" :
            self.last_result ? "Provider refused; spatial fallback retained" :
            !self.accepted && self.backend != RSF_SR_SPATIAL ? "Prepared; awaiting an eligible scene evaluation" : nullptr;
        stats.render_width = self.plan.render_width; stats.render_height = self.plan.render_height;
        stats.output_width = pass->camera.output_width; stats.output_height = pass->camera.output_height;
        stats.frames_presented = static_cast<uint32_t>(++self.overlay_frames);
        UINT presented = 0;
        if (SUCCEEDED(static_cast<IDXGISwapChain*>(pass->swapchain)->GetLastPresentCount(&presented)))
            stats.application_presented_frames = presented;
        LARGE_INTEGER sample{}, frequency{}; QueryPerformanceCounter(&sample); QueryPerformanceFrequency(&frequency);
        stats.sample_qpc = static_cast<uint64_t>(sample.QuadPart); stats.qpc_frequency = static_cast<uint64_t>(frequency.QuadPart);
        stats.frames_evaluated = static_cast<uint32_t>(self.accepted); stats.frames_refused = static_cast<uint32_t>(self.refused);
        stats.reinsert_available = 1; stats.reinsert_on = self.plan.enabled;
        stats.have_scene_color = stats.have_depth = stats.have_motion = self.accepted != 0;
        stats.motion_decoded = self.accepted != 0; stats.jitter_available = 1;
        stats.jitter_active = self.plan.enabled && self.backend != RSF_SR_SPATIAL;
        stats.render_scale_percent = self.width ? self.plan.render_width * 100 / self.width : 0;
        rsf_native_fg_status fg{}; fg.struct_size = sizeof(fg);
        const bool have_fg = rsf_fg12_status(&fg) != 0;
        rsf_overlay_fill_fg_stats(&stats, have_fg ? &fg : nullptr);
        // The saved choice stays on the old provider until the switch lands, so show the pending target.
        if (self.save_pending) stats.fg_requested_backend = self.save_backend;
#if RSF_OVERLAY_ABI_VERSION >= 7
        stats.show_performance_hud = self.show_performance;
#endif
        rsf_overlay_intent intent{}; intent.struct_size = sizeof(intent);
        // The overlay draw and its GPU interop need no host state, and every managed
        // configuration query waits on this lock.
        lock.unlock();
        const int drawn = rsf_overlay_d3d12_frame(device.Get(), queue, pass->swapchain, &stats, &intent, log, &self);
        lock.lock();
        if (drawn) {
            if (intent.fg_backend_changed) {
                const auto applied = rsf_d3d11_present_request(intent.fg_backend);
                if (applied == RSF_BACKEND_OK) { self.save_pending = true; self.save_backend = intent.fg_backend; }
                log(&self, applied == RSF_BACKEND_OK ? "Generation provider requested at the next drained Present." : "Generation provider request refused.");
            }
            if (intent.fg_changed || intent.reflex_changed || intent.frame_limit_changed) {
                if (intent.fg_changed) { fg.requested.mode = intent.fg_mode; fg.requested.generated_frames = intent.fg_generated; }
                if (intent.reflex_changed) fg.requested.reflex_mode = intent.reflex_mode;
                if (intent.frame_limit_changed) fg.requested.frame_limit_us = intent.frame_limit_us;
                rsf_fg12_options(&fg.requested);
            }
#if RSF_OVERLAY_ABI_VERSION >= 7
            if (intent.performance_hud_changed) self.show_performance = intent.performance_hud != 0;
#endif
            if (intent.backend_changed || intent.quality_changed || intent.enabled_changed || intent.start_requested) {
                const auto requested_backend = intent.backend_changed ? intent.backend : self.requested_backend;
                const auto requested_quality = intent.quality_changed ? intent.quality : self.requested_quality;
                if (requested_backend <= RSF_SR_SPATIAL && requested_quality <= 4 && apply_policy(self, requested_backend, requested_quality)) {
                    self.requested_backend = requested_backend; self.requested_quality = requested_quality;
                    if (intent.backend_changed) self.automatic = false;
                    if (intent.enabled_changed) self.enabled = intent.enabled != 0;
                    self.dirty = true; self.plan.enabled = 0;
                    log(&self, "Overlay requested vendor/quality change; draining previous GPU work.");
                }
            }
        }
        return;
    }
    if (pass->role != RSF_GAME_RENDER_SR && pass->role != RSF_GAME_RENDER_FG_INPUTS) return;
    auto* list = static_cast<ID3D12GraphicsCommandList*>(command_list);
    if ((self.dirty && (pass->flags & RSF_GAME_RENDER_RECONFIGURE)) || self.width != pass->camera.output_width || self.height != pass->camera.output_height) {
        // The Unity submission adapter joins its previous frame-fence values before this resize.
        rsf_sr12_destroy(self.sr); self.sr = nullptr;
        rsf_sr12_destroy(self.inputs); self.inputs = nullptr; self.input_width = self.input_height = 0;
        self.width = pass->camera.output_width; self.height = pass->camera.output_height;
        self.attempted = false; self.plan = {};
        self.backend = self.requested_backend; self.quality = self.requested_quality;
        self.dirty = false; self.accepted = self.refused = 0; self.last_result = 0;
    }
    if (!self.attempted) {
        self.attempted = true;
        self.plan = {sizeof(self.plan), 0, self.width, self.height, 0, 0};
        if (self.automatic) {
            Microsoft::WRL::ComPtr<ID3D12Device> device;
            Microsoft::WRL::ComPtr<IDXGIFactory4> factory;
            Microsoft::WRL::ComPtr<IDXGIAdapter1> adapter;
            DXGI_ADAPTER_DESC1 description{};
            if (FAILED(list->GetDevice(IID_PPV_ARGS(&device))) || FAILED(CreateDXGIFactory1(IID_PPV_ARGS(&factory))) ||
                FAILED(factory->EnumAdapterByLuid(device->GetAdapterLuid(), IID_PPV_ARGS(&adapter))) ||
                FAILED(adapter->GetDesc1(&description))) {
                self.last_result = RSF_BACKEND_ERROR_NOT_SUPPORTED;
                log(&self, "Automatic SR refused: owning D3D12 adapter could not be identified.");
                return;
            }
            self.backend = self.requested_backend = rsf_gpu_default_sr_backend(description.VendorId,
                (description.Flags & DXGI_ADAPTER_FLAG_SOFTWARE) != 0);
            if (!apply_policy(self, self.backend, self.quality)) return;
            char detail[160];
            std::snprintf(detail, sizeof(detail), "Auto SR selected backend %u from owning D3D12 adapter vendor=0x%x device=0x%x",
                self.backend, description.VendorId, description.DeviceId);
            log(&self, detail);
        }
        if (!self.backend || !self.enabled) { /* Native rendering still supplies generation inputs below. */ }
        else if (self.backend == RSF_SR_SPATIAL) {
            constexpr float ratios[] = {1.0f, 1.5f, 1.7f, 2.0f, 3.0f, 1.3f};
            self.plan.render_width = static_cast<uint32_t>(static_cast<float>(self.width) / ratios[self.quality]);
            self.plan.render_height = static_cast<uint32_t>(static_cast<float>(self.height) / ratios[self.quality]);
            self.plan.enabled = 1;
            log(&self, "FSR1 selected through URP's original spatial upscaling path.");
        } else {
        Microsoft::WRL::ComPtr<ID3D12Device> device;
        if (FAILED(list->GetDevice(IID_PPV_ARGS(&device)))) return;
        rsf_sr12_setup setup{}; setup.struct_size = sizeof(setup); setup.abi_version = 1;
        setup.device = device.Get(); setup.backend = self.backend; setup.quality = self.quality;
        setup.streamline_host = rsf_d3d11_present_host();
        setup.output_width = self.width; setup.output_height = self.height; setup.inverted_depth = pass->camera.depth_inverted;
        setup.fsr2_directory_utf8 = self.fsr2.c_str(); setup.fsr3_directory_utf8 = self.fsr3.c_str();
        setup.fsr4_directory_utf8 = self.fsr4.c_str(); setup.xess_directory_utf8 = self.xess.c_str();
        setup.log = log; setup.user = &self;
        const std::string interposer = self.streamline + "\\sl.interposer.dll";
        setup.dlss.struct_size = sizeof(setup.dlss); setup.dlss.abi_version = RSF_DLSS_ABI_VERSION;
        setup.dlss.interposer_path_utf8 = interposer.c_str(); setup.dlss.plugin_directory_utf8 = self.streamline.c_str();
        setup.dlss.engine = RSF_DLSS_ENGINE_UNITY; setup.dlss.engine_version_utf8 = RSF_UNITY_ENGINE_VERSION;
        setup.dlss.project_id_utf8 = RSF_UNITY_PROJECT_ID;
        setup.dlss.require_signature = 1; setup.dlss.log = log; setup.dlss.log_user = &self;
        const auto created = rsf_sr12_create(&setup, &self.sr);
        self.last_result = created;
        if (created == RSF_BACKEND_OK) {
            rsf_sr12_plan(self.sr, &self.plan.render_width, &self.plan.render_height);
            self.plan.enabled = 1;
        }
        char text[192];
        std::snprintf(text, sizeof(text), "Native D3D12 provider %u preparation result %d, %ux%u -> %ux%u",
            self.backend, created, self.plan.render_width, self.plan.render_height, self.width, self.height);
        log(&self, text);
        }
    }
    if (pass->role == RSF_GAME_RENDER_FG_INPUTS && pass->camera_valid) {
        if (!self.inputs || self.input_width != pass->camera.render_width || self.input_height != pass->camera.render_height) {
            rsf_sr12_destroy(self.inputs); self.inputs = nullptr;
            Microsoft::WRL::ComPtr<ID3D12Device> device;
            if (FAILED(list->GetDevice(IID_PPV_ARGS(&device)))) return;
            rsf_sr12_setup setup{}; setup.struct_size = sizeof(setup); setup.abi_version = 1; setup.device = device.Get();
            setup.output_width = pass->camera.output_width; setup.output_height = pass->camera.output_height;
            setup.render_width = pass->camera.render_width; setup.render_height = pass->camera.render_height;
            setup.log = log; setup.user = &self;
            if (rsf_sr12_create(&setup, &self.inputs) != RSF_BACKEND_OK) return;
            self.input_width = setup.render_width; self.input_height = setup.render_height;
        }
        uint32_t slot = 3;
        for (uint32_t i = 0; i < 3; ++i) if (self.command_lists[i] == command_list) { slot = i; break; }
        if (slot == 3) for (uint32_t i = 0; i < 3; ++i) if (!self.command_lists[i]) { self.command_lists[i] = command_list; slot = i; break; }
        if (slot < 3) (void)rsf_sr12_evaluate(self.inputs, command_list, pass, slot);
        return;
    }
    if (!self.sr || !pass->camera_valid) return;
    uint32_t slot = 3;
    for (uint32_t i = 0; i < 3; ++i) if (self.command_lists[i] == command_list) { slot = i; break; }
    if (slot == 3) for (uint32_t i = 0; i < 3; ++i) if (!self.command_lists[i]) { self.command_lists[i] = command_list; slot = i; break; }
    if (slot == 3) return;
    const auto result = rsf_sr12_evaluate(self.sr, command_list, pass, slot);
    if (result != RSF_BACKEND_OK && self.refused == 0) {
        void* resources[] = {pass->color_input, pass->depth, pass->motion, pass->color_output};
        for (uint32_t i = 0; i < 4; ++i) if (resources[i]) {
            const auto desc = static_cast<ID3D12Resource*>(resources[i])->GetDesc();
            char detail[192];
            std::snprintf(detail, sizeof(detail), "SR input %u: %llux%u format=%u samples=%u; camera=%ux%u plan=%ux%u",
                i, static_cast<unsigned long long>(desc.Width), desc.Height, static_cast<uint32_t>(desc.Format), desc.SampleDesc.Count,
                pass->camera.render_width, pass->camera.render_height, self.plan.render_width, self.plan.render_height);
            log(&self, detail);
        }
    }
    self.last_result = result;
    if (result == RSF_BACKEND_OK) ++self.accepted; else ++self.refused;
    if (result == RSF_BACKEND_OK ? (self.accepted <= 3 || self.accepted % 600 == 0) : self.refused <= 3) {
        char text[160];
        std::snprintf(text, sizeof(text), "Unity SR view %llu frame %llu result %d, accepted=%llu refused=%llu reset=%u jitter=%.3f,%.3f",
            static_cast<unsigned long long>(pass->view_key), static_cast<unsigned long long>(pass->native_frame), result,
            static_cast<unsigned long long>(self.accepted), static_cast<unsigned long long>(self.refused),
            pass->flags & RSF_GAME_RENDER_RESET, pass->camera.jitter_pixels[0], pass->camera.jitter_pixels[1]);
        log(&self, text);
    }
}
void cpu_event(void*, const rsf_game_cpu_event* event) { rsf_fg12_cpu(event); }
}
extern "C" uint32_t __stdcall rsf_unity_sr_start(const wchar_t* path) try
{
    std::lock_guard<std::mutex> lock(lifecycle);
    if (host || !path || path[1] != L':' || (path[2] != L'\\' && path[2] != L'/')) return 1;
    std::unique_ptr<Host> next(new Host);
    if (!rsf_fg_choice_get().choices) rsf::unity_config::fg_choice_start(path);
    next->backend = GetPrivateProfileIntW(L"UnitySR", L"Backend", RSF_SR_AUTO, path);
    next->automatic = next->backend == RSF_SR_AUTO;
    if (next->automatic) next->backend = 0;
    next->quality = GetPrivateProfileIntW(L"UnitySR", L"Quality", 1, path);
    next->requested_backend = next->backend; next->requested_quality = next->quality;
    if (next->backend > RSF_SR_SPATIAL || next->quality > 5 || (next->backend == RSF_SR_DLSS && next->quality > 4)) return 2;
    using rsf::unity_config::path_setting;
    using rsf::unity_config::utf8;
    const auto plugin_path = path_setting(path, L"Plugin");
    const auto log_path = path_setting(path, L"Log");
    if (plugin_path.empty() || log_path.empty()) return 3;
    next->log_file = CreateFileW(log_path.c_str(), FILE_APPEND_DATA, FILE_SHARE_READ | FILE_SHARE_WRITE, nullptr, OPEN_ALWAYS, FILE_ATTRIBUTE_NORMAL, nullptr);
    if (next->log_file == INVALID_HANDLE_VALUE) return 4;
    next->streamline = utf8(path_setting(path, L"Streamline")); next->fsr2 = utf8(path_setting(path, L"FSR2"));
    next->fsr3 = utf8(path_setting(path, L"FSR3")); next->fsr4 = utf8(path_setting(path, L"FSR4")); next->xess = utf8(path_setting(path, L"XeSS"));
    wchar_t executable[32768]{};
    DWORD length = GetModuleFileNameW(nullptr, executable, 32768);
    if (!length || length >= 32768) return 5;
    const auto executable_name = utf8(std::wstring(executable).substr(std::wstring(executable).find_last_of(L"/\\") + 1));
    std::string hash;
    if (!rsf::sha256_file(executable, hash)) return 6;
    rsf_game_probe probe{sizeof(probe), 0x8664, 0, executable_name.c_str(), hash.c_str()};
    HMODULE policy_module = LoadLibraryExW(plugin_path.c_str(), nullptr, LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_DEFAULT_DIRS);
    if (!policy_module) return 7;
    using Policy = int (*)(uint32_t, uint32_t);
    auto policy = reinterpret_cast<Policy>(reinterpret_cast<void*>(GetProcAddress(policy_module, "rsf_unity_set_engine_spatial")));
    next->policy = policy;
    if (!apply_policy(*next, next->backend, next->quality)) { FreeLibrary(policy_module); return 8; }
    rsf_plugin_session_options options{}; options.struct_size = sizeof(options); options.abi_version = RSF_PLUGIN_SESSION_ABI_VERSION;
    options.plugin_path = plugin_path.c_str(); options.probe = &probe;
    options.services.struct_size = sizeof(options.services); options.services.abi_version = RSF_GAME_ABI_VERSION;
    options.services.session_id = GetTickCount64() + 1; options.services.log = log; options.services.render_pass = render;
    options.services.render_config = configuration; options.services.user = next.get();
    options.services.cpu_event = cpu_event;
    auto result = rsf_plugin_session_prepare(&options, &next->plugin);
    FreeLibrary(policy_module);
    if (result == RSF_OK) result = rsf_plugin_session_start(next->plugin);
    if (result != RSF_OK) {
        log(next.get(), "Unity plugin preparation/activation refused.");
        if (next->plugin) {
            rsf_plugin_session_quiesce(next->plugin);
            if (rsf_plugin_session_stop(next->plugin) != RSF_OK) { host = std::move(next); return 9; }
        }
        return 10;
    }
    log(next.get(), "Unity Mono lifecycle activated; awaiting actual D3D12 render callback.");
    host = std::move(next); return 0;
}
catch (...) { return 11; }
extern "C" uint32_t __stdcall rsf_unity_sr_stop(void*) try
{
    std::lock_guard<std::mutex> lock(lifecycle);
    if (!host) return 0;
    if (host->plugin && (rsf_plugin_session_quiesce(host->plugin) != RSF_OK || rsf_plugin_session_stop(host->plugin) != RSF_OK))
        return 1;
    host->plugin = nullptr;
    rsf_overlay_d3d12_stop();
    rsf_sr12_destroy(host->sr); host->sr = nullptr;
    rsf_sr12_destroy(host->inputs); host->inputs = nullptr;
    log(host.get(), "Unity SR stopped after managed/native/GPU retirement.");
    host.reset(); return 0;
}
catch (...) { return 2; }
