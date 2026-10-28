// SPDX-License-Identifier: GPL-3.0-only
// ASI entry. The game's ASI loader (ScriptHookRDR2 or Ultimate ASI Loader) calls LoadLibrary on
// this module, so no system DLL is impersonated and nothing is forwarded. The directory holding
// the .asi is the deployment root: ReScaleFrame.ini beside it, the runtime and plugin in a
// ReScaleFrame subdirectory.
#include <rescaleframe/module_dump.h>
#ifdef RSF_HAVE_FRAME_CAPTURE
#include <rescaleframe/frame_capture.h>
#endif
#include <rescaleframe/vendor_census.h>
#include <rescaleframe/plugin_session.h>
#include <rescaleframe/render_host.h>
#include "runtime_setup.h"
#include <rescaleframe/sha256_file.h>
#include <windows.h>
#include <MinHook.h>
#include <intrin.h>
#include <cstdint>
#include <cstdio>
#include <cstring>
#include <cwchar>
#include <mutex>
#include <string>
#include "social_club.h"
#include "camera_probe.h"
#include "native_capture.h"
#include "script_input.h"
#include "engine_settings.h"
#include "native_settings.h"

namespace {
HMODULE self = nullptr;
std::wstring log_path;
std::mutex log_guard;
rsf_plugin_session* session = nullptr;
std::string dump_directory;
std::string modules_path;

// RDR2.exe 1.0.1491.50 (PE timestamp 0x65eec92e, SizeOfImage 0x7397600). The engine reads a string
// parameter before the saved API setting: "d3d12" selects D3D12 in settings load, adapter
// enumeration and device resolution. See docs/research/rdr2-renderer.md.
constexpr DWORD expected_timestamp = 0x65eec92e;
constexpr DWORD expected_image_size = 0x7397600;
constexpr DWORD api_override_rva = 0x57abf10;
struct ReadSite { DWORD rva; unsigned char opcode[3]; };
// Every engine read of the parameter: mov r64, [rip + disp32] resolving to api_override_rva.
constexpr ReadSite api_override_reads[] = {
    {0x2c65575, {0x48, 0x8b, 0x1d}},  // settings load
    {0x26b8ebb, {0x48, 0x8b, 0x05}},  // adapter manager API selection
    {0x22a5f8, {0x48, 0x8b, 0x0d}},   // device settings adapter resolution
    {0x2c662f6, {0x48, 0x8b, 0x05}},  // saved API level application
};
const char d3d12_api[] = "d3d12";
char force_result[192] = "ForceD3D12 disabled.";
char census_result[192] = "Vendor census disabled.";
char census_directory[MAX_PATH * 3]{};
char social_club_result[128] = "Social Club backbuffer fix disabled.";
char camera_probe_result[128] = "Camera probe disabled.";
char frame_probe_result[128] = "Frame probe disabled.";
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
#endif

// Opt-in, game-owned HRESULT observation. This catches the first error before RAGE converts a
// failed command-list Reset into ERR_GFX_D3D_DEFERRED_MEM. It does not alter the HRESULT or touch
// any cache, device, command list or allocator. See the matching-build sites in rdr2-renderer.md.
using CheckD3D12Result = HRESULT (*)(HRESULT);
CheckD3D12Result original_check_d3d12_result = nullptr;
wchar_t d3d12_error_path[32768]{};
char d3d12_trace_result[192] = "D3D12 result trace disabled.";

HRESULT trace_d3d12_result(HRESULT result) noexcept
{
    if (FAILED(result)) {
        std::lock_guard<std::mutex> lock(log_guard);
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
                if (address >= base && address - base < expected_image_size) {
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

void install_d3d12_result_trace() noexcept
{
    const auto base = reinterpret_cast<unsigned char*>(GetModuleHandleW(nullptr));
    // ForceD3D12 already validated this exact image and the decrypted read sites.
    if (std::strncmp(force_result, "ForceD3D12 applied", 17) != 0) {
        std::snprintf(d3d12_trace_result, sizeof(d3d12_trace_result), "D3D12 result trace refused: build not validated.");
        return;
    }
    constexpr unsigned char expected[] = {0x40, 0x53, 0x48, 0x83, 0xec, 0x20, 0x8d, 0x81,
        0xfb, 0xff, 0x85, 0x77, 0x8b, 0xd9, 0x83, 0xf8};
    auto* target = base + 0x268b5c0;
    if (std::memcmp(target, expected, sizeof(expected)) != 0) {
        std::snprintf(d3d12_trace_result, sizeof(d3d12_trace_result), "D3D12 result trace refused: unexpected helper bytes.");
        return;
    }
    auto status = MH_Initialize();
    if (status == MH_OK || status == MH_ERROR_ALREADY_INITIALIZED) {
        status = MH_CreateHook(target, reinterpret_cast<void*>(&trace_d3d12_result),
            reinterpret_cast<void**>(&original_check_d3d12_result));
        if (status == MH_OK) status = MH_EnableHook(target);
    }
    std::snprintf(d3d12_trace_result, sizeof(d3d12_trace_result),
        "D3D12 result trace at RVA 0x268b5c0: %s", MH_StatusToString(status));
}
// Runs in DllMain: the ASI loader loads modules during CRT startup, after decryption and before the
// engine reads its settings. Writes only when every expected byte and the null value match.
void force_d3d12() noexcept
{
    const auto base = reinterpret_cast<const unsigned char*>(GetModuleHandleW(nullptr));
    const auto dos = reinterpret_cast<const IMAGE_DOS_HEADER*>(base);
    if (!base || dos->e_magic != IMAGE_DOS_SIGNATURE) {
        std::snprintf(force_result, sizeof(force_result), "ForceD3D12 refused: no main image.");
        return;
    }
    const auto nt = reinterpret_cast<const IMAGE_NT_HEADERS64*>(base + dos->e_lfanew);
    if (nt->Signature != IMAGE_NT_SIGNATURE || nt->FileHeader.TimeDateStamp != expected_timestamp ||
        nt->OptionalHeader.SizeOfImage != expected_image_size) {
        std::snprintf(force_result, sizeof(force_result),
            "ForceD3D12 refused: build differs (timestamp 0x%lx, image 0x%lx).",
            nt->FileHeader.TimeDateStamp, nt->OptionalHeader.SizeOfImage);
        return;
    }
    const auto* section = IMAGE_FIRST_SECTION(nt);
    bool in_data = false;
    for (WORD i = 0; i < nt->FileHeader.NumberOfSections; ++i, ++section) {
        if (!std::memcmp(section->Name, ".data\0\0\0", 8) && api_override_rva >= section->VirtualAddress &&
            api_override_rva + sizeof(void*) <= section->VirtualAddress + section->Misc.VirtualSize) {
            in_data = true;
        }
    }
    if (!in_data) {
        std::snprintf(force_result, sizeof(force_result), "ForceD3D12 refused: parameter outside .data.");
        return;
    }
    for (const auto& site : api_override_reads) {
        const auto* code = base + site.rva;
        std::int32_t displacement = 0;
        std::memcpy(&displacement, code + 3, sizeof(displacement));
        if (std::memcmp(code, site.opcode, 3) || site.rva + 7 + displacement != api_override_rva) {
            std::snprintf(force_result, sizeof(force_result),
                "ForceD3D12 refused: unexpected bytes at RVA 0x%lx.", site.rva);
            return;
        }
    }
    auto* slot = reinterpret_cast<void* volatile*>(const_cast<unsigned char*>(base) + api_override_rva);
    void* const previous = InterlockedCompareExchangePointer(slot, const_cast<char*>(d3d12_api), nullptr);
    if (previous) {
        std::snprintf(force_result, sizeof(force_result),
            "ForceD3D12 refused: parameter already set by another component.");
        return;
    }
    std::snprintf(force_result, sizeof(force_result),
        "ForceD3D12 applied: API parameter at RVA 0x%lx set to \"d3d12\".", api_override_rva);
}

void log(void*, const char* message) noexcept try
{
    std::lock_guard<std::mutex> lock(log_guard);
    FILE* stream = _wfopen(log_path.c_str(), L"ab");
    if (stream) {
        std::fprintf(stream, "[%llu] %s\n", static_cast<unsigned long long>(GetTickCount64()), message);
        std::fclose(stream);
    }
}
catch (...) {}

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

DWORD WINAPI startup(void*) noexcept try
{
    wchar_t filename[32768]{};
    const auto length = GetModuleFileNameW(self, filename, 32768);
    if (!length || length >= 32768) return 1;
    std::wstring root(filename, length);
    root.resize(root.find_last_of(L"/\\") + 1);
    const auto config = root + L"ReScaleFrame.ini";
    if (!GetPrivateProfileIntW(L"RDR2", L"Enable", 1, config.c_str())) return 0;
    const auto mod = root + L"ReScaleFrame\\";
    log_path = mod + L"rdr2.log";
    log(nullptr, "RDR2 ASI entry started.");
    log(nullptr, force_result);
    log(nullptr, d3d12_trace_result);
    log(nullptr, social_club_result);
    log(nullptr, camera_probe_result);
    log(nullptr, frame_probe_result);
    log(nullptr, census_result);
    modules_path = utf8(mod + L"rdr2-modules.log");
    if (GetPrivateProfileIntW(L"RDR2", L"DumpModule", 0, config.c_str())) {
        wchar_t directory[MAX_PATH]{};
        GetPrivateProfileStringW(L"RDR2", L"DumpDirectory", L"", directory, MAX_PATH, config.c_str());
        dump_directory = utf8(directory[0] ? std::wstring(directory) : mod + L"dumps");
    }
    if (HANDLE worker = CreateThread(nullptr, 0, diagnostics, nullptr, 0, nullptr)) CloseHandle(worker);
    const auto runtime_path = mod + L"ReScaleFrame.Runtime.dll";
    const auto runtime = LoadLibraryExW(runtime_path.c_str(), nullptr,
        LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_DEFAULT_DIRS);
    if (!runtime) { log(nullptr, "Runtime DLL load refused."); return 1; }
    const auto prepare = reinterpret_cast<decltype(&rsf_plugin_session_prepare)>(
        reinterpret_cast<void*>(GetProcAddress(runtime, "rsf_plugin_session_prepare")));
    const auto start = reinterpret_cast<decltype(&rsf_plugin_session_start)>(
        reinterpret_cast<void*>(GetProcAddress(runtime, "rsf_plugin_session_start")));
    if (!prepare || !start) { log(nullptr, "Runtime plugin lifecycle exports are unavailable."); return 1; }
    if (!GetModuleFileNameW(nullptr, filename, 32768)) return 1;
    std::string digest;
    if (!rsf::sha256_file(filename, digest)) { log(nullptr, "Executable fingerprint could not be read."); return 1; }
    const rsf_game_probe probe{sizeof(probe), 0x8664, 0, "RDR2.exe", digest.c_str()};
    const auto plugin_path = mod + L"ReScaleFrame.Game.RDR2.dll";
    rsf_plugin_session_options options{};
    options.struct_size = sizeof(options); options.abi_version = RSF_PLUGIN_SESSION_ABI_VERSION;
    options.plugin_path = plugin_path.c_str(); options.probe = &probe;
    options.services.struct_size = sizeof(options.services);
    options.services.abi_version = RSF_GAME_ABI_VERSION;
    options.services.session_id = GetTickCount64() + 1;
    options.services.log = log;
    const auto create_host = reinterpret_cast<decltype(&rsf_render_host_create)>(
        reinterpret_cast<void*>(GetProcAddress(runtime, "rsf_render_host_create")));
    const auto setup = rsf_rdr2_runtime_setup(config.c_str(), mod.c_str());
    if (!create_host || create_host(&setup, &options.services) != RSF_OK) {
        log(nullptr, "RDR2 SR runtime host initialization refused.");
        return 1;
    }
    if (GetPrivateProfileIntW(L"RDR2", L"NativeSettingsProbe", 0, config.c_str())) {
        wchar_t path[32768]{};
        GetPrivateProfileStringW(L"RDR2", L"NativeSettingsProbePath", L"", path, 32768, config.c_str());
        if (!rsf_rdr2_native_settings_probe(path, log, nullptr)) log(nullptr, "Native settings probe refused.");
    }
    const auto prepared = prepare(&options, &session);
    char message[192]{};
    std::snprintf(message, sizeof(message), "RDR2 prepare result=%d sha256=%s", int(prepared), digest.c_str());
    log(nullptr, message);
    if (prepared != RSF_OK) return unsigned(prepared);
    const auto started = start(session);
    std::snprintf(message, sizeof(message), "RDR2 start result=%d", int(started));
    log(nullptr, message);
    if (started == RSF_OK && !rsf_rdr2_start_script_input(self, runtime, config.c_str(), log, nullptr))
        log(nullptr, "RDR2 ScriptHook input guard unavailable; shared overlay still uses its window input hook.");
    rsf_rdr2_report_engine_settings(log, nullptr);
    // Runtime, session and entry callbacks remain owned until process termination.
    return unsigned(started);
}
catch (...) { return 1; }
}

BOOL WINAPI DllMain(HINSTANCE instance, DWORD reason, void*)
{
    if (reason == DLL_PROCESS_ATTACH) {
        self = instance; DisableThreadLibraryCalls(instance);
        static wchar_t executable[32768]{};
        if (GetModuleFileNameW(nullptr, executable, 32768)) {
            const auto* leaf = wcsrchr(executable, L'\\');
            if (!_wcsicmp(leaf ? leaf + 1 : executable, L"RDR2.exe")) {
                static wchar_t config[32768]{};
                const auto length = GetModuleFileNameW(instance, config, 32768);
                auto* separator = length && length < 32768 ? wcsrchr(config, L'\\') : nullptr;
                const bool enabled = separator &&
                    wcscpy_s(separator + 1, 32768 - (separator + 1 - config), L"ReScaleFrame.ini") == 0 &&
                    GetPrivateProfileIntW(L"RDR2", L"Enable", 1, config);
                if (enabled && GetPrivateProfileIntW(L"RDR2", L"ForceD3D12", 1, config)) {
                    force_d3d12();
                    if (std::strncmp(force_result, "ForceD3D12 applied", 17) == 0)
                        rsf_rdr2_own_engine_settings(config);
                }
                if (enabled && std::strncmp(force_result, "ForceD3D12 applied", 17) == 0 &&
                    GetPrivateProfileIntW(L"RDR2", L"NativeCapture", 0, config)) {
                    wchar_t directory[MAX_PATH]{};
                    GetPrivateProfileStringW(L"RDR2", L"CameraProbeDirectory", L"", directory, MAX_PATH, config);
                    if (directory[0]) rsf_rdr2_install_native_capture(directory);
                }
#ifdef RSF_HAVE_FRAME_CAPTURE
                if (enabled && GetPrivateProfileIntW(L"RDR2", L"FrameProbe", 0, config)) {
                    wchar_t library[MAX_PATH]{}, prefix[MAX_PATH]{};
                    char library_utf8[MAX_PATH * 3]{}, prefix_utf8[MAX_PATH * 3]{};
                    GetPrivateProfileStringW(L"RDR2", L"FrameProbeLibrary", L"", library, MAX_PATH, config);
                    GetPrivateProfileStringW(L"RDR2", L"FrameProbePrefix", L"", prefix, MAX_PATH, config);
                    if (library[0] && prefix[0] && wcslen(prefix) < MAX_PATH - 16) {
                        WideCharToMultiByte(CP_UTF8, 0, library, -1, library_utf8, sizeof(library_utf8), nullptr, nullptr);
                        WideCharToMultiByte(CP_UTF8, 0, prefix, -1, prefix_utf8, sizeof(prefix_utf8), nullptr, nullptr);
                        const auto result = rsf_capture_initialise(library_utf8, prefix_utf8);
                        std::snprintf(frame_probe_result, sizeof(frame_probe_result), "Frame probe result=%d", int(result));
                        if (result == RSF_CAPTURE_OK) {
                            swprintf_s(frame_request, L"%s.request", prefix);
                            HANDLE poller = CreateThread(nullptr, 0, frame_requests, nullptr, 0, nullptr);
                            if (poller) CloseHandle(poller);
                        }
                    }
                }
#endif
                // The engine resolves D3D12, DXGI, NGX and NVAPI entry points after CRT startup, so
                // the census must be bound here rather than on the worker thread.
                if (enabled && GetPrivateProfileIntW(L"RDR2", L"Census", 0, config)) {
                    static wchar_t directory[MAX_PATH]{};
                    GetPrivateProfileStringW(L"RDR2", L"CensusDirectory", L"", directory, MAX_PATH, config);
                    if (!directory[0]) {
                        wcsncpy_s(directory, config, separator + 1 - config);
                        wcscat_s(directory, L"ReScaleFrame\\census");
                    }
                    WideCharToMultiByte(CP_UTF8, 0, directory, -1, census_directory,
                        static_cast<int>(sizeof(census_directory)), nullptr, nullptr);
                    rsf_census_options options{};
                    options.struct_size = sizeof(options);
                    options.abi_version = RSF_CENSUS_ABI_VERSION;
                    options.output_dir_utf8 = census_directory;
                    // RSF_CENSUS_FLAG_* bits, for isolating a hook family during a failing run.
                    options.flags = GetPrivateProfileIntW(L"RDR2", L"CensusFlags", 0, config);
                    const auto result = rsf_census_install(&options);
                    std::snprintf(census_result, sizeof(census_result),
                        "Vendor census install result=%d flags=0x%x output=%s", int(result), options.flags,
                        census_directory);
                }
                // Census initializes this ASI's MinHook instance; the optional game trace reuses it.
                if (enabled && GetPrivateProfileIntW(L"RDR2", L"TraceD3D12", 0, config)) {
                    wcsncpy_s(d3d12_error_path, config, separator + 1 - config);
                    wcscat_s(d3d12_error_path, L"ReScaleFrame\\rdr2-d3d12-errors.log");
                    install_d3d12_result_trace();
                }
                if (enabled && std::strncmp(force_result, "ForceD3D12 applied", 17) == 0) {
                    if (GetPrivateProfileIntW(L"RDR2", L"CameraProbe", 0, config)) {
                        wchar_t directory[MAX_PATH]{};
                        GetPrivateProfileStringW(L"RDR2", L"CameraProbeDirectory", L"", directory, MAX_PATH, config);
                        const bool armed = directory[0] && rsf_rdr2_install_camera_probe(directory);
                        std::snprintf(camera_probe_result, sizeof(camera_probe_result),
                            "Camera probe: %s", armed ? "armed" : "refused");
                        if (armed && GetPrivateProfileIntW(L"RDR2", L"CameraReference", 0, config)) {
                            wchar_t library[MAX_PATH]{};
                            GetPrivateProfileStringW(L"RDR2", L"CameraReferenceLibrary", L"", library, MAX_PATH, config);
                            const bool reference = library[0] && rsf_rdr2_install_camera_reference(instance, library);
                            std::snprintf(camera_probe_result, sizeof(camera_probe_result),
                                "Camera probe: armed; ScriptHook reference: %s", reference ? "registered" : "unavailable");
                        }
                    }
                    const bool installed = rsf_rdr2_install_social_club_fix(log,
                        GetPrivateProfileIntW(L"RDR2", L"TraceD3D12", 0, config) != 0);
                    std::snprintf(social_club_result, sizeof(social_club_result),
                        "Social Club backbuffer dispatcher at RVA 0x27f09e8: %s",
                        installed ? "armed" : "refused");
                }
                HANDLE worker = CreateThread(nullptr, 0, startup, nullptr, 0, nullptr);
                if (worker) CloseHandle(worker);
            }
        }
    }
    return TRUE;
}
