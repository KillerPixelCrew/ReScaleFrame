// SPDX-License-Identifier: GPL-3.0-only
#include "package.h"
#include "bridge.h"
#include "vulkan_bridge.h"
#include "preview.h"
#include <rescaleframe/native_sr_d3d12.h>
#include <rescaleframe/native_fg_d3d12.h>
#include <rescaleframe/gpu_policy.h>
#include <rescaleframe/unity_config.h>
#include <rescaleframe/fg_choice.h>
#include <windows.h>
#include <d3d12.h>
#include <dxgi1_4.h>
#include <IUnityGraphics.h>
#include <IUnityGraphicsD3D12.h>
#include <wrl/client.h>
#include <array>
#include <mutex>
#include <string>
#include <cstdio>
#include <cstring>
#include <unordered_map>
#include <atomic>

extern "C" void UNITY_INTERFACE_API rsf_unity_package_bridge_load(IUnityInterfaces*);
extern "C" void UNITY_INTERFACE_API rsf_unity_package_bridge_unload();
extern "C" int rsf_unity_set_engine_spatial(uint32_t, uint32_t) noexcept;

namespace {
using Microsoft::WRL::ComPtr;
std::mutex guard;
rsf_unity_package_settings settings{sizeof(settings), 1, RSF_SR_AUTO, 1, 0, 7, 0, 1, 0, 0, 1};
rsf_unity_package_status status{sizeof(status), 1};
rsf_game_render_config plan{sizeof(plan)};
rsf_unity_native_api api{};
std::string root, streamline, fsr, xess, version, project;
std::wstring ini;
bool started = false, changed = false, editor = false;
bool presentation_installed = false;
uint32_t default_fg_backend = 0;
std::atomic<bool> temporal_ready{false};
std::atomic<int32_t> bridge_result{0};
IUnityInterfaces* registry = nullptr;
IUnityGraphics* graphics = nullptr;
HANDLE log_file = INVALID_HANDLE_VALUE;
std::mutex logging;
HWND input_window = nullptr;
WNDPROC previous_proc = nullptr;
std::atomic<uint32_t> input_kinds{0}, ping_message{0};
std::atomic<bool> ping_pending{false};
LRESULT CALLBACK input_proc(HWND hwnd, UINT message, WPARAM wparam, LPARAM lparam)
{
    if (message == ping_message.load() && message >= 0xc000u) ping_pending.store(true);
    if (message == WM_KEYDOWN || message == WM_SYSKEYDOWN || message == WM_KEYUP || message == WM_SYSKEYUP) input_kinds.fetch_or(1);
    if ((message >= WM_MOUSEFIRST && message <= WM_MOUSELAST)) input_kinds.fetch_or(2);
    if (message == WM_INPUT) {
        RAWINPUTHEADER header{}; UINT bytes = sizeof(header);
        if (GetRawInputData(reinterpret_cast<HRAWINPUT>(lparam), RID_HEADER, &header, &bytes, sizeof(header)) != UINT(-1))
            input_kinds.fetch_or(header.dwType == RIM_TYPEKEYBOARD ? 1u : header.dwType == RIM_TYPEMOUSE ? 2u : 4u);
    }
    return previous_proc ? CallWindowProcW(previous_proc, hwnd, message, wparam, lparam) : DefWindowProcW(hwnd, message, wparam, lparam);
}
BOOL CALLBACK find_input_window(HWND hwnd, LPARAM)
{
    DWORD process = 0; wchar_t name[64]{}; GetWindowThreadProcessId(hwnd, &process);
    if (process != GetCurrentProcessId() || !GetClassNameW(hwnd, name, 64) || std::wcscmp(name, L"UnityWndClass") != 0) return TRUE;
    input_window = hwnd;
    previous_proc = reinterpret_cast<WNDPROC>(SetWindowLongPtrW(hwnd, GWLP_WNDPROC, reinterpret_cast<LONG_PTR>(input_proc)));
    return FALSE;
}
struct View {
    rsf_sr12* sr = nullptr;
    uint32_t width = 0, height = 0, render_width = 0, render_height = 0, backend = 0;
    std::array<void*, 3> lists{};
};
std::unordered_map<uint64_t, View> views;
void log(void*, const char* text)
{
    if (!text) return;
    std::lock_guard<std::mutex> lock(logging);
    OutputDebugStringA(text); OutputDebugStringA("\n");
    if (log_file != INVALID_HANDLE_VALUE) {
        DWORD written = 0;
        WriteFile(log_file, text, static_cast<DWORD>(std::strlen(text)), &written, nullptr);
        WriteFile(log_file, "\r\n", 2, &written, nullptr);
    }
}
void paths(const std::string& path)
{
    root = path; streamline = root + "/streamline"; fsr = root + "/fidelityfx"; xess = root + "/xess";
}
int accept(void*, void* hwnd)
{
    if (editor) return rsf_unity_preview_owns(hwnd) ? 1 : 0;
    if (graphics && graphics->GetRenderer() == kUnityGfxRendererVulkan) return rsf_uvk_owns_window(hwnd) ? 1 : 0;
    DWORD process = 0; wchar_t name[64]{};
    GetWindowThreadProcessId(static_cast<HWND>(hwnd), &process);
    return !editor && process == GetCurrentProcessId() && GetClassNameW(static_cast<HWND>(hwnd), name, 64) &&
        (std::wcscmp(name, L"UnityWndClass") == 0 || rsf_uvk_owns_window(hwnd));
}
int configuration(void*, rsf_game_render_config* output)
{
    if (!output || output->struct_size < sizeof(*output)) return 0;
    std::lock_guard<std::mutex> lock(guard); *output = plan; return 1;
}
void release_views()
{
    temporal_ready.store(false); status.active_sr = 0;
    for (auto& item : views) rsf_sr12_destroy(item.second.sr);
    views.clear();
}
void request_generation()
{
    if (!rsf_d3d11_present_has_owner()) return;
    uint32_t requested = 0;
    { std::lock_guard<std::mutex> lock(guard); requested = settings.fg_backend; }
    if (requested == RSF_FG_BACKEND_AUTO) {
        if (!default_fg_backend) {
            void* owning_device = nullptr; void* owning_queue = nullptr;
            ComPtr<IDXGIFactory4> factory; ComPtr<IDXGIAdapter1> adapter; DXGI_ADAPTER_DESC1 desc{};
            if (!rsf_unity_package_graphics(&owning_device, &owning_queue) ||
                FAILED(CreateDXGIFactory1(IID_PPV_ARGS(&factory))) ||
                FAILED(factory->EnumAdapterByLuid(static_cast<ID3D12Device*>(owning_device)->GetAdapterLuid(), IID_PPV_ARGS(&adapter))) ||
                FAILED(adapter->GetDesc1(&desc))) return;
            default_fg_backend = rsf_fg_default_backend(desc.VendorId, (desc.Flags & DXGI_ADAPTER_FLAG_SOFTWARE) != 0);
        }
        requested = default_fg_backend;
    }
    if (requested == rsf_d3d11_present_backend()) return;
    const auto result = rsf_d3d11_present_request(requested);
    std::lock_guard<std::mutex> lock(guard); status.fg_result = result;
}
void render(void*, void* commands, const rsf_game_render_pass* original, uint32_t begin)
{
    if (!commands || !original) return;
    auto pass = *original;
    if (!begin) {
        if (pass.role != RSF_GAME_RENDER_WINDOW) rsf_fg12_submitted(pass.session_id, pass.source_frame_id);
        return;
    }
    if (pass.role == RSF_GAME_RENDER_WINDOW) {
        request_generation();
        if (editor) {
            const bool shown = rsf_unity_preview_present(pass.color_input, pass, (pass.flags & RSF_UNITY_PREVIEW_SOURCE_COMMON) != 0);
            static uint32_t reports = 0;
            if (!shown && reports++ < 4 && pass.color_input) {
                const auto desc = static_cast<ID3D12Resource*>(pass.color_input)->GetDesc();
                char text[160]; std::snprintf(text, sizeof(text), "Editor presentation pending: texture=%llux%u format=%u expected=%ux%u",
                    desc.Width, desc.Height, desc.Format, pass.camera.output_width, pass.camera.output_height); log(nullptr, text);
            }
        }
        else rsf_fg12_window(&pass);
        return;
    }
    if (pass.role == RSF_GAME_RENDER_FINAL_SCENE) { rsf_fg12_hudless(commands, &pass); return; }
    if (editor) {
        void* owning_device = nullptr; void* owning_queue = nullptr;
        if (!rsf_unity_package_graphics(&owning_device, &owning_queue) ||
            !rsf_unity_preview_prepare(owning_device, owning_queue, pass.camera.output_width, pass.camera.output_height)) return;
    }
    std::lock_guard<std::mutex> lock(guard);
    pass.screen = settings.gameplay ? RSF_SCREEN_FLIGHT : RSF_SCREEN_MENU;
    if (pass.flags & RSF_GAME_RENDER_RECONFIGURE) { release_views(); changed = false; }
    auto& view = views[pass.view_key];
    const auto& camera = pass.camera;
    if (view.sr && (view.width != camera.output_width || view.height != camera.output_height ||
        (view.backend == 0 && (view.render_width != camera.render_width || view.render_height != camera.render_height)))) {
        rsf_sr12_destroy(view.sr); view = {};
    }
    if (!view.sr) {
        auto* list = static_cast<ID3D12GraphicsCommandList*>(commands);
        ComPtr<ID3D12Device> device;
        if (FAILED(list->GetDevice(IID_PPV_ARGS(&device)))) return;
        uint32_t backend = settings.sr_backend;
        if (backend == RSF_SR_AUTO) {
            ComPtr<IDXGIFactory4> factory; ComPtr<IDXGIAdapter1> adapter; DXGI_ADAPTER_DESC1 desc{};
            if (FAILED(CreateDXGIFactory1(IID_PPV_ARGS(&factory))) ||
                FAILED(factory->EnumAdapterByLuid(device->GetAdapterLuid(), IID_PPV_ARGS(&adapter))) ||
                FAILED(adapter->GetDesc1(&desc))) return;
            backend = rsf_gpu_default_sr_backend(desc.VendorId, (desc.Flags & DXGI_ADAPTER_FLAG_SOFTWARE) != 0);
        }
        if (backend == RSF_SR_SPATIAL) backend = 0;
        rsf_sr12_setup setup{}; setup.struct_size = sizeof(setup); setup.abi_version = 1;
        setup.device = device.Get(); setup.backend = backend; setup.quality = settings.quality;
        setup.output_width = camera.output_width; setup.output_height = camera.output_height;
        setup.render_width = camera.render_width; setup.render_height = camera.render_height;
        setup.inverted_depth = camera.depth_inverted;
        setup.streamline_host = rsf_d3d11_present_host();
        setup.fsr2_directory_utf8 = setup.fsr3_directory_utf8 = setup.fsr4_directory_utf8 = fsr.c_str();
        setup.xess_directory_utf8 = xess.c_str(); setup.log = log;
        const std::string interposer = streamline + "/sl.interposer.dll";
        setup.dlss = {sizeof(setup.dlss), RSF_DLSS_ABI_VERSION, interposer.c_str(), streamline.c_str(), root.c_str(),
            0, RSF_DLSS_ENGINE_UNITY, version.c_str(), project.c_str(), 1, log, nullptr, settings.dlss_preset};
        status.sr_result = rsf_sr12_create(&setup, &view.sr);
        if (!view.sr && backend) { setup.backend = 0; (void)rsf_sr12_create(&setup, &view.sr); backend = 0; }
        if (!view.sr) return;
        view.backend = backend; view.width = camera.output_width; view.height = camera.output_height;
        rsf_sr12_plan(view.sr, &view.render_width, &view.render_height);
        plan = {sizeof(plan), 1, view.width, view.height, view.render_width, view.render_height};
        temporal_ready.store(backend != 0);
        status.render_width = view.render_width; status.render_height = view.render_height;
        status.output_width = view.width; status.output_height = view.height;
        char text[192]; std::snprintf(text, sizeof(text), "URP SR requested=%u active=%u result=%d %ux%u -> %ux%u",
            settings.sr_backend, backend, status.sr_result, view.render_width, view.render_height, view.width, view.height);
        log(nullptr, text);
    }
    if (!pass.camera_valid || changed) return;
    uint32_t slot = 3;
    for (uint32_t i = 0; i < 3; ++i) if (view.lists[i] == commands) { slot = i; break; }
    if (slot == 3) for (uint32_t i = 0; i < 3; ++i) if (!view.lists[i]) { view.lists[i] = commands; slot = i; break; }
    if (slot == 3) return;
    const auto result = rsf_sr12_evaluate(view.sr, commands, &pass, slot);
    if (view.backend || settings.sr_backend == RSF_SR_NONE || settings.sr_backend == RSF_SR_SPATIAL) status.sr_result = result;
    if (result == RSF_BACKEND_OK) {
        ++status.evaluated_frames;
        status.active_sr = settings.sr_backend == RSF_SR_SPATIAL ? RSF_SR_SPATIAL : view.backend;
    }
    else { ++status.refused_frames; status.sr_result = result; }
}
void cpu_event(void*, const rsf_game_cpu_event* event) { rsf_fg12_cpu(event); }
void UNITY_INTERFACE_API device_event(UnityGfxDeviceEventType type)
{
    if (type == kUnityGfxDeviceEventInitialize) rsf_unity_package_bridge_load(registry);
    if (type == kUnityGfxDeviceEventShutdown) {
        rsf_unity_bridge_activate(false);
        if (rsf_unity_bridge_drained()) { release_views(); rsf_unity_bridge_release(); started = false; }
        rsf_unity_package_bridge_unload();
    }
}
void install_presentation()
{
    if (presentation_installed || root.empty() || version.empty() || project.empty()) return;
    rsf_fg12_setup setup{}; setup.struct_size = sizeof(setup); setup.backend = RSF_FG_BACKEND_AUTO;
    setup.max_generated_frames = UINT32_MAX; setup.runtime_switching = 1; setup.ui_mode = RSF_UI_MODE_BACKBUFFER_HUDLESS;
    setup.options = {sizeof(setup.options), RSF_FG_OFF, 1, RSF_REFLEX_OFF, 0, 0};
    setup.depth_inverted = 1; setup.units_to_meters = 1; setup.accept_window = accept; setup.log = log;
    setup.runtime_directory_utf8 = setup.fsr3_directory_utf8 = setup.fsr4_directory_utf8 = fsr.c_str();
    setup.xess_directory_utf8 = xess.c_str(); setup.streamline_directory_utf8 = streamline.c_str();
    setup.engine_type = RSF_DLSS_ENGINE_UNITY; setup.engine_version_utf8 = version.c_str(); setup.project_id_utf8 = project.c_str();
    status.fg_result = rsf_fg12_install(&setup); presentation_installed = status.fg_result == RSF_BACKEND_OK;
}
void preload()
{
    HMODULE module = nullptr;
    GetModuleHandleExW(GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
        reinterpret_cast<LPCWSTR>(preload), &module);
    wchar_t path[32768]{}; GetModuleFileNameW(module, path, 32768);
    ini = path; ini.resize(ini.find_last_of(L"/\\") + 1); ini += L"ReScaleFrame.Unity.ini";
    wchar_t executable[32768]{}; GetModuleFileNameW(nullptr, executable, 32768);
    const auto* leaf = std::wcsrchr(executable, L'\\');
    editor = leaf && _wcsicmp(leaf + 1, L"Unity.exe") == 0;
    paths(rsf::unity_config::utf8(rsf::unity_config::path_setting(ini.c_str(), L"Runtime")));
    wchar_t text[256]{};
    GetPrivateProfileStringW(L"UnitySR", L"EngineVersion", L"", text, 256, ini.c_str()); version = rsf::unity_config::utf8(text);
    GetPrivateProfileStringW(L"UnitySR", L"ProjectId", L"", text, 256, ini.c_str()); project = rsf::unity_config::utf8(text);
    const auto log_path = rsf::unity_config::path_setting(ini.c_str(), L"Log");
    if (!log_path.empty()) log_file = CreateFileW(log_path.c_str(), FILE_APPEND_DATA, FILE_SHARE_READ | FILE_SHARE_WRITE,
        nullptr, OPEN_ALWAYS, FILE_ATTRIBUTE_NORMAL, nullptr);
    if (!editor) install_presentation();
}
}

void rsf_unity_package_bridge_error(int32_t result, const char* message) noexcept
{
    bridge_result.store(result); temporal_ready.store(false);
    log(nullptr, message);
}
bool rsf_unity_package_is_editor() noexcept { return editor; }

extern "C" __declspec(dllexport) int rsfu_initialize(const char* runtime, const char* engine, const char* id, rsf_unity_native_api* output) noexcept try
{
    if (!runtime || !engine || !id || !output) return 0;
    std::lock_guard<std::mutex> lock(guard);
    if (started) {
        if (!rsf_unity_bridge_drained() || !rsf_fg12_end_session(api.session_id)) return 2;
        release_views(); rsf_unity_bridge_release(); started = false;
    }
    if (!started) {
        bridge_result.store(0); plan = {sizeof(plan)}; status = {sizeof(status), 1};
        paths(runtime); version = engine; project = id;
        if (log_file == INVALID_HANDLE_VALUE) {
            wchar_t local[32768]{};
            if (GetEnvironmentVariableW(L"LOCALAPPDATA", local, 32768)) {
                std::wstring directory(local); directory += L"\\ReScaleFrame"; CreateDirectoryW(directory.c_str(), nullptr);
                directory += L"\\Unity"; CreateDirectoryW(directory.c_str(), nullptr);
                const auto file = directory + L"\\" + std::to_wstring(GetCurrentProcessId()) + L".log";
                log_file = CreateFileW(file.c_str(), FILE_APPEND_DATA, FILE_SHARE_READ | FILE_SHARE_WRITE, nullptr, OPEN_ALWAYS, FILE_ATTRIBUTE_NORMAL, nullptr);
            }
        }
        if (editor) install_presentation();
        if (!editor && !input_window) EnumWindows(find_input_window, 0);
        if (graphics && graphics->GetRenderer() == kUnityGfxRendererVulkan && !editor && !rsf_uvk_create_window()) {
            log(nullptr, "Unity Vulkan presentation surface could not initialize on the window thread."); return 0;
        }
        rsf_game_host_services services{}; services.struct_size = sizeof(services); services.abi_version = RSF_GAME_ABI_VERSION;
        services.session_id = GetTickCount64() + 1; services.log = log; services.render_pass = render;
        services.render_config = configuration; services.cpu_event = cpu_event;
        if (!rsf_unity_bridge_prepare(services, &api)) return 0;
        rsf_unity_set_engine_spatial(0, settings.quality); rsf_unity_bridge_activate(true); started = true;
    }
    *output = api; return 1;
} catch (...) { return 0; }

extern "C" __declspec(dllexport) int rsfu_configure(const rsf_unity_package_settings* input) noexcept try
{
    if (!input || input->struct_size != sizeof(*input) || input->abi_version != 1 || input->sr_backend > RSF_SR_AUTO ||
        input->quality > 5 || !rsf_dlss_preset_valid(input->dlss_preset) || input->fg_mode > RSF_FG_FIXED ||
        input->latency_mode > 2 || input->generated_frames < 1 || input->generated_frames > 15 ||
        (input->fg_backend != 1 && input->fg_backend != 3 && input->fg_backend != 4 && input->fg_backend != 5 && input->fg_backend != 7)) return 0;
    std::lock_guard<std::mutex> lock(guard);
    if (input->quality == 5 && (input->sr_backend == RSF_SR_DLSS ||
        (input->sr_backend == RSF_SR_AUTO && status.active_sr == RSF_SR_DLSS))) return 0;
    const bool sr_change = settings.sr_backend != input->sr_backend || settings.quality != input->quality || settings.dlss_preset != input->dlss_preset;
    if (sr_change) { rsf_unity_set_engine_spatial(input->sr_backend == RSF_SR_SPATIAL, input->quality); changed = true; plan.enabled = 0; temporal_ready.store(false); }
    settings = *input; status.requested_sr = settings.sr_backend;
    rsf_native_fg_options options{sizeof(options), settings.fg_mode, settings.generated_frames, settings.latency_mode, settings.frame_limit_us, 0};
    rsf_fg12_options(&options);
    return 1;
} catch (...) { return 0; }

extern "C" __declspec(dllexport) int rsfu_temporal_ready() noexcept { return temporal_ready.load() ? 1 : 0; }
extern "C" __declspec(dllexport) int rsfu_surface(uint32_t width, uint32_t height) noexcept
{
    return rsf_uvk_update_window(width, height) ? 1 : 0;
}
extern "C" __declspec(dllexport) int rsfu_preview_region(int left, int top, int width, int height, uint32_t visible) noexcept
{
    const bool configured = rsf_unity_preview_region(left, top, width, height, visible != 0);
    if (editor && !input_window && rsf_unity_preview_parent()) {
        input_window = static_cast<HWND>(rsf_unity_preview_parent());
        previous_proc = reinterpret_cast<WNDPROC>(SetWindowLongPtrW(input_window, GWLP_WNDPROC, reinterpret_cast<LONG_PTR>(input_proc)));
    }
    return configured ? 1 : 0;
}
extern "C" __declspec(dllexport) int rsfu_frame_counters(rsf_unity_frame_counters* output) noexcept
{
    if (!output || output->struct_size != sizeof(*output)) return 0;
    rsf_native_fg_status fg{}; fg.struct_size = sizeof(fg); rsf_fg12_status(&fg);
    *output = {sizeof(*output), 1, fg.vendor.valid_statistics & RSF_FG_STAT_TOTAL_PRESENTED ? 1u : 0u, fg.vendor.active,
        fg.vendor.total_presented, fg.vendor.generated_presented, rsf_d3d11_present_generation()};
    return 1;
}

extern "C" __declspec(dllexport) int rsfu_status(rsf_unity_package_status* output) noexcept try
{
    if (!output || output->struct_size != sizeof(*output)) return 0;
    std::lock_guard<std::mutex> lock(guard);
    rsf_native_fg_status fg{}; fg.struct_size = sizeof(fg); rsf_fg12_status(&fg);
    status.fg_backend = rsf_d3d11_present_backend(); status.fg_available = fg.available; status.fg_active = fg.vendor.active;
    status.max_generated_frames = fg.vendor.max_generated_frames; status.generated_frames = fg.vendor.effective_generated_frames;
    status.latency_available = fg.vendor.low_latency_available; status.latency_mode = fg.vendor.effective_reflex;
    status.reason = fg.reason; status.tagged_frames = fg.tagged_frames;
    status.fg_result = fg.last_result ? fg.last_result : rsf_d3d11_present_switch_result();
    if (bridge_result.load()) { status.sr_result = bridge_result.load(); status.active_sr = 0; }
    rsf_streamline_latency_status latency{}; latency.struct_size = sizeof(latency);
    if (rsf_streamline_host_latency_status(rsf_d3d11_present_host(), &latency) == RSF_BACKEND_OK) {
        status.sleep_calls = latency.sleep_calls; status.marker_calls = latency.marker_calls;
        status.pcl_available = latency.pcl_message_id >= 0xc000u;
        status.latency_report_available = latency.latency_report_available;
    }
    *output = status; return 1;
} catch (...) { return 0; }

extern "C" __declspec(dllexport) int rsfu_input(uint64_t frame, uint32_t kinds) noexcept try
{
    if (!frame || (kinds & ~7u)) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    rsf_streamline_latency_status latency{}; latency.struct_size = sizeof(latency);
    if (rsf_streamline_host_latency_status(rsf_d3d11_present_host(), &latency) == RSF_BACKEND_OK)
        ping_message.store(latency.pcl_message_id);
    const uint32_t pending = input_kinds.exchange(0) | kinds;
    const uint32_t ping = ping_pending.exchange(false) ? ping_message.load() : 0u;
    if (!pending && !ping) return RSF_BACKEND_OK;
    return rsf_d3d11_present_input(frame, pending, ping);
} catch (...) { return RSF_BACKEND_ERROR_FEATURE_FAILED; }

extern "C" __declspec(dllexport) int rsfu_shutdown() noexcept try
{
    if (editor) rsf_unity_preview_hide();
    rsf_unity_bridge_activate(false);
    if (!rsf_unity_bridge_drained() || !rsf_fg12_end_session(api.session_id)) return 0;
    std::lock_guard<std::mutex> lock(guard);
    release_views(); rsf_unity_bridge_release(); started = false; return 1;
} catch (...) { return 0; }

extern "C" void UNITY_INTERFACE_EXPORT UNITY_INTERFACE_API UnityPluginLoad(IUnityInterfaces* interfaces)
{
    registry = interfaces; graphics = interfaces ? interfaces->Get<IUnityGraphics>() : nullptr;
    if (interfaces) rsf_uvk_preload(interfaces);
    preload();
    if (graphics) { graphics->RegisterDeviceEventCallback(device_event); device_event(kUnityGfxDeviceEventInitialize); }
}
extern "C" void UNITY_INTERFACE_EXPORT UNITY_INTERFACE_API UnityPluginUnload()
{
    if (graphics) graphics->UnregisterDeviceEventCallback(device_event);
    device_event(kUnityGfxDeviceEventShutdown);
    if (input_window && previous_proc && reinterpret_cast<WNDPROC>(GetWindowLongPtrW(input_window, GWLP_WNDPROC)) == input_proc)
        SetWindowLongPtrW(input_window, GWLP_WNDPROC, reinterpret_cast<LONG_PTR>(previous_proc));
    input_window = nullptr; previous_proc = nullptr;
    if (log_file != INVALID_HANDLE_VALUE) { CloseHandle(log_file); log_file = INVALID_HANDLE_VALUE; }
    registry = nullptr; graphics = nullptr;
}
