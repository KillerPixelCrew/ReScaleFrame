// SPDX-License-Identifier: GPL-3.0-only
#include <rescaleframe/game_api.h>
#include <rescaleframe/game_plugin_util.h>
#include <rescaleframe/version.h>

namespace {
using rsf_game::equal_ascii;
using rsf_game::validate;

constexpr char known_sha256[] =
    "092e84225624a4de9c05d2404ff55269bd4a2aa2ff0548f2a183c6bf36abc85a";
constexpr char renderer_status[] =
    "Project Wingman renderer hooks are not implemented; static research only.";

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
    // No hooks or host-service references are retained by the scaffold.
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

rsf_detection detect(const rsf_game_probe* probe) noexcept
{
    if (!probe || probe->struct_size < sizeof(rsf_game_probe) || probe->pe_machine != 0x8664) {
        return RSF_GAME_UNKNOWN;
    }
    return equal_ascii(probe->executable_name_utf8, "ProjectWingman-Win64-Shipping.exe") &&
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
        {"project-wingman", "Project Wingman", RSF_VERSION_STRING, 0, renderer_status},
        detect, {prepare, start, quiesce, stop, status}};
    return RSF_OK;
}
