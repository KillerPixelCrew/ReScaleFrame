#include <rescaleframe/native_cpu.h>
// SPDX-License-Identifier: MIT
// The settings, status snapshot, render plan, temporal readiness, pending reconfiguration and
// session lifecycle are Rust (games/unity-mono/src/package_state.rs, through package_state.h).
// This file keeps the SR views, the Unity, DXGI, Vulkan and preview work, the input window and
// the log, and lends the typed SDK and Runtime calls through rsf_unity_package_platform. Its
// mutex still serializes the exports and the render path where it did.
#include "package.h"
#include "bridge.h"
#include "vulkan_bridge.h"
#include "preview.h"
#include <rescaleframe/native_sr_d3d12.h>
#include <rescaleframe/native_fg_d3d12.h>
#include <rescaleframe/gpu_policy.h>
#include <rescaleframe/unity_runtime_config.h>
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

// package_abi.rs mirrors these layouts and constants.
static_assert(sizeof(rsf_unity_package_settings) == 100 && sizeof(rsf_unity_package_status) == 184);
static_assert(sizeof(rsf_unity_frame_counters) == 40 && sizeof(rsf_unity_package_platform) == 120);
static_assert(sizeof(rsf_native_fg_status) == 352 && sizeof(rsf_native_latency_status) == 152);
static_assert(sizeof(rsf_native_fg_options) == 24 && RSF_BACKEND_OK == 0);
static_assert(RSF_FG_STAT_TOTAL_PRESENTED == 1 && RSF_SR_NONE == 0);

namespace {
using Microsoft::WRL::ComPtr;
std::mutex guard;
rsf_unity_native_api api{};
std::string root, streamline, fsr, xess, version, project;
std::wstring ini;
bool editor = false;
bool presentation_installed = false;
uint32_t default_fg_backend = 0;
IUnityInterfaces* registry = nullptr;
IUnityGraphics* graphics = nullptr;
HANDLE log_file = INVALID_HANDLE_VALUE;
std::mutex logging;
HWND input_window = nullptr;
WNDPROC previous_proc = nullptr;
std::atomic<uint64_t> input_frame{0};
LRESULT CALLBACK input_proc(HWND hwnd, UINT message, WPARAM wparam, LPARAM lparam)
{
    uint32_t kind=0;
    if (message>=WM_KEYFIRST && message<=WM_KEYLAST) kind=RSF_GAME_INPUT_KEYBOARD;
    if (message>=WM_MOUSEFIRST && message<=WM_MOUSELAST) kind=RSF_GAME_INPUT_MOUSE;
    if (message==WM_INPUT) {
        RAWINPUTHEADER header{}; UINT bytes=sizeof(header);
        if (GetRawInputData(reinterpret_cast<HRAWINPUT>(lparam),RID_HEADER,&header,&bytes,sizeof(header))==sizeof(header))
            kind=header.dwType==RIM_TYPEKEYBOARD ? RSF_GAME_INPUT_KEYBOARD : header.dwType==RIM_TYPEMOUSE ? RSF_GAME_INPUT_MOUSE : 0;
    }
    LARGE_INTEGER now{},frequency{}; QueryPerformanceCounter(&now); QueryPerformanceFrequency(&frequency);
    const auto source=input_frame.load();
    rsf_game_cpu_event fact{sizeof(fact),RSF_GAME_CPU_INPUT_EVENT,api.session_id,source,uint64_t(now.QuadPart),
        uint64_t(frequency.QuadPart),0,kind,message,RSF_GAME_CPU_EVENT_VERSION,
        RSF_GAME_CPU_INPUT_KEYED | RSF_GAME_CPU_WINDOW_MESSAGE | (!source && kind ? RSF_GAME_CPU_INPUT_UNFRAMED : 0u)};
    rsf_fg12_cpu(&fact);
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
    std::lock_guard<std::mutex> lock(guard); return rsf_unity_package_plan(output);
}
void destroy_views() noexcept
{
    for (auto& item : views) rsf_sr12_destroy(item.second.sr);
    views.clear();
}
void request_generation()
{
    if (!rsf_d3d11_present_has_owner()) return;
    uint32_t requested = 0;
    {
        std::lock_guard<std::mutex> lock(guard);
        rsf_unity_package_settings current{}; rsf_unity_package_current(&current); requested = current.fg_backend;
    }
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
    std::lock_guard<std::mutex> lock(guard); rsf_unity_package_fg_result(result);
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
    rsf_unity_package_settings settings{}; rsf_unity_package_current(&settings);
    pass.screen = settings.gameplay ? RSF_SCREEN_FLIGHT : RSF_SCREEN_MENU;
    if (pass.flags & RSF_GAME_RENDER_RECONFIGURE) { rsf_unity_package_reconfigured(); destroy_views(); }
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
        const int32_t created = rsf_sr12_create(&setup, &view.sr);
        rsf_unity_package_sr_created(created);
        if (!view.sr && backend) { setup.backend = 0; (void)rsf_sr12_create(&setup, &view.sr); backend = 0; }
        if (!view.sr) return;
        view.backend = backend; view.width = camera.output_width; view.height = camera.output_height;
        rsf_sr12_plan(view.sr, &view.render_width, &view.render_height);
        rsf_unity_package_view_planned(backend, view.width, view.height, view.render_width, view.render_height);
        char text[192]; std::snprintf(text, sizeof(text), "URP SR requested=%u active=%u result=%d %ux%u -> %ux%u",
            settings.sr_backend, backend, created, view.render_width, view.render_height, view.width, view.height);
        log(nullptr, text);
    }
    if (!pass.camera_valid || rsf_unity_package_pending()) return;
    uint32_t slot = 3;
    for (uint32_t i = 0; i < 3; ++i) if (view.lists[i] == commands) { slot = i; break; }
    if (slot == 3) for (uint32_t i = 0; i < 3; ++i) if (!view.lists[i]) { view.lists[i] = commands; slot = i; break; }
    if (slot == 3) return;
    auto sharp=settings.sharpening.struct_size?settings.sharpening:rsf_sharpening_defaults();
    (void)rsf_sr12_set_sharpening(view.sr,&sharp);
    const auto result = rsf_sr12_evaluate(view.sr, commands, &pass, slot);
    rsf_unity_package_evaluated(result, view.backend);
}
void cpu_event(void*, const rsf_game_cpu_event* event) {
    if (!event) return;
    if (event->stage==RSF_GAME_CPU_FRAME_BEGIN) input_frame.store(event->source_frame_id);
    rsf_fg12_cpu(event);
    if (event->stage==RSF_GAME_CPU_FRAME_END) input_frame.store(0);
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
    const auto result = rsf_fg12_install(&setup);
    rsf_unity_package_fg_result(result); presentation_installed = result == RSF_BACKEND_OK;
}
struct Opening { const char* runtime; const char* engine; const char* id; };
// rsf_unity_package_platform::open: everything rsfu_initialize set up between forgetting the
// previous session's state and activating the bridge.
uint32_t open_session(void* user) noexcept try
{
    const auto& opening = *static_cast<const Opening*>(user);
    paths(opening.runtime); version = opening.engine; project = opening.id;
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
    return rsf_unity_bridge_prepare(services, &api) ? 1u : 0u;
} catch (...) { return 0; }
constexpr rsf_unity_package_platform platform{
    [](const rsf_sharpening_options* options) noexcept -> uint32_t { return rsf::sharpening_settings::valid(*options) ? 1u : 0u; },
    [](uint32_t preset) noexcept -> uint32_t { return rsf_dlss_preset_valid(preset) ? 1u : 0u; },
    rsf_unity_set_engine_spatial,
    [](const rsf_native_fg_options* options) noexcept { rsf_fg12_options(options); },
    [](rsf_native_fg_status* output) noexcept { return rsf_fg12_status(output); },
    [](rsf_native_latency_status* output) noexcept { return rsf_native_latency_status_get(output); },
    []() noexcept { return rsf_d3d11_present_backend(); },
    []() noexcept { return rsf_d3d11_present_switch_result(); },
    []() noexcept { return rsf_d3d11_present_generation(); },
    open_session,
    [](uint32_t enabled) noexcept { rsf_unity_bridge_activate(enabled != 0); },
    []() noexcept -> uint32_t { return rsf_unity_bridge_drained() ? 1u : 0u; },
    []() noexcept -> uint32_t { return rsf_fg12_end_session(api.session_id) ? 1u : 0u; },
    destroy_views,
    []() noexcept { rsf_unity_bridge_release(); },
};
void UNITY_INTERFACE_API device_event(UnityGfxDeviceEventType type)
{
    if (type == kUnityGfxDeviceEventInitialize) rsf_unity_package_bridge_load(registry);
    if (type == kUnityGfxDeviceEventShutdown) {
        rsf_unity_package_device_shutdown(&platform);
        rsf_unity_package_bridge_unload();
    }
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
    rsf_unity_package_bridge_failed(result);
    log(nullptr, message);
}
bool rsf_unity_package_is_editor() noexcept { return editor; }

extern "C" __declspec(dllexport) int rsfu_initialize(const char* runtime, const char* engine, const char* id, rsf_unity_native_api* output) noexcept try
{
    if (!runtime || !engine || !id || !output) return 0;
    std::lock_guard<std::mutex> lock(guard);
    Opening opening{runtime, engine, id};
    const int result = rsf_unity_package_initialize(&platform, &opening);
    if (result == 1) *output = api;
    return result;
} catch (...) { return 0; }

extern "C" __declspec(dllexport) int rsfu_configure(const rsf_unity_package_settings* input) noexcept try
{
    std::lock_guard<std::mutex> lock(guard); return rsf_unity_package_configure(input, &platform);
} catch (...) { return 0; }

extern "C" __declspec(dllexport) int rsfu_settings(rsf_unity_package_settings* output) noexcept try
{
    std::lock_guard<std::mutex> lock(guard); return rsf_unity_package_settings_get(output);
} catch (...) { return 0; }

extern "C" __declspec(dllexport) int rsfu_temporal_ready() noexcept { return rsf_unity_package_temporal_ready() ? 1 : 0; }
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
    return rsf_unity_package_frame_counters(output, &platform);
}

extern "C" __declspec(dllexport) int rsfu_status(rsf_unity_package_status* output) noexcept try
{
    std::lock_guard<std::mutex> lock(guard); return rsf_unity_package_status_get(output, &platform);
} catch (...) { return 0; }

extern "C" __declspec(dllexport) int rsfu_input(uint64_t frame, uint32_t kinds) noexcept try
{
    if (!frame || (kinds & ~7u)) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    if (!kinds) return RSF_BACKEND_OK;
    LARGE_INTEGER now{},frequency{}; QueryPerformanceCounter(&now); QueryPerformanceFrequency(&frequency);
    const auto source=input_frame.load()==frame ? frame : 0;
    rsf_game_cpu_event fact{sizeof(fact),RSF_GAME_CPU_INPUT_EVENT,api.session_id,source,uint64_t(now.QuadPart),
        uint64_t(frequency.QuadPart),0,kinds,0,RSF_GAME_CPU_EVENT_VERSION,RSF_GAME_CPU_INPUT_KEYED | (!source ? RSF_GAME_CPU_INPUT_UNFRAMED : 0u)};
    rsf_fg12_cpu(&fact); return RSF_BACKEND_OK;
} catch (...) { return RSF_BACKEND_ERROR_FEATURE_FAILED; }

extern "C" __declspec(dllexport) int rsfu_shutdown() noexcept try
{
    if (editor) rsf_unity_preview_hide();
    if (!rsf_unity_package_quiesce(&platform)) return 0;
    std::lock_guard<std::mutex> lock(guard);
    rsf_unity_package_retire(&platform); return 1;
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
