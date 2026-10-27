// SPDX-License-Identifier: GPL-3.0-only
#include <rescaleframe/game_api.h>
#include <rescaleframe/version.h>

#include <cstddef>
#include "native_renderer.h"

namespace {
// Steam RDR2.exe 1.0.1491.50, PE timestamp 2024-03-11. See engine.json.
constexpr char known_sha256[] =
    "b56c9548f670654a9b73bf25def3cd73af12e269f6e47dba28a34079adaf465e";
constexpr char renderer_status[] =
    "Native DLSS SR replacement implemented; game acceptance, FSR routing and FG pending.";

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
    return rsf_rdr2_renderer_prepare(*args->host);
}

rsf_result start(const rsf_game_start_args* args) noexcept
{
    const auto valid = validate(args);
    return valid == RSF_OK ? rsf_rdr2_renderer_start() : valid;
}

rsf_result quiesce(const rsf_game_control_args* args) noexcept
{
    const auto valid = validate(args);
    return valid == RSF_OK ? rsf_rdr2_renderer_quiesce() : valid;
}

rsf_result stop(const rsf_game_control_args* args) noexcept
{
    const auto valid = validate(args);
    return valid == RSF_OK ? rsf_rdr2_renderer_stop() : valid;
}

rsf_result status(rsf_game_renderer_status* output) noexcept
{
    const auto valid = validate(output);
    if (valid != RSF_OK) {
        return valid;
    }
    rsf_rdr2_renderer_status(*output);
    return RSF_OK;
}

char ascii_lower(char value) noexcept
{
    return value >= 'A' && value <= 'Z' ? static_cast<char>(value + ('a' - 'A')) : value;
}

bool equal_ascii(const char* value, const char* expected) noexcept
{
    if (!value) {
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
    return equal_ascii(probe->executable_name_utf8, "RDR2.exe") &&
                   equal_ascii(probe->sha256_hex, known_sha256)
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
        {"rdr2", "Red Dead Redemption 2", RSF_VERSION_STRING, 0, renderer_status},
        detect, {prepare, start, quiesce, stop, status}};
    return RSF_OK;
}
