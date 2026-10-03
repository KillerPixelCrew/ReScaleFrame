// SPDX-License-Identifier: GPL-3.0-only
#include <rescaleframe/game_api.h>
#include <rescaleframe/version.h>
#include "bridge.h"
#include "mono_runtime.h"
#include <windows.h>

#include <cstddef>
#include <mutex>
#include <string>

namespace {
constexpr char drag_n_wash_sha256[] =
    "5fdfffe386a2f43b77626cd3d70554d84c6588c94d309544924d6fab088ddafc";
constexpr char renderer_status[] = "Unity Mono SR adapter built; game validation is pending.";
std::mutex lifecycle;
bool prepared = false, running = false;
rsf_unity_native_api managed_api{};
const char* reason = "Unity Mono adapter has not been prepared.";

std::wstring managed_path()
{
    HMODULE module = nullptr;
    GetModuleHandleExW(GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
        reinterpret_cast<const wchar_t*>(managed_path), &module);
    wchar_t path[32768]{};
    const DWORD length = GetModuleFileNameW(module, path, 32768);
    if (!length || length >= 32768) return {};
    std::wstring result(path, length);
    const auto slash = result.find_last_of(L"/\\");
    if (slash == std::wstring::npos) return {};
    result.resize(slash + 1); result += L"ReScaleFrame.Unity.Managed.dll";
    return result;
}

template<class T> rsf_result validate(const T* value) noexcept
{
    if (!value || value->struct_size < sizeof(T)) {
        return RSF_ERROR_INVALID_ARGUMENT;
    }
    return value->abi_version == RSF_GAME_ABI_VERSION ? RSF_OK : RSF_ERROR_ABI_MISMATCH;
}

rsf_result prepare(const rsf_game_prepare_args* args) noexcept
{
    const auto valid = validate(args);
    if (valid != RSF_OK) {
        return valid;
    }
    const auto host_valid = validate(args->host);
    if (host_valid != RSF_OK) {
        return host_valid;
    }
    if (!args->host->session_id) {
        return RSF_ERROR_INVALID_ARGUMENT;
    }
    try {
        std::lock_guard<std::mutex> lock(lifecycle);
        if (prepared) return RSF_ERROR_BUSY;
        if (!rsf_unity_bridge_prepare(*args->host, &managed_api)) return RSF_ERROR_NOT_READY;
        const auto path = managed_path();
        prepared = !path.empty() && rsf_unity_mono_start(path.c_str(), &managed_api, &reason);
        if (!prepared && args->host->log) args->host->log(args->host->user, reason);
        if (!prepared) rsf_unity_bridge_release();
        return prepared ? RSF_OK : RSF_ERROR_NOT_READY;
    } catch (...) { return RSF_ERROR_NOT_READY; }
}

rsf_result start(const rsf_game_start_args* args) noexcept
{
    const auto valid = validate(args);
    if (valid != RSF_OK) return valid;
    std::lock_guard<std::mutex> lock(lifecycle);
    if (!prepared) return RSF_ERROR_NOT_READY;
    rsf_unity_bridge_activate(true); running = true;
    reason = "Unity managed adapter active; native provider execution and image validation pending.";
    return RSF_OK;
}

rsf_result quiesce(const rsf_game_control_args* args) noexcept
{
    const auto valid = validate(args); if (valid != RSF_OK) return valid;
    std::lock_guard<std::mutex> lock(lifecycle);
    rsf_unity_bridge_activate(false); running = false;
    reason = "Unity producers quiesced; queued commands may still need to drain.";
    return RSF_OK;
}

rsf_result stop(const rsf_game_control_args* args) noexcept
{
    const auto valid = validate(args); if (valid != RSF_OK) return valid;
    std::lock_guard<std::mutex> lock(lifecycle);
    if (running || !rsf_unity_bridge_drained()) return RSF_ERROR_BUSY;
    if (!rsf_unity_mono_stop()) return RSF_ERROR_BUSY;
    rsf_unity_bridge_release(); prepared = false;
    reason = "Unity adapter stopped; managed assembly remains inert in the player domain.";
    return RSF_OK;
}

rsf_result status(rsf_game_renderer_status* output) noexcept
{
    const auto valid = validate(output);
    if (valid != RSF_OK) {
        return valid;
    }
    std::lock_guard<std::mutex> lock(lifecycle);
    output->prepared = prepared ? 1u : 0u;
    const auto stage = rsf_unity_bridge_managed_state();
    output->active = running && stage == 2 ? 1u : 0u;
    output->rendering_ready = 0;
    output->reason = stage == 3 ? "Managed Unity adapter refused its pipeline contract; see diagnostics." : reason;
    return RSF_OK;
}

char ascii_lower(char value) noexcept
{
    return value >= 'A' && value <= 'Z' ? static_cast<char>(value + ('a' - 'A')) : value;
}

bool equal_ascii(const char* value, const char* expected) noexcept
{
    if (!value || !expected) {
        return false;
    }
    for (std::size_t i = 0;; ++i) {
        if (ascii_lower(value[i]) != ascii_lower(expected[i])) {
            return false;
        }
        if (expected[i] == '\0') {
            return true;
        }
    }
}

rsf_detection detect(const rsf_game_probe* probe) noexcept
{
    if (!probe || probe->struct_size < sizeof(rsf_game_probe) || probe->pe_machine != 0x8664) {
        return RSF_GAME_UNKNOWN;
    }
    // The shared engine plugin starts with a researched-build allowlist. A Unity-looking file
    // layout alone does not establish that a managed adapter is compatible with a game.
    return equal_ascii(probe->executable_name_utf8, "DragNWash.exe") &&
                   equal_ascii(probe->sha256_hex, drag_n_wash_sha256)
               ? RSF_GAME_RECOGNIZED
               : RSF_GAME_UNKNOWN;
}
}

extern "C" __declspec(dllexport) rsf_result rsf_get_game_plugin_api(
    uint32_t requested_abi, rsf_game_plugin_api* api) noexcept
{
    if (!api || api->struct_size < sizeof(rsf_game_plugin_api)) {
        return RSF_ERROR_INVALID_ARGUMENT;
    }
    if (requested_abi != RSF_GAME_ABI_VERSION) {
        return RSF_ERROR_ABI_MISMATCH;
    }
    *api = {
        sizeof(rsf_game_plugin_api), RSF_GAME_ABI_VERSION,
        {"unity-mono", "Unity Mono", RSF_VERSION_STRING, 0, renderer_status},
        detect, {prepare, start, quiesce, stop, status}};
    return RSF_OK;
}
