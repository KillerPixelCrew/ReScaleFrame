// SPDX-License-Identifier: GPL-3.0-only
#include <rescaleframe/game_api.h>
#include <rescaleframe/version.h>

#include <cstddef>

namespace {
constexpr char drag_n_wash_sha256[] =
    "5fdfffe386a2f43b77626cd3d70554d84c6588c94d309544924d6fab088ddafc";
constexpr char renderer_status[] =
    "Unity Mono scaffold only; managed bootstrap, Harmony adapter and renderer hooks are not implemented.";

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
    // The scaffold retains no host services and installs no native or managed hooks.
    return RSF_ERROR_NOT_READY;
}

rsf_result start(const rsf_game_start_args* args) noexcept
{
    const auto valid = validate(args);
    return valid == RSF_OK ? RSF_ERROR_NOT_READY : valid;
}

rsf_result quiesce(const rsf_game_control_args* args) noexcept
{
    return validate(args);
}

rsf_result stop(const rsf_game_control_args* args) noexcept
{
    return validate(args);
}

rsf_result status(rsf_game_renderer_status* output) noexcept
{
    const auto valid = validate(output);
    if (valid != RSF_OK) {
        return valid;
    }
    output->prepared = 0;
    output->active = 0;
    output->rendering_ready = 0;
    output->reason = renderer_status;
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
