// SPDX-License-Identifier: GPL-3.0-only
#include <rescaleframe/rdr2_sr_host.h>
#include <rescaleframe/rdr2_frontend.h>
#include <rescaleframe/native_sr_d3d12.h>
#include <rescaleframe/dlss_native12.h>
#include <rescaleframe/native_fg_d3d12.h>
#include <rescaleframe/gpu_policy.h>
#include <rescaleframe/overlay_d3d12.h>
#include <rescaleframe/overlay_fg_stats.h>
#include "../../../loader/proxy/src/overlay_host.h"
#include <dxgi1_4.h>
#include <windows.h>
#include <d3d12.h>
#include <wrl/client.h>
#include <array>
#include <atomic>
#include <algorithm>
#include <cmath>
#include <cstdio>
#include <memory>
#include <mutex>
#include <string>
#include <unordered_map>
#include <vector>

namespace {
using Microsoft::WRL::ComPtr;
struct Host;
std::atomic<Host*> frontend_host{};
std::string utf8(const std::wstring& value)
{
    const int bytes = WideCharToMultiByte(CP_UTF8, 0, value.data(), static_cast<int>(value.size()), nullptr, 0, nullptr, nullptr);
    std::string out(static_cast<size_t>(bytes), '\0');
    if (bytes) WideCharToMultiByte(CP_UTF8, 0, value.data(), static_cast<int>(value.size()), out.data(), bytes, nullptr, nullptr);
    return out;
}
struct Slot {
    void* commands{};
    uint64_t serial{}, fence_value{};
    bool pending{};
    bool capture_retired{};
    uint64_t native_frame{};
};
struct Host {
    std::mutex guard;
    std::wstring ini;
    std::string root, streamline, interposer, fsr, xess;
    std::string capture_directory;
    rsf_game_log_fn logger{};
    void* logger_user{};
    uint64_t session{}, poll_tick{}, serial{}, fence_value{};
    uint32_t backend{RSF_SR_DLSS}, preset{};
    bool enabled{true}, reset{true};
    std::atomic<bool> poisoned{};
    ComPtr<ID3D12Device> device;
    ComPtr<ID3D12Fence> fence;
    ComPtr<ID3D12CommandQueue> queue;
    rsf_sr12* context{};
    // The game retains native NGX handles for pause/fallback. Global SDK shutdown during a
    // backend change invalidates those handles, so the registration belongs to the session.
    rsf_dlss_native12_registration* dlss_registration{};
    uint32_t context_backend{}, quality{}, width{}, height{}, output_width{}, output_height{}, context_preset{};
    bool context_exposure{};
    bool fg_installed{};
    uint32_t fg_backend_requested{UINT32_MAX};
    std::atomic<uint32_t> latency_reports{};
    int32_t fg_switch_result{INT32_MAX};
    std::array<Slot, 3> slots{};
    struct Scene {
        uint64_t frame{}, source{}, recording{};
        uintptr_t surface{};
        bool submitted{}, captured{};
    };
    std::array<Scene, 64> scenes{};
    uint32_t scene_reports{};
    uint32_t renderer_reason{}, status_overlay{1};
    uint64_t feedback_tick{}, native_frame{}, application_presents{}, rate_tick{}, rate_base{}, rate_output{};
    uint32_t rate_backend{};
    float base_fps{}, output_fps{};
    bool output_fps_valid{};
    bool overlay_probe{};
    uint32_t overlay_probe_phase{};
    uint64_t overlay_probe_start{};
    // Snapshot the recording serial before Execute. A recycled list cannot retire a newer slot.
    std::unordered_map<uint64_t, std::vector<std::pair<size_t, uint64_t>>> submitting;
    int32_t last_result{INT32_MAX};
    int32_t last_dispatch{INT32_MAX};
    void log(const char* message) { if (logger) logger(logger_user, message); }
    static void backend_log(void* user, const char* message) { static_cast<Host*>(user)->log(message); }
    static int accept_window(void*, void* pointer)
    {
        DWORD pid{}; wchar_t name[64]{};
        auto window = static_cast<HWND>(pointer);
        return GetWindowThreadProcessId(window, &pid) && pid == GetCurrentProcessId() &&
            GetClassNameW(window, name, 64) && std::wcscmp(name, L"sgaWindow") == 0;
    }
    uint32_t vendor()
    {
        ComPtr<IDXGIFactory4> factory; ComPtr<IDXGIAdapter1> adapter; DXGI_ADAPTER_DESC1 desc{};
        return device && SUCCEEDED(CreateDXGIFactory1(IID_PPV_ARGS(&factory))) &&
            SUCCEEDED(factory->EnumAdapterByLuid(device->GetAdapterLuid(), IID_PPV_ARGS(&adapter))) &&
            SUCCEEDED(adapter->GetDesc1(&desc)) ? desc.VendorId : 0;
    }
    void settings()
    {
        const auto now = GetTickCount64();
        if (now < poll_tick) return;
        poll_tick = now + 1000;
        enabled = GetPrivateProfileIntW(L"RDR2", L"SR", 1, ini.c_str()) != 0;
        backend = GetPrivateProfileIntW(L"RDR2", L"SRBackend", RSF_SR_DLSS, ini.c_str());
        preset = GetPrivateProfileIntW(L"RDR2", L"DLSSPreset", 0, ini.c_str());
        status_overlay = GetPrivateProfileIntW(L"RDR2", L"StatusOverlay", 1, ini.c_str()) != 0;
        if (fg_installed) {
            rsf_native_fg_options options{sizeof(options), GetPrivateProfileIntW(L"RDR2", L"FG", 0, ini.c_str()),
                GetPrivateProfileIntW(L"RDR2", L"FGGeneratedFrames", 1, ini.c_str()),
                GetPrivateProfileIntW(L"RDR2", L"Latency", RSF_REFLEX_ON, ini.c_str()),
                GetPrivateProfileIntW(L"RDR2", L"FrameLimitUs", 0, ini.c_str()), 0};
            rsf_fg12_options(&options);
            auto desired = GetPrivateProfileIntW(L"RDR2", L"FGBackend", RSF_FG_BACKEND_AUTO, ini.c_str());
            if (desired == RSF_FG_BACKEND_AUTO && device) {
                desired = rsf_fg_default_backend(vendor(), 0);
                const auto choices = rsf_d3d11_present_backend_choices();
                if (!(choices & (1u << desired))) desired = (choices & (1u << RSF_FG_BACKEND_FSR3)) ? RSF_FG_BACKEND_FSR3 : 0;
            }
            if (desired != RSF_FG_BACKEND_AUTO && desired != fg_backend_requested && rsf_d3d11_present_has_owner()) {
                const auto result = rsf_d3d11_present_request(desired);
                if (result == RSF_BACKEND_OK) fg_backend_requested = desired;
                if (result != fg_switch_result) {
                    char message[112]{}; std::snprintf(message, sizeof(message), "RDR2 FG backend request=%u result=%d.", desired, result);
                    log(message); fg_switch_result = result;
                }
            }
        }
    }
    bool wait(uint64_t value)
    {
        if (!value) return true;
        const auto completed = fence->GetCompletedValue();
        if (completed == UINT64_MAX) { poisoned = true; return false; }
        if (completed >= value) return true;
        HANDLE event = CreateEventW(nullptr, FALSE, FALSE, nullptr);
        if (!event) return false;
        const bool result = SUCCEEDED(fence->SetEventOnCompletion(value, event)) &&
            WaitForSingleObject(event, 15000) == WAIT_OBJECT_0;
        CloseHandle(event);
        if (!result || fence->GetCompletedValue() == UINT64_MAX) { poisoned = true; return false; }
        return true;
    }
    bool drain()
    {
        // Never wait for work the engine has not submitted. Its producer might be this worker.
        for (const auto& slot : slots) if (slot.pending) return false;
        return wait(fence_value);
    }
    void flush_captures()
    {
        if (!context || capture_directory.empty() || !fence) return;
        const auto completed = fence->GetCompletedValue();
        if (completed == UINT64_MAX) { poisoned = true; return; }
        for (uint32_t i = 0; i < slots.size(); ++i) {
            auto& slot = slots[i];
            if (!slot.pending && !slot.capture_retired && slot.fence_value && completed >= slot.fence_value) {
                slot.capture_retired = rsf_sr12_capture_flush(context, i) == RSF_BACKEND_OK;
            }
        }
    }
    uint32_t infer_quality(const rsf_camera_frame& camera)
    {
        // The game applies resolution changes asynchronously. Until its actual
        // inputs change, keep the backend configured for those actual inputs.
        const float ratio = static_cast<float>(camera.output_width) / camera.render_width;
        const float ratios[] = {1, 1.5f, 1.72f, 2, 3};
        uint32_t best{};
        for (uint32_t i = 1; i < 5; ++i)
            if (std::abs(ratio - ratios[i]) < std::abs(ratio - ratios[best])) best = i;
        return best;
    }
    int32_t process(void* commands, const rsf_game_render_pass* pass)
    {
        if (!pass || pass->struct_size < sizeof(*pass) || !commands || !pass->camera_valid ||
            pass->session_id != session) return RSF_ERROR_INVALID_ARGUMENT;
        std::lock_guard lock(guard);
        flush_captures();
        settings();
        if (poisoned || !enabled || backend < RSF_SR_DLSS || backend > RSF_SR_XESS ||
            !rsf_dlss_preset_valid(preset)) { reset = true; return RSF_ERROR_NOT_READY; }
        ComPtr<ID3D12Device> owner;
        auto* list = static_cast<ID3D12GraphicsCommandList*>(commands);
        if (FAILED(list->GetDevice(IID_PPV_ARGS(&owner)))) return RSF_ERROR_NOT_READY;
        if (!device) {
            device = owner;
            if (FAILED(device->CreateFence(0, D3D12_FENCE_FLAG_NONE, IID_PPV_ARGS(&fence)))) {
                device.Reset(); return RSF_ERROR_NOT_READY;
            }
        }
        if (owner.Get() != device.Get()) return RSF_ERROR_NATIVE_REFUSED;
        const auto& camera = pass->camera;
        const auto desired_quality = infer_quality(camera);
        if (desired_quality > 4) return RSF_ERROR_INVALID_ARGUMENT;
        if (!context || context_backend != backend || quality != desired_quality || context_preset != preset ||
            context_exposure != (pass->exposure != nullptr) ||
            width != camera.render_width || height != camera.render_height ||
            output_width != camera.output_width || output_height != camera.output_height) {
            if (!drain()) return RSF_ERROR_BUSY;
            rsf_sr12_setup setup{};
            setup.struct_size = sizeof(setup); setup.abi_version = 2;
            setup.device = device.Get(); setup.backend = backend; setup.quality = desired_quality;
            setup.output_width = camera.output_width; setup.output_height = camera.output_height;
            setup.render_width = camera.render_width; setup.render_height = camera.render_height;
            setup.engine_input_extents = 1; setup.inverted_depth = 1;
            setup.external_exposure = pass->exposure ? 1u : 0u;
            setup.fsr2_directory_utf8 = setup.fsr3_directory_utf8 = setup.fsr4_directory_utf8 = fsr.c_str();
            setup.xess_directory_utf8 = xess.c_str(); setup.log = backend_log; setup.user = this;
            setup.dlss.struct_size = sizeof(setup.dlss); setup.dlss.abi_version = RSF_DLSS_ABI_VERSION;
            setup.dlss.interposer_path_utf8 = interposer.c_str(); setup.dlss.plugin_directory_utf8 = streamline.c_str();
            setup.dlss.engine = RSF_DLSS_ENGINE_CUSTOM; setup.dlss.engine_version_utf8 = "RAGE 1.0.1491.50";
            setup.dlss.application_id = 0x60e8ab7; setup.dlss.require_signature = 1;
            setup.dlss.log = backend_log; setup.dlss.log_user = this; setup.dlss.preset = preset;
            setup.streamline_host = rsf_d3d11_present_host();
            // The scene is linear floating-point HDR even when the swapchain is SDR.
            rsf_sr12* replacement{};
            if (backend == RSF_SR_DLSS && !setup.streamline_host) {
                if (!dlss_registration) {
                    const auto registered = rsf_dlss_native12_registration_create(device.Get(), &setup.dlss, &dlss_registration);
                    if (registered != RSF_BACKEND_OK) return registered;
                    log("RDR2 NGX registration retained for native game handles across backend changes.");
                }
                setup.dlss_registration = dlss_registration;
            }
            // Retire this SR feature's resources after GPU completion. Its registration remains
            // alive for native evaluation and for a later SR context on the same device.
            rsf_sr12_destroy(context); context = nullptr;
            const auto result = rsf_sr12_create(&setup, &replacement);
            if (result != RSF_BACKEND_OK) return result;
            context = replacement;
            if (!capture_directory.empty()) {
                const auto captured = rsf_sr12_capture(context, capture_directory.c_str(), pass->native_frame, 4);
                if (captured != RSF_BACKEND_OK) log("RDR2 bounded SR capture configuration refused.");
            }
            context_backend = backend; quality = desired_quality; context_preset = preset;
            context_exposure = pass->exposure != nullptr;
            width = camera.render_width; height = camera.render_height;
            output_width = camera.output_width; output_height = camera.output_height;
            reset = true;
            char message[192]{};
            std::snprintf(message, sizeof(message), "RDR2 SR context backend=%u quality=%u input=%ux%u output=%ux%u.",
                backend, quality, width, height, output_width, output_height);
            log(message);
        }
        size_t index = slots.size();
        for (size_t i = 0; i < slots.size(); ++i) {
            if (!slots[i].pending && (!slots[i].fence_value || fence->GetCompletedValue() >= slots[i].fence_value)) {
                index = i; break;
            }
        }
        if (index == slots.size()) {
            // A submitted slot can be joined. An unsubmitted one must never block its producer.
            for (size_t i = 0; i < slots.size(); ++i) if (!slots[i].pending && wait(slots[i].fence_value)) { index = i; break; }
        }
        if (index == slots.size()) { reset = true; return RSF_ERROR_BUSY; }
        auto copied = *pass;
        if (reset) copied.flags |= RSF_GAME_RENDER_RESET;
        // Reserve before evaluation: a failure can still contain normalization/vendor commands.
        slots[index] = {commands, ++serial, 0, true};
        slots[index].native_frame = pass->native_frame;
        const auto result = rsf_sr12_evaluate(context, commands, &copied, static_cast<uint32_t>(index));
        reset = result != RSF_BACKEND_OK;
        if (result != last_result) {
            char message[128]{};
            std::snprintf(message, sizeof(message), "RDR2 RSF SR evaluation result=%d frame=%llu.", result,
                static_cast<unsigned long long>(pass->native_frame));
            log(message); last_result = result;
        }
        return result;
    }
    void submit(void* native_queue, uint64_t id, uint32_t count, void* const* lists, bool begin)
    {
        std::lock_guard lock(guard);
        if (!native_queue) return;
        // The plugin observes only the renderer's selected DIRECT queue. Retain it
        // before the first upscale so the overlay also works on the opening menus.
        if (!queue) queue = static_cast<ID3D12CommandQueue*>(native_queue);
        if (!context || !fence) return;
        if (begin) {
            std::vector<std::pair<size_t, uint64_t>> snapshot;
            for (size_t i = 0; i < slots.size(); ++i) if (slots[i].pending &&
                std::find(lists, lists + count, slots[i].commands) != lists + count) snapshot.emplace_back(i, slots[i].serial);
            if (!snapshot.empty()) submitting.emplace(id, std::move(snapshot));
            return;
        }
        const auto entry = submitting.find(id);
        if (entry == submitting.end()) return;
        auto* submitted_queue = static_cast<ID3D12CommandQueue*>(native_queue);
        if (queue && queue.Get() != submitted_queue) {
            poisoned = true; log("RDR2 SR refused: its recordings reached multiple DIRECT queues.");
            submitting.erase(entry); return;
        }
        queue = submitted_queue;
        const auto value = ++fence_value;
        if (FAILED(queue->Signal(fence.Get(), value))) {
            poisoned = true; log("RDR2 SR queue fence signal failed; GPU resources remain pinned.");
            submitting.erase(entry); return;
        }
        for (const auto& [index, recording] : entry->second) {
            auto& slot = slots[index];
            if (slot.pending && slot.serial == recording) { slot.pending = false; slot.fence_value = value; }
            auto& scene = scenes[slot.native_frame % scenes.size()];
            if (scene.frame == slot.native_frame && scene.recording == recording) scene.submitted = true;
            if (scene.frame == slot.native_frame && scene.recording == recording) rsf_fg12_submitted(session, scene.frame);
        }
        submitting.erase(entry);
    }
    void scene(void* commands, const rsf_game_render_pass& pass)
    {
        std::lock_guard lock(guard);
        if (!context || !pass.color_input || !pass.scene_surface || !pass.native_frame) return;
        for (uint32_t i = 0; i < slots.size(); ++i) {
            const auto& slot = slots[i];
            if (!slot.pending || slot.commands != commands || slot.native_frame != pass.native_frame) continue;
            const auto result = rsf_sr12_capture_scene(context, commands, &pass, i);
            auto& scene = scenes[pass.native_frame % scenes.size()];
            scene = {pass.native_frame, pass.source_frame_id, slot.serial,
                reinterpret_cast<uintptr_t>(pass.scene_surface), false, result == RSF_BACKEND_OK};
            if (pass.source_frame_id) rsf_fg12_hudless(commands, &pass);
            return;
        }
    }
    void window(const rsf_game_render_pass& pass)
    {
        std::lock_guard lock(guard);
        if (!pass.native_frame) return;
        const auto& scene = scenes[pass.native_frame % scenes.size()];
        const bool matched = scene.frame == pass.native_frame && scene.surface == reinterpret_cast<uintptr_t>(pass.scene_surface) && scene.submitted;
        if (matched && pass.source_frame_id && scene.source == pass.source_frame_id) rsf_fg12_window(&pass);
        if (scene_reports++ < 8 || scene.captured) {
            char message[192]{};
            std::snprintf(message, sizeof(message), "RDR2 scene/Present frame=%llu input=%llu submitted=%u surface_match=%u captured=%u.",
                static_cast<unsigned long long>(pass.native_frame), static_cast<unsigned long long>(pass.source_frame_id),
                scene.submitted ? 1u : 0u, matched ? 1u : 0u, scene.captured ? 1u : 0u);
            log(message);
        }
    }
};
int32_t process(void* user, void* commands, const rsf_game_render_pass* pass) noexcept try
{
    auto& owner = *static_cast<Host*>(user);
    const auto result = owner.process(commands, pass);
    std::lock_guard lock(owner.guard);
    if (result != owner.last_dispatch) {
        char message[112]{};
        std::snprintf(message, sizeof(message), "RDR2 SR host dispatch result=%d; %s.", result,
            result == 0 ? "RSF output accepted" : "native evaluation retained");
        owner.log(message); owner.last_dispatch = result;
    }
    return result;
} catch (...) { return RSF_ERROR_NOT_READY; }
void submit(void* user, void* queue, uint64_t id, uint32_t count, void* const* lists, uint32_t begin) noexcept try
{
    static_cast<Host*>(user)->submit(queue, id, count, lists, begin != 0);
} catch (...) { static_cast<Host*>(user)->poisoned = true; }
void overlay(Host& owner, void* chain)
{
    ComPtr<ID3D12CommandQueue> queue;
    uint32_t quality{};
    {
        std::lock_guard lock(owner.guard);
        queue = owner.queue; quality = owner.quality;
    }
    if (!queue) return;
    ComPtr<ID3D12Device> device;
    if (FAILED(queue->GetDevice(IID_PPV_ARGS(&device)))) return;
    thread_local rsf_rdr2_status status{sizeof(rsf_rdr2_status), RSF_RDR2_FRONTEND_ABI_VERSION};
    rsf_rdr2_status sample{sizeof(rsf_rdr2_status), RSF_RDR2_FRONTEND_ABI_VERSION};
    if (rsf_rdr2_status_get(&sample) == RSF_OK) status = sample;
    if (owner.overlay_probe && !owner.overlay_probe_phase && status.application_presents>=60) {
        rsf_overlay_native12_toggle();
        if (rsf_overlay_native12_visible()) {
            owner.overlay_probe_phase=1;owner.overlay_probe_start=status.application_presents;
            owner.log("RDR2 overlay probe: opening shared panel.");
            int count=0;unsigned attempts=0;
            do {count=ShowCursor(FALSE);} while(count>=0 && ++attempts<512);
            if (count>=0) { owner.overlay_probe_phase=3;owner.log("RDR2 overlay probe FAILED: cursor hide loop did not terminate."); }
            else owner.log("RDR2 overlay probe: game-style cursor hide loop returned negative.");
        }
    } else if (owner.overlay_probe && owner.overlay_probe_phase==1 && status.application_presents-owner.overlay_probe_start>=120) {
        owner.overlay_probe_phase=2;
        if (rsf_overlay_native12_visible()) rsf_overlay_native12_toggle();
        owner.log("RDR2 overlay probe: full panel survived120 application Presents; closed.");
    }
    if (owner.overlay_probe && owner.overlay_probe_phase==1) {
        char progress[96];std::snprintf(progress,sizeof(progress),"RDR2 overlay probe present=%llu",
            static_cast<unsigned long long>(status.application_presents));owner.log(progress);
    }
    thread_local rsf_native_fg_status fg{sizeof(rsf_native_fg_status)};
    rsf_native_fg_status fresh{sizeof(rsf_native_fg_status)};
    if (rsf_fg12_status_try(&fresh)) fg = fresh;
    // Upscale workers own the renderer lock for most of a busy frame. A busy
    // settings observer must not remove the HUD/panel from that frame.
    thread_local rsf_rdr2_settings cached_prefs{sizeof(rsf_rdr2_settings), RSF_RDR2_FRONTEND_ABI_VERSION};
    thread_local bool have_prefs{};
    auto prefs = cached_prefs;
    const auto settings_result = rsf_rdr2_settings_get(&prefs);
    if (settings_result == RSF_OK) { cached_prefs = prefs; have_prefs = true; }
    else if (settings_result != RSF_ERROR_BUSY || !have_prefs) return;
    rsf_overlay_stats stats{}; stats.struct_size = sizeof(stats);
    stats.backend_loaded = 1; stats.backend_supported = status.sr_effective_backend != 0;
    stats.backend = prefs.sr_backend; stats.requested_backend = prefs.sr_backend;
    stats.enabled = prefs.sr_enabled; stats.quality = quality;
    stats.sr_backend_choices = (1u << RSF_SR_DLSS) | (1u << RSF_SR_FSR3) | (1u << RSF_SR_FSR4) | (1u << RSF_SR_XESS);
    stats.dlss_preset = prefs.dlss_preset; stats.dlss_preset_available = 1;
    stats.refusal_reason = status.fg_requested && !status.fg_active ? status.reason : nullptr;
    stats.render_width = status.render_width; stats.render_height = status.render_height;
    stats.output_width = status.output_width; stats.output_height = status.output_height;
    stats.application_presented_frames = status.application_presents;
    stats.show_performance_hud = prefs.status_overlay;
    LARGE_INTEGER now{}, frequency{}; QueryPerformanceCounter(&now); QueryPerformanceFrequency(&frequency);
    stats.sample_qpc = static_cast<uint64_t>(now.QuadPart); stats.qpc_frequency = static_cast<uint64_t>(frequency.QuadPart);
    rsf_overlay_fill_fg_stats(&stats, &fg);
    // Keep the saved provider for re-enabling FG, but Off is the selected UI
    // choice while disabled. A retained SDK context is not an active provider.
    stats.fg_requested_backend = !prefs.fg_enabled ? 0 :
        prefs.fg_backend == RSF_FG_BACKEND_AUTO ? stats.fg_backend : prefs.fg_backend;
    if (!prefs.fg_enabled && fg.vendor.configured_mode == RSF_FG_OFF) stats.fg_backend = 0;
    stats.fg_backend_choices = rsf_d3d11_present_backend_choices() | RSF_OVERLAY_FG_RUNTIME_SWITCH | 1u | (1u << RSF_FG_BACKEND_AUTO);
    stats.fg_requested_mode = prefs.fg_enabled ? RSF_FG_FIXED : RSF_FG_OFF;
    stats.fg_requested_generated = prefs.generated_frames;
    stats.reflex_requested_mode = prefs.latency; stats.frame_limit_us = prefs.frame_limit_us;
    rsf_overlay_intent intent{}; intent.struct_size = sizeof(intent);
    if (!rsf_overlay_d3d12_present(device.Get(), queue.Get(), chain, &stats, &intent, Host::backend_log, &owner)) return;
    bool changed = false;
    if (intent.backend_changed) { prefs.sr_backend = intent.backend; changed = true; }
    if (intent.enabled_changed) { prefs.sr_enabled = intent.enabled; changed = true; }
    if (intent.quality_changed) { prefs.sr_quality = intent.quality; changed = true; }
    if (intent.dlss_preset_changed) { prefs.dlss_preset = intent.dlss_preset; changed = true; }
    if (intent.fg_backend_changed) {
        if (!intent.fg_backend) prefs.fg_enabled = 0;
        else { prefs.fg_backend = intent.fg_backend; prefs.fg_enabled = 1; }
        changed = true;
    }
    if (intent.fg_changed) {
        prefs.fg_enabled = intent.fg_mode != RSF_FG_OFF;
        if (intent.fg_generated) prefs.generated_frames = intent.fg_generated;
        changed = true;
    }
    if (intent.reflex_changed) { prefs.latency = intent.reflex_mode; changed = true; }
    if (intent.frame_limit_changed) { prefs.frame_limit_us = intent.frame_limit_us; changed = true; }
    if (intent.performance_hud_changed) { prefs.status_overlay = intent.performance_hud; changed = true; }
    if (changed) {
        if (rsf_rdr2_settings_apply(&prefs) == RSF_OK) cached_prefs = prefs;
        else owner.log("RSF overlay settings request refused; saved settings retained.");
    }
}
void present(void* user, void* swapchain, uint32_t interval, uint32_t flags, uint32_t begin) noexcept try
{
    auto& owner = *static_cast<Host*>(user);
    if (begin && !(flags & DXGI_PRESENT_TEST)) overlay(owner, swapchain);
    std::lock_guard lock(owner.guard);
    owner.flush_captures();
    if (!begin && !(flags & DXGI_PRESENT_TEST)) ++owner.application_presents;
    const auto result = owner.dlss_registration ? rsf_dlss_native12_registration_present(owner.dlss_registration,
        swapchain, interval, flags, begin) : owner.context ? rsf_sr12_present(owner.context, swapchain, interval, flags, begin) : RSF_BACKEND_OK;
    if (result != RSF_BACKEND_OK) {
        owner.poisoned = true;
        owner.log("RDR2 SR manual Present bookkeeping failed; native evaluation retained.");
    }
} catch (...) { static_cast<Host*>(user)->poisoned = true; }
void render_pass(void* user, void* commands, const rsf_game_render_pass* pass, uint32_t begin) noexcept try
{
    if (!pass || pass->struct_size < sizeof(*pass)) return;
    auto& owner = *static_cast<Host*>(user);
    if (pass->role == RSF_GAME_RENDER_FINAL_SCENE && !begin) owner.scene(commands, *pass);
    if (pass->role == RSF_GAME_RENDER_WINDOW && begin) owner.window(*pass);
} catch (...) { static_cast<Host*>(user)->poisoned = true; }
int32_t process_latency(void* user, const rsf_game_latency_event* event) noexcept try
{
    auto& owner = *static_cast<Host*>(user);
    if (!event || event->struct_size < sizeof(*event) || event->session_id != owner.session) return RSF_ERROR_INVALID_ARGUMENT;
    {
        std::lock_guard lock(owner.guard); owner.settings();
    }
    // Native graphics Apply also sends driver sleep options before gameplay has
    // produced its first CPU frame. RSF owns that request as soon as its host is
    // installed; the game preference must not overwrite the overlay's latency mode.
    if (owner.fg_installed && event->marker == RSF_GAME_LATENCY_MODE) return RSF_OK;
    const auto result = owner.fg_installed ? rsf_fg12_latency(event) : RSF_BACKEND_ERROR_NOT_READY;
    if (result == RSF_BACKEND_OK && owner.latency_reports++ < 12) {
        char message[112]{}; std::snprintf(message, sizeof(message), "RDR2 latency boundary=%u source=%llu thread=%lu owned=1.",
            event->marker, static_cast<unsigned long long>(event->source_frame_id), GetCurrentThreadId());
        owner.log(message);
    }
    return result;
} catch (...) { return RSF_ERROR_NOT_READY; }

void render_feedback(void* user, const rsf_game_render_feedback* event) noexcept
{
    auto& owner = *static_cast<Host*>(user);
    if (!event || event->struct_size < sizeof(*event) || event->session_id != owner.session) return;
    std::lock_guard lock(owner.guard);
    owner.renderer_reason = event->reason; owner.feedback_tick = GetTickCount64();
    if (event->native_frame) owner.native_frame = event->native_frame;
}
}
extern "C" uint32_t rsf_rdr2_overlay_visible() { return rsf_overlay_native12_visible(); }
extern "C" void rsf_rdr2_overlay_toggle() { rsf_overlay_native12_toggle(); }
extern "C" int32_t rsf_rdr2_sr_host_create(const wchar_t* configuration, const wchar_t* directory,
    rsf_game_host_services* services) try
{
    if (!configuration || !directory || !services || services->struct_size < sizeof(*services) ||
        !services->session_id) return RSF_ERROR_INVALID_ARGUMENT;
    if (services->abi_version != RSF_GAME_ABI_VERSION) return RSF_ERROR_ABI_MISMATCH;
    auto owner = std::make_unique<Host>();
    owner->ini = configuration; owner->root = utf8(directory);
    owner->overlay_probe = GetPrivateProfileIntW(L"RDR2",L"OverlayProbe",0,configuration)!=0;
    wchar_t capture_path[32768]{};
    GetPrivateProfileStringW(L"RDR2", L"SRCaptureDirectory", L"", capture_path, 32768, configuration);
    owner->capture_directory = utf8(capture_path);
    owner->streamline = owner->root + "/streamline"; owner->interposer = owner->streamline + "/sl.interposer.dll";
    owner->fsr = owner->root + "/fidelityfx"; owner->xess = owner->root + "/xess";
    owner->session = services->session_id; owner->logger = services->log; owner->logger_user = services->user;
    // A diagnostic request belongs to this one process, never the next game launch.
    if (owner->overlay_probe && !WritePrivateProfileStringW(L"RDR2",L"OverlayProbe",L"0",configuration))
        owner->log("Could not clear the one-shot overlay probe setting.");
    rsf_fg12_setup fg{};
    fg.struct_size = sizeof(fg); fg.backend = RSF_FG_BACKEND_AUTO; fg.max_generated_frames = UINT32_MAX;
    fg.runtime_directory_utf8 = fg.fsr3_directory_utf8 = fg.fsr4_directory_utf8 = owner->fsr.c_str();
    fg.xess_directory_utf8 = owner->xess.c_str(); fg.streamline_directory_utf8 = owner->streamline.c_str();
    fg.options = {sizeof(fg.options), RSF_FG_OFF, 1, RSF_REFLEX_ON, 0, 0};
    fg.depth_inverted = 1; fg.units_to_meters = 1; fg.runtime_switching = 1; fg.ui_mode = RSF_UI_MODE_BACKBUFFER_HUDLESS;
    fg.engine_type = RSF_DLSS_ENGINE_CUSTOM; fg.engine_version_utf8 = "RAGE 1.0.1491.50";
    fg.project_id_utf8 = "6fd496e2-e0fd-48dc-9e99-0c37e87d30ca";
    fg.log = Host::backend_log; fg.user = owner.get(); fg.accept_window = Host::accept_window;
    owner->fg_installed = rsf_fg12_install(&fg) == RSF_BACKEND_OK;
    if (owner->fg_installed && !owner->capture_directory.empty())
        (void)rsf_fg12_capture_outputs(owner->capture_directory.c_str());
    owner->log(owner->fg_installed ? "RDR2 shared FG presentation installed before graphics; generation starts Off." :
        "RDR2 FG presentation interception refused; native presentation retained.");
    services->process_upscale = process; services->submit = submit; services->present = present;
    services->render_pass = render_pass;
    services->process_latency = process_latency;
    services->render_feedback = render_feedback;
    frontend_host.store(owner.get(), std::memory_order_release);
    services->user = owner.release();
    // Existing log callback retains its original user argument through the runtime owner.
    services->log = Host::backend_log;
    return RSF_OK;
} catch (...) { return RSF_ERROR_NOT_READY; }

extern "C" int32_t rsf_rdr2_status_get(rsf_rdr2_status* output) try
{
    if (!output || output->struct_size < sizeof(*output)) return RSF_ERROR_INVALID_ARGUMENT;
    if (output->abi_version != RSF_RDR2_FRONTEND_ABI_VERSION) return RSF_ERROR_ABI_MISMATCH;
    auto* owner = frontend_host.load(std::memory_order_acquire);
    if (!owner) return RSF_ERROR_NOT_READY;
    rsf_native_fg_status fg{}; fg.struct_size = sizeof(fg);
    if (!rsf_fg12_status_try(&fg)) return RSF_ERROR_BUSY;
    std::unique_lock lock(owner->guard, std::try_to_lock);
    if (!lock.owns_lock()) return RSF_ERROR_BUSY;
    const auto now = GetTickCount64();
    const uint32_t backend = rsf_d3d11_present_backend();
    const bool valid = fg.available && (fg.vendor.valid_statistics & RSF_FG_STAT_TOTAL_PRESENTED);
    if (!owner->rate_tick || owner->rate_backend != backend || fg.vendor.total_presented < owner->rate_output) {
        owner->rate_tick = now; owner->rate_base = owner->application_presents;
        owner->rate_output = fg.vendor.total_presented; owner->rate_backend = backend;
        owner->base_fps = owner->output_fps = 0; owner->output_fps_valid = false;
    } else if (now - owner->rate_tick >= 500) {
        const float seconds = static_cast<float>(now - owner->rate_tick) / 1000;
        owner->base_fps = static_cast<float>(owner->application_presents - owner->rate_base) / seconds;
        owner->output_fps = valid ? static_cast<float>(fg.vendor.total_presented - owner->rate_output) / seconds : 0;
        owner->output_fps_valid = valid;
        owner->rate_tick = now; owner->rate_base = owner->application_presents; owner->rate_output = fg.vendor.total_presented;
    }
    rsf_rdr2_status value{}; value.struct_size = sizeof(value); value.abi_version = RSF_RDR2_FRONTEND_ABI_VERSION;
    value.sr_enabled = owner->enabled; value.sr_requested_backend = owner->backend;
    value.sr_effective_backend = owner->enabled && !owner->renderer_reason && owner->last_dispatch == 0 &&
        now - owner->feedback_tick < 1000 ? owner->context_backend : 0;
    value.render_width = owner->width; value.render_height = owner->height;
    value.output_width = owner->output_width; value.output_height = owner->output_height;
    value.fg_requested_backend = GetPrivateProfileIntW(L"RDR2", L"FGBackend", RSF_FG_BACKEND_AUTO, owner->ini.c_str());
    value.fg_effective_backend = backend;
    value.fg_requested = GetPrivateProfileIntW(L"RDR2", L"FG", 0, owner->ini.c_str()) != 0;
    if (!value.fg_requested) {
        value.fg_requested_backend = 0;
        if (fg.vendor.configured_mode == RSF_FG_OFF) value.fg_effective_backend = 0;
    }
    value.fg_active = fg.vendor.active;
    value.requested_generated = GetPrivateProfileIntW(L"RDR2", L"FGGeneratedFrames", 1, owner->ini.c_str());
    value.effective_generated = fg.vendor.effective_generated_frames;
    value.max_generated = fg.vendor.max_generated_frames;
    DWORD foreground{}; GetWindowThreadProcessId(GetForegroundWindow(), &foreground); value.foreground = foreground == GetCurrentProcessId();
    value.latency_requested = GetPrivateProfileIntW(L"RDR2", L"Latency", 1, owner->ini.c_str());
    value.latency_effective = fg.vendor.effective_reflex;
    value.renderer_reason = owner->renderer_reason; value.fg_reason = fg.reason;
    value.sr_result = owner->last_dispatch; value.fg_result = fg.last_result; value.fg_vendor_result = fg.vendor.vendor_status;
    value.output_fps_valid = owner->output_fps_valid && valid; value.status_overlay = owner->status_overlay;
    value.sr_backend_choices = (1u << RSF_SR_DLSS) | (1u << RSF_SR_FSR3) | (1u << RSF_SR_FSR4) | (1u << RSF_SR_XESS);
    value.fg_backend_choices = rsf_d3d11_present_backend_choices();
    value.base_fps = owner->base_fps; value.output_fps = owner->output_fps;
    value.native_frame = owner->native_frame; value.source_frame = fg.source_frame_id; value.tagged_frames = fg.tagged_frames;
    value.application_presents = owner->application_presents; value.output_presents = fg.vendor.total_presented;
    const char* reason = "Waiting for renderer";
    if (!value.fg_requested) reason = "Disabled by setting";
    else if (!value.sr_enabled) reason = "RSF SR input path is disabled";
    else if (owner->poisoned) reason = "GPU submission or presentation failed";
    else if (owner->feedback_tick && now - owner->feedback_tick < 1000 && owner->renderer_reason) {
        const char* reasons[] = {"", "Unsupported game inputs", "Unsupported input rectangle", "Camera unavailable",
            "Unsupported output", "Invalid camera or frozen jitter", "Presentation queue unavailable", "SR backend refused frame"};
        reason = owner->renderer_reason < std::size(reasons) ? reasons[owner->renderer_reason] : "Renderer refused frame";
    } else if (!fg.available) reason = "FG presentation unavailable";
    else if (fg.last_result) reason = "FG backend error";
    else if (fg.reason == RSF_NATIVE_FG_REASON_RESET) reason = "Resetting temporal history";
    else if (fg.reason == RSF_NATIVE_FG_REASON_SCENE_UNSUPPORTED) reason = "Scene is not eligible";
    else if (fg.reason == RSF_NATIVE_FG_REASON_HUDLESS_MISSING) reason = "HUD-less scene missing";
    else if (fg.reason) reason = "Frame inputs or submission not matched";
    else if (fg.vendor.active) reason = "Active";
    else if (!value.foreground) reason = "Game is in background";
    else reason = "SDK reports inactive";
    std::snprintf(value.reason, sizeof(value.reason), "%s", reason); *output = value;
    return RSF_OK;
} catch (...) { return RSF_ERROR_NOT_READY; }

extern "C" int32_t rsf_rdr2_settings_get(rsf_rdr2_settings* output) try
{
    if (!output || output->struct_size < sizeof(*output)) return RSF_ERROR_INVALID_ARGUMENT;
    if (output->abi_version != RSF_RDR2_FRONTEND_ABI_VERSION) return RSF_ERROR_ABI_MISMATCH;
    auto* owner = frontend_host.load(std::memory_order_acquire); if (!owner) return RSF_ERROR_NOT_READY;
    std::unique_lock lock(owner->guard, std::try_to_lock);
    if (!lock.owns_lock()) return RSF_ERROR_BUSY;
    auto read = [&](const wchar_t* key, uint32_t fallback) { return GetPrivateProfileIntW(L"RDR2", key, fallback, owner->ini.c_str()); };
    *output = {sizeof(*output), RSF_RDR2_FRONTEND_ABI_VERSION, read(L"SR",1), read(L"SRBackend",1), read(L"DLSSPreset",0),
        read(L"FG",0) != 0, read(L"FGBackend",7), read(L"FGGeneratedFrames",1), read(L"Latency",1), read(L"FrameLimitUs",0), read(L"StatusOverlay",1), read(L"SRQuality",2)};
    return RSF_OK;
} catch (...) { return RSF_ERROR_NOT_READY; }

extern "C" int32_t rsf_rdr2_settings_apply(const rsf_rdr2_settings* request) try
{
    if (!request || request->struct_size < sizeof(*request)) return RSF_ERROR_INVALID_ARGUMENT;
    if (request->abi_version != RSF_RDR2_FRONTEND_ABI_VERSION) return RSF_ERROR_ABI_MISMATCH;
    auto* owner = frontend_host.load(std::memory_order_acquire); if (!owner) return RSF_ERROR_NOT_READY;
    if (request->sr_enabled > 1 || request->fg_enabled > 1 || request->status_overlay > 1 || request->latency > 2 || request->sr_quality > 4 ||
        !request->generated_frames || request->generated_frames > 15 || !rsf_dlss_preset_valid(request->dlss_preset) ||
        (request->sr_backend != 1 && request->sr_backend != 3 && request->sr_backend != 4 && request->sr_backend != 5) ||
        (request->fg_backend != 1 && request->fg_backend != 3 && request->fg_backend != 4 && request->fg_backend != 5 && request->fg_backend != 7))
        return RSF_ERROR_INVALID_ARGUMENT;
    std::unique_lock lock(owner->guard, std::try_to_lock);
    if (!lock.owns_lock()) return RSF_ERROR_BUSY;
    const wchar_t* keys[] = {L"SR",L"SRBackend",L"DLSSPreset",L"FG",L"FGBackend",L"FGGeneratedFrames",L"Latency",L"FrameLimitUs",L"StatusOverlay",L"SRQuality"};
    const uint32_t values[] = {request->sr_enabled,request->sr_backend,request->dlss_preset,request->fg_enabled,
        request->fg_backend,request->generated_frames,request->latency,request->frame_limit_us,request->status_overlay,request->sr_quality};
    wchar_t section[32768]{}; const auto length = GetPrivateProfileSectionW(L"RDR2", section, 32768, owner->ini.c_str());
    if (length >= 32766) return RSF_ERROR_INVALID_ARGUMENT;
    std::vector<std::wstring> entries;
    for (const wchar_t* entry = section; *entry; entry += std::wcslen(entry) + 1) {
        bool owned = false;
        for (const auto* key : keys) if (!_wcsnicmp(entry,key,std::wcslen(key)) && entry[std::wcslen(key)] == L'=') owned = true;
        if (!owned) entries.emplace_back(entry);
    }
    std::wstring updated;
    for (const auto& entry : entries) { updated += entry; updated += L'\0'; }
    for (size_t i = 0; i < std::size(keys); ++i) { updated += keys[i]; updated += L'='; updated += std::to_wstring(values[i]); updated += L'\0'; }
    updated += L'\0';
    if (!WritePrivateProfileSectionW(L"RDR2", updated.c_str(), owner->ini.c_str())) return RSF_ERROR_NOT_READY;
    owner->poll_tick = 0; owner->settings();
    return RSF_OK;
} catch (...) { return RSF_ERROR_NOT_READY; }
