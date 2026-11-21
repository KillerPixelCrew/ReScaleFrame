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
#include "runtime_worker_native.h"
#include <windows.h>
#include <dxgi1_4.h>
#include <wrl/client.h>
#include <MinHook.h>
#include <intrin.h>
#include <cstdint>
#include <cstdio>
#include <cstring>
#include <cwchar>
#include <mutex>
#include <string>
#include "asi_platform.h"
#include "social_club.h"
#include "camera_probe.h"
#include "native_capture.h"
#include "native_settings.h"

namespace {
HMODULE self = nullptr;
constexpr auto log = rsf_rdr2_runtime_log;
std::mutex trace_guard;
std::string dump_directory;
std::string modules_path;
char census_directory[MAX_PATH * 3]{};
#ifdef RSF_HAVE_FRAME_CAPTURE
wchar_t frame_request[MAX_PATH]{};
DWORD WINAPI frame_requests(void*) noexcept
{
    bool previous_key{};
    for (;;) {
        Sleep(250);
        DWORD foreground_process{};
        GetWindowThreadProcessId(GetForegroundWindow(), &foreground_process);
        const bool key = foreground_process == GetCurrentProcessId() && (GetAsyncKeyState(VK_F10) & 0x8000) != 0;
        const bool requested = GetFileAttributesW(frame_request) != INVALID_FILE_ATTRIBUTES && DeleteFileW(frame_request);
        if (requested || (key && !previous_key)) {
            rsf_rdr2_request_camera_capture();
            rsf_capture_trigger(1);
        }
        previous_key = key;
    }
}

int32_t install_frame_probe(const wchar_t* library, const wchar_t* prefix) noexcept
{
    char library_utf8[MAX_PATH * 3]{}, prefix_utf8[MAX_PATH * 3]{};
    WideCharToMultiByte(CP_UTF8, 0, library, -1, library_utf8, sizeof(library_utf8), nullptr, nullptr);
    WideCharToMultiByte(CP_UTF8, 0, prefix, -1, prefix_utf8, sizeof(prefix_utf8), nullptr, nullptr);
    const auto result = rsf_capture_initialise(library_utf8, prefix_utf8);
    if (result == RSF_CAPTURE_OK) {
        swprintf_s(frame_request, L"%s.request", prefix);
        HANDLE poller = CreateThread(nullptr, 0, frame_requests, nullptr, 0, nullptr);
        if (poller) CloseHandle(poller);
    }
    return static_cast<int32_t>(result);
}
#endif

// Opt-in, game-owned HRESULT observation. This catches the first error before RAGE converts a
// failed command-list Reset into ERR_GFX_D3D_DEFERRED_MEM. It does not alter the HRESULT or touch
// any cache, device, command list or allocator. The crate verified the build and helper bytes; see
// the matching-build sites in rdr2-renderer.md.
using CheckD3D12Result = HRESULT (*)(HRESULT);
CheckD3D12Result original_check_d3d12_result = nullptr;
wchar_t d3d12_error_path[32768]{};
uint32_t traced_image_size{};

HRESULT trace_d3d12_result(HRESULT result) noexcept
{
    if (FAILED(result)) {
        std::lock_guard<std::mutex> lock(trace_guard);
        if (FILE* stream = _wfopen(d3d12_error_path, L"ab")) {
            void* stack[24]{};
            const auto count = CaptureStackBackTrace(1, 24, stack, nullptr);
            const auto base = reinterpret_cast<uintptr_t>(GetModuleHandleW(nullptr));
            std::fprintf(stream, "tick=%llu tid=%lu hr=0x%08lx caller_rva=0x%llx stack=",
                static_cast<unsigned long long>(GetTickCount64()), GetCurrentThreadId(),
                static_cast<unsigned long>(result),
                static_cast<unsigned long long>(reinterpret_cast<uintptr_t>(_ReturnAddress()) - base));
            for (USHORT i = 0; i < count; ++i) {
                const auto address = reinterpret_cast<uintptr_t>(stack[i]);
                if (address >= base && address - base < traced_image_size) {
                    std::fprintf(stream, " RDR2+0x%llx", static_cast<unsigned long long>(address - base));
                } else {
                    std::fprintf(stream, " %p", stack[i]);
                }
            }
            std::fprintf(stream, "\n");
            std::fclose(stream);
        }
    }
    return original_check_d3d12_result(result);
}

int32_t install_d3d12_result_trace(uintptr_t target, uint32_t image_size, const wchar_t* path) noexcept
{
    wcsncpy_s(d3d12_error_path, path, _TRUNCATE);
    traced_image_size = image_size;
    auto status = MH_Initialize();
    if (status == MH_OK || status == MH_ERROR_ALREADY_INITIALIZED) {
        status = MH_CreateHook(reinterpret_cast<void*>(target), reinterpret_cast<void*>(&trace_d3d12_result),
            reinterpret_cast<void**>(&original_check_d3d12_result));
        if (status == MH_OK) status = MH_EnableHook(reinterpret_cast<void*>(target));
    }
    return static_cast<int32_t>(status);
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

std::string utf8(const std::wstring& value)
{
    const int size = WideCharToMultiByte(CP_UTF8, 0, value.c_str(), -1, nullptr, 0, nullptr, nullptr);
    if (size <= 1) return {};
    std::string result(static_cast<size_t>(size), '\0');
    WideCharToMultiByte(CP_UTF8, 0, value.c_str(), -1, result.data(), size, nullptr, nullptr);
    result.resize(static_cast<size_t>(size - 1));
    return result;
}

// Diagnostics only: the shipped code section is ciphertext (entropy 7.998), so offline analysis
// needs the decrypted image. Which graphics runtime the game selected is only visible after it
// initialises one, so the module list is sampled on a schedule.
DWORD WINAPI diagnostics(void*) noexcept try
{
    double entropy = 0.0;
    rsf_measure_module_code(nullptr, &entropy);
    char message[256]{};
    std::snprintf(message, sizeof(message), "Code entropy at entry %.3f.", entropy);
    log(nullptr, message);
    if (!dump_directory.empty()) {
        CreateDirectoryA(dump_directory.c_str(), nullptr);
        rsf_dump_options options{};
        options.struct_size = sizeof(options);
        options.abi_version = RSF_MODULE_DUMP_ABI_VERSION;
        options.output_directory_utf8 = dump_directory.c_str();
        options.entropy_threshold = 7.0;
        options.require_decrypted = 1;
        rsf_dump_report report{};
        report.struct_size = sizeof(report);
        const auto result = rsf_dump_when_decrypted(&options, 250, 180000, 2, &report);
        std::snprintf(message, sizeof(message),
            "Module dump result=%d entropy=%.3f sections=%u imports=%u iat_refs=%u base=0x%llx bytes=%llu",
            int(result), report.code_entropy, report.sections_written, report.imports_described,
            report.iat_references, static_cast<unsigned long long>(report.load_base),
            static_cast<unsigned long long>(report.bytes_written));
        log(nullptr, message);
    }
    static const DWORD marks[] = {5000, 15000, 30000, 60000, 120000};
    const auto begin = GetTickCount64();
    for (const auto mark : marks) {
        const auto elapsed = GetTickCount64() - begin;
        if (elapsed < mark) Sleep(static_cast<DWORD>(mark - elapsed));
        char label[32]{};
        std::snprintf(label, sizeof(label), "t+%lus", static_cast<unsigned long>(mark / 1000));
        rsf_write_module_list(modules_path.c_str(), label);
    }
    log(nullptr, "Module sampling finished.");
    rsf_census_counters counters{};
    counters.struct_size = sizeof(counters);
    counters.abi_version = RSF_CENSUS_ABI_VERSION;
    if (census_directory[0] && rsf_census_flush() == RSF_CENSUS_OK &&
        rsf_census_get_counters(&counters) == RSF_CENSUS_OK) {
        std::snprintf(message, sizeof(message),
            "Census at 120 s: calls=%llu written=%llu sites=%u hooks=%u conflicts=%u presents=%llu dropped=%llu",
            static_cast<unsigned long long>(counters.calls_seen),
            static_cast<unsigned long long>(counters.records_written), counters.sites,
            counters.hooks_installed, counters.hook_conflicts,
            static_cast<unsigned long long>(counters.present_index),
            static_cast<unsigned long long>(counters.records_dropped_queue + counters.records_dropped_budget +
                                            counters.records_dropped_size));
        log(nullptr, message);
    }
    return 0;
}
catch (...) { return 1; }

void start_diagnostics(const wchar_t* modules, const wchar_t* dumps) noexcept try
{
    modules_path = utf8(modules);
    if (dumps) dump_directory = utf8(dumps);
    if (HANDLE worker = CreateThread(nullptr, 0, diagnostics, nullptr, 0, nullptr)) CloseHandle(worker);
}
catch (...) {}

constexpr rsf_rdr2_runtime_leaves runtime_leaves{
    rsf_rdr2_fingerprint,
    start_diagnostics,
    [](const wchar_t* path) noexcept { return rsf_rdr2_native_settings_probe(path, log, nullptr); },
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
    [](bool trace) noexcept { return rsf_rdr2_install_social_club_fix(log, trace); },
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
