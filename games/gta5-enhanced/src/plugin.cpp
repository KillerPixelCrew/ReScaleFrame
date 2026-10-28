// SPDX-License-Identifier: GPL-3.0-only
#include <rescaleframe/game_api.h>
#include <rescaleframe/game_plugin_util.h>
#include <rescaleframe/version.h>
#include "build_identity.h"
#include "native_renderer.h"

namespace {
constexpr char reason[] =
    "GTA V Enhanced native SR adapter; integrated FG and gameplay acceptance are in progress.";

rsf_detection detect(const rsf_game_probe* probe) noexcept
{
    if (!probe || probe->struct_size < sizeof(*probe) || probe->pe_machine != 0x8664 ||
        !rsf_game::equal_ascii(probe->executable_name_utf8, "GTA5_Enhanced.exe")) return RSF_GAME_UNKNOWN;
    return rsf_game::equal_ascii(probe->sha256_hex, rsf_gta5_enhanced::executable_sha256)
        ? RSF_GAME_RECOGNIZED : RSF_GAME_UNKNOWN;
}

rsf_result prepare(const rsf_game_prepare_args* args) noexcept
{
    auto result = rsf_game::validate(args);
    if (result != RSF_OK) return result;
    result = rsf_game::validate(args->host);
    if (result != RSF_OK) return result;
    if (!args->host->session_id) return RSF_ERROR_INVALID_ARGUMENT;
    return rsf_gta5_renderer_prepare(*args->host);
}
rsf_result start(const rsf_game_start_args* args) noexcept
{
    const auto result = rsf_game::validate(args);
    return result == RSF_OK ? rsf_gta5_renderer_start() : result;
}
rsf_result quiesce(const rsf_game_control_args* args) noexcept {
    const auto valid = rsf_game::validate(args);
    return valid == RSF_OK ? rsf_gta5_renderer_quiesce() : valid;
}
rsf_result stop(const rsf_game_control_args* args) noexcept {
    const auto valid = rsf_game::validate(args);
    return valid == RSF_OK ? rsf_gta5_renderer_stop() : valid;
}
rsf_result status(rsf_game_renderer_status* output) noexcept
{
    const auto result = rsf_game::validate(output);
    if (result != RSF_OK) return result;
    rsf_gta5_renderer_status(*output);
    return RSF_OK;
}
}

extern "C" __declspec(dllexport) rsf_result rsf_get_game_plugin_api(
    uint32_t requested_abi, rsf_game_plugin_api* api) noexcept
{
    if (!api || api->struct_size < sizeof(*api)) return RSF_ERROR_INVALID_ARGUMENT;
    if (requested_abi != RSF_GAME_ABI_VERSION) return RSF_ERROR_ABI_MISMATCH;
    *api = {sizeof(*api), RSF_GAME_ABI_VERSION,
        {"gta5-enhanced", "Grand Theft Auto V Enhanced", RSF_VERSION_STRING, 0, reason},
        detect, {prepare, start, quiesce, stop, status}};
    return RSF_OK;
}
