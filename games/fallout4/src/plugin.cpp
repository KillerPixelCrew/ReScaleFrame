// SPDX-License-Identifier: MIT
#include "native_creation.h"
#include "cpu_events.h"
#include "render_hooks.h"
#include "render_inputs.h"
#include <rescaleframe/d3d11_host.h>
#include <rescaleframe/game_plugin_util.h>
#include <rescaleframe/version.h>
#include <MinHook.h>
#include <windows.h>
#include <cstdio>
#include <cwchar>
#include <iterator>
#include <mutex>
#include <string>

namespace {
constexpr char pending[] = "Guarded native/reduced SR callbacks; game quality, FG admission and measured latency pending validation.";
std::mutex lifecycle;
bool prepared{}, active{};
rsf_game_host_services upstream{}, services{};
using CreateHost = decltype(&rsf_d3d11_host_create);
using Activate = decltype(&rsf_d3d11_host_set_active);
Activate activate{};
const char* reason = pending;
BOOL CALLBACK count_process_windows(HWND window, LPARAM value) {
    DWORD process{};
    GetWindowThreadProcessId(window, &process);
    if (process == GetCurrentProcessId()) ++*reinterpret_cast<uint32_t*>(value);
    return TRUE;
}
const char* prepare_dpi(const rsf_game_host_services& host) {
    const auto saved_error = GetLastError();
    struct RestoreError { DWORD value; ~RestoreError() { SetLastError(value); } } restore{saved_error};
    uintptr_t renderer{}, device{}, window{};
    const auto read = [](uintptr_t address, uintptr_t& value) {
        SIZE_T copied{};
        return ReadProcessMemory(GetCurrentProcess(), reinterpret_cast<const void*>(address),
            &value, sizeof(value), &copied) && copied == sizeof(value);
    };
    const bool owner_read = read(uintptr_t(GetModuleHandleW(nullptr)) + 0x38e13a0, renderer) &&
        (!renderer || (read(renderer + 0x48, device) && read(renderer + 0x58, window)));
    using SetDpi = BOOL (WINAPI*)(HANDLE);
    using ProcessDpi = HANDLE (WINAPI*)(HANDLE);
    using ThreadDpi = HANDLE (WINAPI*)();
    using EqualDpi = BOOL (WINAPI*)(HANDLE, HANDLE);
    const auto user32 = GetModuleHandleW(L"user32.dll");
    const auto set = reinterpret_cast<SetDpi>(GetProcAddress(user32, "SetProcessDpiAwarenessContext"));
    const auto process = reinterpret_cast<ProcessDpi>(GetProcAddress(user32, "GetDpiAwarenessContextForProcess"));
    const auto thread = reinterpret_cast<ThreadDpi>(GetProcAddress(user32, "GetThreadDpiAwarenessContext"));
    const auto equal = reinterpret_cast<EqualDpi>(GetProcAddress(user32, "AreDpiAwarenessContextsEqual"));
    const auto desired = reinterpret_cast<HANDLE>(INT_PTR(-4));
    const bool available = set && process && thread && equal;
    const auto process_before = available ? process(GetCurrentProcess()) : nullptr;
    const auto thread_before = available ? thread() : nullptr;
    const bool prior_compatible = available && equal(process_before, desired) && equal(thread_before, desired);
    bool requested = false, accepted = false;
    bool windows_read = false;
    uint32_t windows{};
    DWORD error = available ? ERROR_SUCCESS : ERROR_PROC_NOT_FOUND;
    if (owner_read && !device && !window && available && !prior_compatible) {
        SetLastError(ERROR_SUCCESS);
        windows_read = EnumWindows(count_process_windows, reinterpret_cast<LPARAM>(&windows)) != FALSE;
        error = windows_read ? ERROR_SUCCESS : GetLastError();
        if (windows_read && !windows) {
            requested = true;
            SetLastError(ERROR_SUCCESS);
            accepted = set(desired) != FALSE;
            error = accepted ? ERROR_SUCCESS : GetLastError();
        }
    }
    const auto process_after = available ? process(GetCurrentProcess()) : nullptr;
    const auto thread_after = available ? thread() : nullptr;
    const bool verified = owner_read && !device && !window && available &&
        (prior_compatible || (windows_read && !windows)) &&
        equal(process_after, desired) && equal(thread_after, desired);
    if (host.log) {
        char message[480]{};
        std::snprintf(message, sizeof(message),
            "Fallout4 DPI startup PMv2 requested=%u accepted=%u prior_compatible=%u verified=%u error=%lu owner_read=%u device_absent=%u window_absent=%u windows_read=%u process_windows=%u process_before=%p thread_before=%p process_after=%p thread_after=%p; prepare before host graphics setup.",
            requested ? 1u : 0u, accepted ? 1u : 0u, prior_compatible ? 1u : 0u, verified ? 1u : 0u,
            static_cast<unsigned long>(error), owner_read ? 1u : 0u, owner_read && !device ? 1u : 0u,
            owner_read && !window ? 1u : 0u, windows_read ? 1u : 0u, windows,
            process_before, thread_before, process_after, thread_after);
        host.log(host.user, message);
    }
    if (!owner_read || device || window) return "Fallout4 DPI startup refused: native owner unreadable or window/device already created.";
    if (!available) return "Fallout4 DPI startup refused: required awareness APIs unavailable.";
    if (!prior_compatible && (!windows_read || windows)) return "Fallout4 DPI startup refused: window enumeration failed or a process window already exists.";
    if (!verified) return "Fallout4 DPI startup refused: process or caller awareness is incompatible; existing thread awareness preserved.";
    return nullptr;
}
bool busy() {
    return rsf::fallout4::creation_busy() || rsf::fallout4::render_hooks_busy() || rsf::fallout4::cpu_events_busy();
}
void suspend() {
    rsf::fallout4::quiesce_creation();
    rsf::fallout4::quiesce_render_hooks();
    rsf::fallout4::quiesce_cpu_events();
    if (activate && services.user) (void)activate(services.user, 0);
    active = false;
}
bool release_hooks() {
    const bool rendering = rsf::fallout4::stop_render_hooks();
    const bool creation = rsf::fallout4::stop_creation();
    // Keep the CPU owner until its native Swap has closed every render lease.
    return rendering && creation && !busy() && rsf::fallout4::stop_cpu_events();
}
rsf_result refuse(const char* message) {
    reason = message;
    if (services.log) services.log(services.user, message);
    return RSF_ERROR_NATIVE_REFUSED;
}
rsf_result prepare(const rsf_game_prepare_args* args) noexcept try {
    auto valid = rsf_game::validate(args);
    if (valid != RSF_OK) return valid;
    valid = rsf_game::validate(args->host);
    if (valid != RSF_OK) return valid;
    if (!args->host->session_id) return RSF_ERROR_INVALID_ARGUMENT;
    std::lock_guard lock(lifecycle);
    if (prepared || busy()) return RSF_ERROR_BUSY;
    if (!rsf::fallout4::verify_executable()) return refuse("Fallout4 cold creation refused: executable identity mismatch.");
    if (!rsf::fallout4::preflight_creation()) return refuse("Fallout4 cold creation refused: expected native bytes or cold owner unavailable.");
    if (const auto dpi_refusal = prepare_dpi(*args->host)) {
        reason = dpi_refusal;
        if (args->host->log) args->host->log(args->host->user, reason);
        return RSF_ERROR_NATIVE_REFUSED;
    }
    if (!rsf::fallout4::verify_render_hooks() || !rsf::fallout4::verify_render_inputs())
        return refuse("Fallout4 render preparation refused: native call, TAA, resource, camera or depth-domain guard mismatch.");
    const auto initialized = MH_Initialize();
    if (initialized != MH_OK && initialized != MH_ERROR_ALREADY_INITIALIZED)
        return refuse("Fallout4 cold creation refused: native hook initialization failed.");
    HMODULE pinned{};
    if (!GetModuleHandleExW(GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_PIN,
        reinterpret_cast<LPCWSTR>(&prepare), &pinned)) return refuse("Fallout4 native hook lifetime pin refused.");
    const auto runtime = GetModuleHandleW(L"ReScaleFrame.Runtime.dll");
    const auto create = runtime ? reinterpret_cast<CreateHost>(reinterpret_cast<void*>(GetProcAddress(runtime, "rsf_d3d11_host_create"))) : nullptr;
    activate = runtime ? reinterpret_cast<Activate>(reinterpret_cast<void*>(GetProcAddress(runtime, "rsf_d3d11_host_set_active"))) : nullptr;
    if (!create || !activate) return RSF_ERROR_NOT_READY;
    HMODULE runtime_pin{};
    if (!GetModuleHandleExW(GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_PIN,
        reinterpret_cast<LPCWSTR>(runtime), &runtime_pin)) return RSF_ERROR_NOT_READY;
    if (services.user) {
        if (upstream.session_id != args->host->session_id || upstream.user != args->host->user ||
            upstream.log != args->host->log || upstream.cpu_event != args->host->cpu_event)
            return refuse("Fallout4 process-lived creation host requires the original session.");
    } else {
        wchar_t path[32768]{};
        const auto length = GetModuleFileNameW(runtime, path, DWORD(std::size(path)));
        if (!length || length >= std::size(path)) return RSF_ERROR_NOT_READY;
        const auto* separator = std::wcsrchr(path, L'\\');
        if (!separator) return RSF_ERROR_NOT_READY;
        const std::wstring directory(path, size_t(separator - path));
        const auto ini = directory + L"\\ReScaleFrame.ini";
        rsf_d3d11_host_setup setup{};
        setup.struct_size = sizeof(setup); setup.abi_version = RSF_D3D11_HOST_ABI_VERSION;
        setup.configuration_path = ini.c_str(); setup.configuration_section = L"Rendering";
        setup.runtime_directory = directory.c_str(); setup.identity_utf8 = "fallout4";
        setup.executable_sha256 = rsf::fallout4::executable_sha256;
        setup.engine_version_utf8 = "1.11.240.0";
        setup.project_id_utf8 = "8b91a3c1-3ce2-4c66-a35c-42d2074da9fe";
        setup.units_to_meters = 0.01428222656f;
        setup.motion.struct_size = sizeof(setup.motion);
        setup.motion.scale_x = setup.motion.scale_y = 1.0f;
        setup.motion.output_scale_x = setup.motion.output_scale_y = 1.0f;
        setup.colour_transport_default = 1;
        // Reduction requires the independently negotiated recovery export.
        // Older runtimes retain the standard Native control and native raster.
        setup.quality_choices = GetProcAddress(runtime,"rsf_d3d11_host_reduced_frame") ? 0x1f : 1;
        setup.presentation_bridge = 3;
        setup.accept_window = rsf::fallout4::accept_window;
        // Implemented producer declarations for cold admission tests, not acceptance.
        setup.capabilities = RSF_D3D11_HOST_CAP_FG_INPUTS | RSF_D3D11_HOST_CAP_INPUT_TIMELINE;
        auto bound = *args->host;
        const auto result = create(&setup, &bound);
        if (result != RSF_OK) return result;
        services = bound;
        upstream = *args->host;
    }
    if (!rsf::fallout4::prepare_creation(services))
        return refuse("Fallout4 cold creation refused: native bytes, owner, Steam forwarding or scoped services unavailable.");
    if (!rsf::fallout4::prepare_render_hooks(services))
        return refuse("Fallout4 render preparation refused: native guards, synchronous services or hook creation unavailable.");
    if (!rsf::fallout4::prepare_cpu_events(services))
        return refuse("Fallout4 CPU preparation refused: guarded Swap/input callbacks or source services unavailable.");
    prepared = true; reason = pending;
    return RSF_OK;
} catch (...) { return RSF_ERROR_NOT_READY; }
rsf_result start(const rsf_game_start_args* args) noexcept try {
    const auto valid = rsf_game::validate(args);
    if (valid != RSF_OK) return valid;
    std::lock_guard lock(lifecycle);
    if (!prepared) return RSF_ERROR_NOT_READY;
    if (active) return RSF_OK;
    const char* failed = nullptr;
    if (!rsf::fallout4::start_cpu_events()) failed = "Fallout4 start refused: CPU source/input guards or hooks.";
    else if (!rsf::fallout4::start_render_hooks()) failed = "Fallout4 start refused: render/TAA guards or hooks.";
    else if (!rsf::fallout4::start_creation()) failed = "Fallout4 start refused: cold owner, creation call/thunk or hook.";
    if (failed) {
        suspend(); (void)release_hooks(); prepared = false;
        return refuse(failed);
    }
    const auto result = activate(services.user, 1);
    if (result != RSF_OK) { suspend(); (void)release_hooks(); prepared = false; return result; }
    active = true;
    return RSF_OK;
} catch (...) { return RSF_ERROR_NOT_READY; }
rsf_result quiesce(const rsf_game_control_args* args) noexcept try {
    const auto valid = rsf_game::validate(args);
    if (valid != RSF_OK) return valid;
    std::lock_guard lock(lifecycle);
    suspend();
    return busy() ? RSF_ERROR_BUSY : RSF_OK;
} catch (...) { return RSF_ERROR_NOT_READY; }
rsf_result stop(const rsf_game_control_args* args) noexcept try {
    const auto valid = rsf_game::validate(args);
    if (valid != RSF_OK) return valid;
    std::lock_guard lock(lifecycle);
    suspend();
    if (!release_hooks()) return RSF_ERROR_BUSY;
    // Steam's immutable relays and MinHook trampolines remain process-lived.
    prepared = false;
    return RSF_OK;
} catch (...) { return RSF_ERROR_NOT_READY; }
rsf_result status(rsf_game_renderer_status* output) noexcept try {
    const auto valid = rsf_game::validate(output);
    if (valid != RSF_OK) return valid;
    std::lock_guard lock(lifecycle);
    output->prepared = prepared ? 1u : 0u;
    output->active = active ? 1u : 0u;
    output->rendering_ready = 0;
    output->reason = reason;
    return RSF_OK;
} catch (...) { return RSF_ERROR_NOT_READY; }
rsf_detection detect(const rsf_game_probe* probe) noexcept {
    return probe && probe->struct_size >= sizeof(*probe) && probe->pe_machine == 0x8664 &&
        rsf_game::equal_ascii(probe->executable_name_utf8, "Fallout4.exe") &&
        rsf_game::equal_ascii(probe->sha256_hex, rsf::fallout4::executable_sha256) ? RSF_GAME_RECOGNIZED : RSF_GAME_UNKNOWN;
}
}
extern "C" __declspec(dllexport) rsf_result rsf_get_game_plugin_api(uint32_t requested_abi, rsf_game_plugin_api* api) noexcept {
    if (!api || api->struct_size < sizeof(*api)) return RSF_ERROR_INVALID_ARGUMENT;
    if (requested_abi != RSF_GAME_ABI_VERSION) return RSF_ERROR_ABI_MISMATCH;
    *api = {sizeof(*api), RSF_GAME_ABI_VERSION,
        {"fallout4", "Fallout 4", RSF_VERSION_STRING, 0, pending}, detect, {prepare, start, quiesce, stop, status}};
    return RSF_OK;
}
