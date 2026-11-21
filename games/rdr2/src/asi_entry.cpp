// SPDX-License-Identifier: MIT
// ASI entry. The game's ASI loader (ScriptHookRDR2 or Ultimate ASI Loader) calls LoadLibrary on
// this module, so no system DLL is impersonated and nothing is forwarded. The directory holding
// the .asi is the deployment root: ReScaleFrame.ini beside it, the runtime and plugin in a
// ReScaleFrame subdirectory.
//
// Startup admission, the [RDR2] configuration gates, the ForceD3D12 write and engine-settings
// ownership are owned by the Rust crate rsf-rdr2 (games/rdr2/src/asi.rs, engine_settings.rs).
// This file lends the startup and Runtime session workers their typed leaves.
#include <rescaleframe/module_dump.h>
#ifdef RSF_HAVE_FRAME_CAPTURE
#include <rescaleframe/frame_capture.h>
#endif
#include <rescaleframe/vendor_census.h>
#include "d3d12_trace.h"
#include "diagnostics.h"
#include "runtime_worker_native.h"
#include <windows.h>
#include <dxgi1_4.h>
#include <wrl/client.h>
#include <MinHook.h>
#include <intrin.h>
#include <cstdint>
#include <cstring>
#include <cwchar>
#include "asi_platform.h"
#include "social_club.h"
#include "camera_probe.h"
#include "native_capture.h"
#include "native_settings.h"

namespace {
HMODULE self = nullptr;
char census_directory[MAX_PATH * 3]{};
#ifdef RSF_HAVE_FRAME_CAPTURE
static_assert(RSF_CAPTURE_OK == 0);
int32_t install_frame_probe(const wchar_t* library, const wchar_t* prefix) noexcept
{
    return rsf_rdr2_start_frame_probe(library, prefix, rsf_capture_initialise, rsf_capture_trigger);
}
#endif

// Opt-in, game-owned HRESULT observation. This catches the first error before RAGE converts a
// failed command-list Reset into ERR_GFX_D3D_DEFERRED_MEM. It does not alter the HRESULT or touch
// any cache, device, command list or allocator. The crate verified the build and helper bytes; see
// the matching-build sites in rdr2-renderer.md.
using CheckD3D12Result = HRESULT (*)(HRESULT);
CheckD3D12Result original_check_d3d12_result = nullptr;

HRESULT trace_d3d12_result(HRESULT result) noexcept
{
    if (FAILED(result)) {
        void* stack[24]{};
        const auto count = CaptureStackBackTrace(1, 24, stack, nullptr);
        const auto base = reinterpret_cast<uintptr_t>(GetModuleHandleW(nullptr));
        const auto caller = reinterpret_cast<uintptr_t>(_ReturnAddress());
        rsf_rdr2_d3d12_trace_record(static_cast<int32_t>(result), base, caller,
            GetTickCount64(), GetCurrentThreadId(), stack, count);
    }
    return original_check_d3d12_result(result);
}

int32_t install_d3d12_result_trace(uintptr_t target, uint32_t image_size, const wchar_t* path) noexcept
{
    static_assert(MH_ERROR_NOT_INITIALIZED == 2);
    const rsf_rdr2_d3d12_trace_hook hook{
        reinterpret_cast<const void*>(&trace_d3d12_result),
        reinterpret_cast<void**>(&original_check_d3d12_result),
        [](uintptr_t address) noexcept -> int32_t {
            return static_cast<int32_t>(MH_EnableHook(reinterpret_cast<void*>(address)));
        },
    };
    return rsf_rdr2_d3d12_trace_install(target, image_size, path, &hook);
}

int32_t install_census(const wchar_t* directory, uint32_t flags) noexcept
{
    WideCharToMultiByte(CP_UTF8, 0, directory, -1, census_directory,
        static_cast<int>(sizeof(census_directory)), nullptr, nullptr);
    rsf_census_options options{};
    options.struct_size = sizeof(options);
    options.abi_version = RSF_CENSUS_ABI_VERSION;
    options.output_dir_utf8 = census_directory;
    options.flags = flags;
    return static_cast<int32_t>(rsf_census_install(&options));
}

constexpr rsf_rdr2_diagnostics_leaves diagnostic_leaves{
    []() noexcept {
        double entropy = 0.0;
        rsf_measure_module_code(nullptr, &entropy);
        return entropy;
    },
    [](const char* directory, double threshold, uint32_t require_decrypted,
        uint32_t poll_ms, uint32_t timeout_ms, uint32_t stable_samples, rsf_rdr2_dump_result* out) noexcept {
        CreateDirectoryA(directory, nullptr);
        rsf_dump_options options{};
        options.struct_size = sizeof(options);
        options.abi_version = RSF_MODULE_DUMP_ABI_VERSION;
        options.output_directory_utf8 = directory;
        options.entropy_threshold = threshold;
        options.require_decrypted = require_decrypted;
        rsf_dump_report report{};
        report.struct_size = sizeof(report);
        const auto result = rsf_dump_when_decrypted(&options, poll_ms, timeout_ms, stable_samples, &report);
        *out = {result, report.sections_written, report.imports_described, report.iat_references,
            report.code_entropy, report.load_base, report.bytes_written};
    },
    [](const char* path, const char* label) noexcept { rsf_write_module_list(path, label); },
    [](rsf_rdr2_census_result* out) noexcept {
        rsf_census_counters counters{};
        counters.struct_size = sizeof(counters);
        counters.abi_version = RSF_CENSUS_ABI_VERSION;
        if (!census_directory[0] || rsf_census_flush() != RSF_CENSUS_OK ||
            rsf_census_get_counters(&counters) != RSF_CENSUS_OK)
            return false;
        *out = {counters.calls_seen, counters.records_written, counters.present_index,
            counters.records_dropped_queue + counters.records_dropped_budget + counters.records_dropped_size,
            counters.sites, counters.hooks_installed, counters.hook_conflicts};
        return true;
    },
};

void start_diagnostics(const wchar_t* modules, const wchar_t* dumps) noexcept
{
    rsf_rdr2_start_diagnostics(modules, dumps, &diagnostic_leaves);
}

constexpr rsf_rdr2_runtime_leaves runtime_leaves{
    rsf_rdr2_fingerprint,
    start_diagnostics,
    rsf_rdr2_native_settings_probe,
};

DWORD WINAPI startup(void*) noexcept
{
    return rsf_rdr2_asi_start_runtime(self, &runtime_leaves);
}

bool read_memory(uintptr_t address, void* out, size_t size) noexcept
{
    SIZE_T count{};
    return address && out &&
        ReadProcessMemory(GetCurrentProcess(), reinterpret_cast<const void*>(address), out, size, &count) &&
        count == size;
}

struct SettingsPath { const uint32_t* ids; uint64_t count; };

constexpr rsf::rdr2::AsiPlatform native{
    sizeof(rsf::rdr2::AsiPlatform),
    rsf::rdr2::asi_platform_version,
    []() noexcept { return reinterpret_cast<uintptr_t>(GetModuleHandleW(nullptr)); },
    read_memory,
    [](wchar_t* out, uint32_t capacity) noexcept -> uint32_t { return GetModuleFileNameW(nullptr, out, capacity); },
    [](wchar_t* out, uint32_t capacity) noexcept -> uint32_t { return GetModuleFileNameW(self, out, capacity); },
    []() noexcept -> uint32_t { return GetCurrentThreadId(); },
    [](const wchar_t* path, const wchar_t* section, const wchar_t* key, int32_t fallback) noexcept -> uint32_t {
        return GetPrivateProfileIntW(section, key, fallback, path);
    },
    [](const wchar_t* path, const wchar_t* section, const wchar_t* key, wchar_t* out, uint32_t capacity) noexcept
        -> uint32_t { return GetPrivateProfileStringW(section, key, L"", out, capacity, path); },
    []() noexcept -> int32_t { return MH_Initialize(); },
    [](uintptr_t target, const void* detour, void** original) noexcept -> int32_t {
        return MH_CreateHook(reinterpret_cast<void*>(target), const_cast<void*>(detour), original);
    },
    [](uintptr_t target) noexcept -> int32_t { return MH_QueueEnableHook(reinterpret_cast<void*>(target)); },
    []() noexcept -> int32_t { return MH_ApplyQueued(); },
    [](uintptr_t target) noexcept -> int32_t { return MH_DisableHook(reinterpret_cast<void*>(target)); },
    [](uintptr_t target) noexcept -> int32_t { return MH_RemoveHook(reinterpret_cast<void*>(target)); },
    [](int32_t status) noexcept { return MH_StatusToString(static_cast<MH_STATUS>(status)); },
    [](uint32_t index) noexcept -> uint32_t {
        Microsoft::WRL::ComPtr<IDXGIFactory1> factory;
        Microsoft::WRL::ComPtr<IDXGIAdapter1> adapter;
        DXGI_ADAPTER_DESC1 description{};
        return SUCCEEDED(CreateDXGIFactory1(IID_PPV_ARGS(&factory))) &&
            SUCCEEDED(factory->EnumAdapters1(index, &adapter)) && SUCCEEDED(adapter->GetDesc1(&description)) ?
            description.VendorId : 0;
    },
    []() noexcept -> uint32_t {
        DEVMODEW desktop{};
        desktop.dmSize = sizeof(desktop);
        return EnumDisplaySettingsW(nullptr, ENUM_CURRENT_SETTINGS, &desktop) ? desktop.dmDisplayFrequency : 0;
    },
    [](uintptr_t function) noexcept { return reinterpret_cast<void* (*)()>(function)(); },
    [](uintptr_t function, void* manager) noexcept { return reinterpret_cast<void* (*)(void*)>(function)(manager); },
    [](uintptr_t function, void* manager, const uint32_t* ids, uint64_t count, void* scratch) noexcept {
        const SettingsPath path{ids, count};
        return reinterpret_cast<void* (*)(void*, const SettingsPath*, void*)>(function)(manager, &path, scratch);
    },
    install_d3d12_result_trace,
    [](const wchar_t* directory) noexcept { return rsf_rdr2_install_native_capture(directory); },
#ifdef RSF_HAVE_FRAME_CAPTURE
    install_frame_probe,
#else
    nullptr,
#endif
    install_census,
    [](const wchar_t* directory) noexcept { return rsf_rdr2_install_camera_probe(directory); },
    [](const wchar_t* library) noexcept { return rsf_rdr2_install_camera_reference(self, library); },
    rsf_rdr2_install_social_club_fix,
    []() noexcept {
        if (HANDLE worker = CreateThread(nullptr, 0, startup, nullptr, 0, nullptr)) CloseHandle(worker);
    },
};
}

BOOL WINAPI DllMain(HINSTANCE instance, DWORD reason, void*)
{
    if (reason == DLL_PROCESS_ATTACH) {
        self = instance; DisableThreadLibraryCalls(instance);
        // Runs under the loader lock: the ASI loader loads modules during CRT startup, after
        // decryption and before the engine reads its settings.
        rsf_rdr2_asi_attach(&native);
    }
    return TRUE;
}
