#include <rescaleframe/native_cpu.h>
// SPDX-License-Identifier: MIT
// The settings, status snapshot, render plan, temporal readiness, pending reconfiguration,
// session lifecycle, the D3D12 SR views with their recording slots, the presentation (the
// package identity, the generation install and the window pass's provider request) and the
// session services (each session's host services and bridge API, the log, the input window and
// the routing of CPU events, window messages and rsfu_input) and the routing (the render
// callback's passes with the Editor preview's admission and diagnostic budget, the generation
// window guard and Unity's device lifecycle) are Rust (games/unity-mono/src/package_state.rs,
// package_views.rs and package_state/presentation.rs, services.rs and routing.rs, through
// package_state.h). This file keeps the Unity, DXGI, Vulkan and preview work and lends the typed
// Win32, SDK and Runtime calls through rsf_unity_package_platform. Its mutex serializes the
// session exports, Unity device events and the SR pass.
#include "package.h"
#include "bridge.h"
#include "vulkan_bridge.h"
#include "preview.h"
#include <rescaleframe/native_sr_d3d12.h>
#include <rescaleframe/native_fg_d3d12.h>
#include <rescaleframe/fg_choice.h>
#include <windows.h>
#include <d3d12.h>
#include <dxgi1_4.h>
#include <IUnityGraphics.h>
#include <IUnityGraphicsD3D12.h>
#include <wrl/client.h>
#include <mutex>
#include <string>
#include <cstring>

extern "C" void UNITY_INTERFACE_API rsf_unity_package_bridge_load(IUnityInterfaces*);
extern "C" void UNITY_INTERFACE_API rsf_unity_package_bridge_unload();
extern "C" int rsf_unity_set_engine_spatial(uint32_t, uint32_t) noexcept;

extern "C" void* rsf_unity_package_player_window() noexcept;
extern "C" uint32_t rsf_unity_package_window_class(void* hwnd) noexcept;
extern "C" uint32_t rsf_unity_package_subclass(void* hwnd) noexcept;
extern "C" void rsf_unity_package_restore(void* hwnd) noexcept;

// package_abi.rs mirrors these layouts and constants.
static_assert(sizeof(rsf_unity_package_settings) == 100 && sizeof(rsf_unity_package_status) == 184);
static_assert(sizeof(rsf_unity_frame_counters) == 40 && sizeof(rsf_unity_package_platform) == 432);
static_assert(sizeof(rsf_native_fg_status) == 352 && sizeof(rsf_native_latency_status) == 152);
static_assert(sizeof(rsf_native_fg_options) == 24 && RSF_BACKEND_OK == 0);
static_assert(RSF_FG_STAT_TOTAL_PRESENTED == 1 && RSF_SR_NONE == 0);
static_assert(sizeof(rsf_unity_package_view_setup) == 88 && RSF_SCREEN_MENU == 1 && RSF_SCREEN_FLIGHT == 4);
// generation.rs builds the install; runtime_abi.rs mirrors rsf_fg12_setup.
static_assert(sizeof(rsf_fg12_setup) == 152 && RSF_DLSS_ENGINE_UNITY == 2);
// services.rs mirrors rsfu_input's results, the host services and the input classification.
static_assert(RSF_BACKEND_ERROR_INVALID_ARGUMENT == -1 && RSF_BACKEND_ERROR_FEATURE_FAILED == -9);
static_assert(sizeof(rsf_game_host_services) == 104 && sizeof(rsf_unity_native_api) == 72 && sizeof(wchar_t) == 2);
static_assert(WM_INPUT == 0x00FF && WM_KEYFIRST == 0x0100 && WM_KEYLAST == 0x0109);
static_assert(WM_MOUSEFIRST == 0x0200 && WM_MOUSELAST == 0x020E && RIM_TYPEMOUSE == 0 && RIM_TYPEKEYBOARD == 1);

namespace {
using Microsoft::WRL::ComPtr;
std::mutex guard;
std::wstring ini;
bool editor = false;
IUnityInterfaces* registry = nullptr;
IUnityGraphics* graphics = nullptr;
uint32_t raw_input_type(intptr_t lparam, uint32_t* type) noexcept
{
    RAWINPUTHEADER header{}; UINT bytes = sizeof(header);
    if (GetRawInputData(reinterpret_cast<HRAWINPUT>(lparam), RID_HEADER, &header, &bytes, sizeof(header)) != sizeof(header)) return 0;
    *type = header.dwType; return 1;
}
void timestamp(uint64_t* counter, uint64_t* frequency) noexcept
{
    LARGE_INTEGER now{}, ticks{}; QueryPerformanceCounter(&now); QueryPerformanceFrequency(&ticks);
    *counter = uint64_t(now.QuadPart); *frequency = uint64_t(ticks.QuadPart);
}
// rsf_unity_package_platform's log leaves. log is the package log in the Runtime's form.
void log(void*, const char* text) { rsf_unity_package_log(text); }
uint32_t local_app_data(wchar_t* output, uint32_t capacity) noexcept
{
    const DWORD length = GetEnvironmentVariableW(L"LOCALAPPDATA", output, capacity);
    return length < capacity ? length : 0;
}
void* log_open(const wchar_t* path) noexcept
{
    const HANDLE file = CreateFileW(path, FILE_APPEND_DATA, FILE_SHARE_READ | FILE_SHARE_WRITE, nullptr, OPEN_ALWAYS, FILE_ATTRIBUTE_NORMAL, nullptr);
    return file == INVALID_HANDLE_VALUE ? nullptr : file;
}
void log_write(void* file, const char* text) noexcept
{
    OutputDebugStringA(text); OutputDebugStringA("\n");
    if (file) {
        DWORD written = 0;
        WriteFile(file, text, static_cast<DWORD>(std::strlen(text)), &written, nullptr);
        WriteFile(file, "\r\n", 2, &written, nullptr);
    }
}
// rsf_unity_package_platform::accept_window, generation's window guard; routing.rs decides.
int accept(void*, void* hwnd) { return rsf_unity_package_accept_window(hwnd); }
int configuration(void*, rsf_game_render_config* output)
{
    if (!output || output->struct_size < sizeof(*output)) return 0;
    return rsf_unity_package_plan(output);
}
// rsf_unity_package_platform's SR view leaves. commands is the bridge's D3D12 command list.
uint32_t sr_create(void* commands, rsf_unity_package_view_setup* view, int32_t* result, rsf_sr12** output) noexcept try
{
    ComPtr<ID3D12Device> device;
    if (FAILED(static_cast<ID3D12GraphicsCommandList*>(commands)->GetDevice(IID_PPV_ARGS(&device)))) return 0;
    rsf_sr12_setup setup{}; setup.struct_size = sizeof(setup); setup.abi_version = 1;
    setup.device = device.Get(); setup.backend = view->backend; setup.quality = view->quality;
    setup.output_width = view->output_width; setup.output_height = view->output_height;
    setup.render_width = view->render_width; setup.render_height = view->render_height;
    setup.inverted_depth = view->inverted_depth;
    setup.streamline_host = rsf_d3d11_present_host();
    setup.fsr2_directory_utf8 = setup.fsr3_directory_utf8 = setup.fsr4_directory_utf8 = view->fsr_utf8;
    setup.xess_directory_utf8 = view->xess_utf8; setup.log = log;
    setup.dlss = {sizeof(setup.dlss), RSF_DLSS_ABI_VERSION, view->interposer_utf8, view->streamline_utf8, view->root_utf8,
        0, RSF_DLSS_ENGINE_UNITY, view->engine_version_utf8, view->project_id_utf8, 1, log, nullptr, view->dlss_preset};
    rsf_sr12_plan_result plan{};
    plan.struct_size = sizeof(plan);
    plan.abi_version = RSF_SR12_PLAN_ABI_VERSION;
    const auto created = rsf_sr12_create_planned(&setup, &plan, output);
    *result = plan.requested_result != RSF_BACKEND_OK ? plan.requested_result : created;
    if (created == RSF_BACKEND_OK) {
        view->backend = plan.backend;
        view->render_width = plan.render_width;
        view->render_height = plan.render_height;
    }
    return 1;
} catch (...) { return 0; }
// rsf_unity_package_platform::fg_default_backend: Auto's generation provider for the adapter of
// Unity's D3D12 device.
uint32_t fg_default_backend(uint32_t* backend) noexcept
{
    ComPtr<ID3D12Device> owning_device;
    ComPtr<ID3D12CommandQueue> owning_queue;
    ComPtr<IDXGIFactory4> factory; ComPtr<IDXGIAdapter1> adapter; DXGI_ADAPTER_DESC1 desc{};
    if (!rsf_unity_package_graphics(owning_device, owning_queue) ||
        FAILED(CreateDXGIFactory1(IID_PPV_ARGS(&factory))) ||
        FAILED(factory->EnumAdapterByLuid(owning_device->GetAdapterLuid(), IID_PPV_ARGS(&adapter))) ||
        FAILED(adapter->GetDesc1(&desc))) return 0;
    *backend = rsf_fg_default_backend(desc.VendorId, (desc.Flags & DXGI_ADAPTER_FLAG_SOFTWARE) != 0);
    return 1;
}
void render_sr(void* commands, const rsf_game_render_pass* pass);
// rsf_unity_package_platform::render_pass, the host services' render callback. routing.rs routes
// the window, final-scene and submitted passes and admits the SR pass, which runs under the mutex.
void render(void*, void* commands, const rsf_game_render_pass* original, uint32_t begin)
{
    if (rsf_unity_package_route(commands, original, begin)) render_sr(commands, original);
}
// rsf_unity_package_platform's render-routing leaves.
uint32_t preview_present(void* resource, const rsf_game_render_pass* pass, uint32_t common) noexcept try
{
    return rsf_unity_preview_present(resource, *pass, common != 0) ? 1u : 0u;
} catch (...) { return 0; }
void texture_desc(void* resource, uint64_t* width, uint32_t* height, uint32_t* format) noexcept
{
    const auto desc = static_cast<ID3D12Resource*>(resource)->GetDesc();
    *width = desc.Width; *height = desc.Height; *format = static_cast<uint32_t>(desc.Format);
}
uint32_t preview_prepare(uint32_t width, uint32_t height) noexcept try
{
    ComPtr<ID3D12Device> owning_device;
    ComPtr<ID3D12CommandQueue> owning_queue;
    return rsf_unity_package_graphics(owning_device, owning_queue) &&
        rsf_unity_preview_prepare(owning_device.Get(), owning_queue.Get(), width, height) ? 1u : 0u;
} catch (...) { return 0; }
uint32_t bridge_open(uint32_t is_editor) noexcept try
{
    return rsf_unity_package_bridge_open(is_editor != 0) ? 1u : 0u;
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
    [](uint32_t enabled) noexcept { rsf_unity_bridge_activate(enabled != 0); },
    []() noexcept -> uint32_t { return rsf_unity_package_bridge_try_close() ? 1u : 0u; },
    [](uint64_t session) noexcept -> uint32_t { return rsf_fg12_end_session(session) ? 1u : 0u; },
    rsf_sr12_quality_supported,
    sr_create,
    [](rsf_sharpening_options* output) noexcept { *output = rsf_sharpening_defaults(); },
    [](rsf_sr12* sr, const rsf_sharpening_options* options) noexcept -> int32_t { return rsf_sr12_set_sharpening(sr, options); },
    [](rsf_sr12* sr, void* commands, const rsf_game_render_pass* pass, uint32_t slot) noexcept -> int32_t {
        return rsf_sr12_evaluate(sr, commands, pass, slot);
    },
    [](rsf_sr12* sr) noexcept { rsf_sr12_destroy(sr); },
    [](uint32_t commit) noexcept { rsf_unity_package_bridge_finish_retirement(commit != 0); },
    accept,
    log,
    [](const rsf_fg12_setup* setup) noexcept -> int32_t { return rsf_fg12_install(setup); },
    []() noexcept -> uint32_t { return rsf_d3d11_present_has_owner() ? 1u : 0u; },
    [](uint32_t backend) noexcept -> int32_t { return rsf_d3d11_present_request(backend); },
    fg_default_backend,
    bridge_open,
    []() noexcept -> uint64_t { return GetTickCount64(); },
    render,
    configuration,
    [](const rsf_game_host_services* services, rsf_unity_native_api* api) noexcept -> uint32_t {
        return rsf_unity_bridge_prepare(*services, api) ? 1u : 0u;
    },
    local_app_data,
    [](const wchar_t* path) noexcept { CreateDirectoryW(path, nullptr); },
    log_open,
    log_write,
    [](void* file) noexcept { CloseHandle(file); },
    rsf_unity_package_player_window,
    []() noexcept { return rsf_unity_preview_parent(); },
    rsf_unity_package_subclass,
    rsf_unity_package_restore,
    raw_input_type,
    timestamp,
    [](const rsf_game_cpu_event* event) noexcept { rsf_fg12_cpu(event); },
    [](uint64_t session, uint64_t frame) noexcept { rsf_fg12_submitted(session, frame); },
    [](const rsf_game_render_pass* pass) noexcept { rsf_fg12_window(pass); },
    [](void* commands, const rsf_game_render_pass* pass) noexcept { rsf_fg12_hudless(commands, pass); },
    preview_present,
    texture_desc,
    preview_prepare,
    [](void* window) noexcept -> uint32_t { return rsf_unity_preview_owns(window) ? 1u : 0u; },
    []() noexcept -> uint32_t { return graphics && graphics->GetRenderer() == kUnityGfxRendererVulkan ? 1u : 0u; },
    [](void* window) noexcept -> uint32_t { return rsf_uvk_owns_window(window) ? 1u : 0u; },
    rsf_unity_package_window_class,
    // Under the mutex, which also guards registry.
    []() noexcept -> uint32_t {
        if (!registry) return 0;
        rsf_unity_package_bridge_load(registry);
        return 1;
    },
    []() noexcept { rsf_unity_package_bridge_unload(); },
};
void render_sr(void* commands, const rsf_game_render_pass* pass)
{
    std::lock_guard<std::mutex> lock(guard); rsf_unity_package_render(commands, pass);
}
// routing.rs keeps the device state: Unity may have destroyed a retained device.
void UNITY_INTERFACE_API device_event(UnityGfxDeviceEventType type)
{
    if (type != kUnityGfxDeviceEventInitialize && type != kUnityGfxDeviceEventShutdown) return;
    std::lock_guard<std::mutex> lock(guard);
    rsf_unity_package_device_event(type == kUnityGfxDeviceEventInitialize ? 1u : 0u);
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
    // The identity, the [UnitySR] Log file and the player's install.
    rsf_unity_package_load(ini.c_str(), editor ? 1u : 0u, &platform);
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
    return rsf_unity_package_initialize(runtime, engine, id, output);
} catch (...) { return 0; }

extern "C" __declspec(dllexport) int rsfu_configure(const rsf_unity_package_settings* input) noexcept try
{
    std::lock_guard<std::mutex> lock(guard); return rsf_unity_package_configure(input);
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
    rsf_unity_package_preview_input();
    return configured ? 1 : 0;
}
extern "C" __declspec(dllexport) int rsfu_frame_counters(rsf_unity_frame_counters* output) noexcept
{
    return rsf_unity_package_frame_counters(output);
}

extern "C" __declspec(dllexport) int rsfu_status(rsf_unity_package_status* output) noexcept try
{
    std::lock_guard<std::mutex> lock(guard); return rsf_unity_package_status_get(output);
} catch (...) { return 0; }

extern "C" __declspec(dllexport) int rsfu_input(uint64_t frame, uint32_t kinds) noexcept
{
    return rsf_unity_package_input(frame, kinds);
}

extern "C" __declspec(dllexport) int rsfu_shutdown() noexcept try
{
    if (editor) rsf_unity_preview_hide();
    std::lock_guard<std::mutex> lock(guard);
    return rsf_unity_package_shutdown() ? 1 : 0;
} catch (...) { return 0; }

extern "C" void UNITY_INTERFACE_EXPORT UNITY_INTERFACE_API UnityPluginLoad(IUnityInterfaces* interfaces)
{
    {
        std::lock_guard<std::mutex> lock(guard);
        if (rsf_unity_package_device_retained() || registry) return;
        registry = interfaces;
    }
    graphics = interfaces ? interfaces->Get<IUnityGraphics>() : nullptr;
    if (interfaces) rsf_uvk_preload(interfaces);
    preload();
    if (graphics) { graphics->RegisterDeviceEventCallback(device_event); device_event(kUnityGfxDeviceEventInitialize); }
}
extern "C" void UNITY_INTERFACE_EXPORT UNITY_INTERFACE_API UnityPluginUnload()
{
    if (graphics) graphics->UnregisterDeviceEventCallback(device_event);
    device_event(kUnityGfxDeviceEventShutdown);
    rsf_unity_package_unload();
    std::lock_guard<std::mutex> lock(guard);
    registry = nullptr; graphics = nullptr;
}
