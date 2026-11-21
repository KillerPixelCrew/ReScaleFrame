#include <rescaleframe/native_cpu.h>
// SPDX-License-Identifier: MIT
// The settings, status snapshot, render plan, temporal readiness, pending reconfiguration,
// session lifecycle, the D3D12 SR views with their recording slots, the presentation (the
// package identity, the generation install and the window pass's provider request) and the
// session services (each session's host services and bridge API, the log, the input window and
// the routing of CPU events, window messages and rsfu_input) are Rust
// (games/unity-mono/src/package_state.rs, package_views.rs, package_state/presentation.rs and
// package_state/services.rs, through package_state.h). This file keeps the Unity, DXGI, Vulkan
// and preview work, the render callback's routing and the window guard, and lends the typed
// Win32, SDK and Runtime calls through rsf_unity_package_platform. Its mutex still serializes the
// exports and the SR pass where it did.
#include "package.h"
#include "bridge.h"
#include "vulkan_bridge.h"
#include "preview.h"
#include <rescaleframe/native_sr_d3d12.h>
#include <rescaleframe/native_fg_d3d12.h>
#include <rescaleframe/gpu_policy.h>
#include <rescaleframe/fg_choice.h>
#include <windows.h>
#include <d3d12.h>
#include <dxgi1_4.h>
#include <IUnityGraphics.h>
#include <IUnityGraphicsD3D12.h>
#include <wrl/client.h>
#include <mutex>
#include <string>
#include <cstdio>
#include <cstring>

extern "C" void UNITY_INTERFACE_API rsf_unity_package_bridge_load(IUnityInterfaces*);
extern "C" void UNITY_INTERFACE_API rsf_unity_package_bridge_unload();
extern "C" int rsf_unity_set_engine_spatial(uint32_t, uint32_t) noexcept;

// package_abi.rs mirrors these layouts and constants.
static_assert(sizeof(rsf_unity_package_settings) == 100 && sizeof(rsf_unity_package_status) == 184);
static_assert(sizeof(rsf_unity_frame_counters) == 40 && sizeof(rsf_unity_package_platform) == 344);
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
// The table, defined below; the log, window procedure and exports hand it to Rust.
const rsf_unity_package_platform* leaves() noexcept;
// The hooked input window's procedure: services.rs reports the message and names the procedure
// it goes on to.
LRESULT CALLBACK input_proc(HWND hwnd, UINT message, WPARAM wparam, LPARAM lparam)
{
    const auto previous = reinterpret_cast<WNDPROC>(rsf_unity_package_window_message(message, lparam, leaves()));
    return previous ? CallWindowProcW(previous, hwnd, message, wparam, lparam) : DefWindowProcW(hwnd, message, wparam, lparam);
}
// rsf_unity_package_platform's input-window leaves.
BOOL CALLBACK find_player_window(HWND hwnd, LPARAM found)
{
    DWORD process = 0; wchar_t name[64]{}; GetWindowThreadProcessId(hwnd, &process);
    if (process != GetCurrentProcessId() || !GetClassNameW(hwnd, name, 64) || std::wcscmp(name, L"UnityWndClass") != 0) return TRUE;
    *reinterpret_cast<HWND*>(found) = hwnd;
    return FALSE;
}
void* player_window() noexcept
{
    HWND found = nullptr; EnumWindows(find_player_window, reinterpret_cast<LPARAM>(&found)); return found;
}
void* subclass(void* window) noexcept
{
    return reinterpret_cast<void*>(SetWindowLongPtrW(static_cast<HWND>(window), GWLP_WNDPROC, reinterpret_cast<LONG_PTR>(input_proc)));
}
void restore(void* window, void* previous) noexcept
{
    if (reinterpret_cast<WNDPROC>(GetWindowLongPtrW(static_cast<HWND>(window), GWLP_WNDPROC)) == input_proc)
        SetWindowLongPtrW(static_cast<HWND>(window), GWLP_WNDPROC, reinterpret_cast<LONG_PTR>(previous));
}
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
void log(void*, const char* text) { rsf_unity_package_log(text, leaves()); }
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
// rsf_unity_package_platform::accept_window, generation's window guard.
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
// rsf_unity_package_platform's SR view leaves. commands is the bridge's D3D12 command list.
uint32_t sr_default_backend(void* commands, uint32_t* backend) noexcept
{
    ComPtr<ID3D12Device> device; ComPtr<IDXGIFactory4> factory; ComPtr<IDXGIAdapter1> adapter; DXGI_ADAPTER_DESC1 desc{};
    if (FAILED(static_cast<ID3D12GraphicsCommandList*>(commands)->GetDevice(IID_PPV_ARGS(&device))) ||
        FAILED(CreateDXGIFactory1(IID_PPV_ARGS(&factory))) ||
        FAILED(factory->EnumAdapterByLuid(device->GetAdapterLuid(), IID_PPV_ARGS(&adapter))) ||
        FAILED(adapter->GetDesc1(&desc))) return 0;
    *backend = rsf_gpu_default_sr_backend(desc.VendorId, (desc.Flags & DXGI_ADAPTER_FLAG_SOFTWARE) != 0);
    return 1;
}
uint32_t sr_create(void* commands, const rsf_unity_package_view_setup* view, int32_t* result, rsf_sr12** output) noexcept try
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
    *output = nullptr; *result = rsf_sr12_create(&setup, output);
    return 1;
} catch (...) { return 0; }
// rsf_unity_package_platform::fg_default_backend: Auto's generation provider for the adapter of
// Unity's D3D12 device.
uint32_t fg_default_backend(uint32_t* backend) noexcept
{
    void* owning_device = nullptr; void* owning_queue = nullptr;
    ComPtr<IDXGIFactory4> factory; ComPtr<IDXGIAdapter1> adapter; DXGI_ADAPTER_DESC1 desc{};
    if (!rsf_unity_package_graphics(&owning_device, &owning_queue) ||
        FAILED(CreateDXGIFactory1(IID_PPV_ARGS(&factory))) ||
        FAILED(factory->EnumAdapterByLuid(static_cast<ID3D12Device*>(owning_device)->GetAdapterLuid(), IID_PPV_ARGS(&adapter))) ||
        FAILED(adapter->GetDesc1(&desc))) return 0;
    *backend = rsf_fg_default_backend(desc.VendorId, (desc.Flags & DXGI_ADAPTER_FLAG_SOFTWARE) != 0);
    return 1;
}
void render_sr(void* commands, const rsf_game_render_pass* pass);
void request_generation();
// rsf_unity_package_platform::render_pass, the host services' render callback.
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
    render_sr(commands, original);
}
// rsf_unity_package_platform::vulkan_surface: services.rs asks only in the player.
uint32_t vulkan_surface() noexcept try
{
    return !(graphics && graphics->GetRenderer() == kUnityGfxRendererVulkan) || rsf_uvk_create_window() ? 1u : 0u;
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
    []() noexcept -> uint32_t { return rsf_unity_bridge_drained() ? 1u : 0u; },
    [](uint64_t session) noexcept -> uint32_t { return rsf_fg12_end_session(session) ? 1u : 0u; },
    sr_default_backend,
    sr_create,
    [](const rsf_sr12* sr, uint32_t* width, uint32_t* height) noexcept -> int32_t { return rsf_sr12_plan(sr, width, height); },
    [](rsf_sharpening_options* output) noexcept { *output = rsf_sharpening_defaults(); },
    [](rsf_sr12* sr, const rsf_sharpening_options* options) noexcept -> int32_t { return rsf_sr12_set_sharpening(sr, options); },
    [](rsf_sr12* sr, void* commands, const rsf_game_render_pass* pass, uint32_t slot) noexcept -> int32_t {
        return rsf_sr12_evaluate(sr, commands, pass, slot);
    },
    [](rsf_sr12* sr) noexcept { rsf_sr12_destroy(sr); },
    []() noexcept { rsf_unity_bridge_release(); },
    accept,
    log,
    [](const rsf_fg12_setup* setup) noexcept -> int32_t { return rsf_fg12_install(setup); },
    []() noexcept -> uint32_t { return rsf_d3d11_present_has_owner() ? 1u : 0u; },
    [](uint32_t backend) noexcept -> int32_t { return rsf_d3d11_present_request(backend); },
    fg_default_backend,
    vulkan_surface,
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
    player_window,
    []() noexcept { return rsf_unity_preview_parent(); },
    subclass,
    restore,
    raw_input_type,
    timestamp,
    [](const rsf_game_cpu_event* event) noexcept { rsf_fg12_cpu(event); },
};
const rsf_unity_package_platform* leaves() noexcept { return &platform; }
void render_sr(void* commands, const rsf_game_render_pass* pass)
{
    std::lock_guard<std::mutex> lock(guard); rsf_unity_package_render(commands, pass, &platform);
}
// The window pass's request stays outside the mutex, which only wrapped its settings read and
// result store; Rust's state lock covers both now. The Editor's install runs under it from open.
void request_generation() { rsf_unity_package_request_generation(&platform); }
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
    return rsf_unity_package_initialize(runtime, engine, id, output, &platform);
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
    rsf_unity_package_preview_input(&platform);
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

extern "C" __declspec(dllexport) int rsfu_input(uint64_t frame, uint32_t kinds) noexcept
{
    return rsf_unity_package_input(frame, kinds, &platform);
}

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
    rsf_unity_package_unload(&platform);
    registry = nullptr; graphics = nullptr;
}
