// SPDX-License-Identifier: GPL-3.0-only
#include <rescaleframe/game_api.h>
#include <rescaleframe/game_plugin_util.h>
#include <rescaleframe/version.h>
#include "native_observer.h"

namespace {
using rsf_game::equal_ascii;
constexpr char renderer_status[] =
    "Native temporal GPU input leases; SR, FG, latency and VR processing are unimplemented.";

rsf_detection detect(const rsf_game_probe* probe) noexcept
{
    if (!probe || probe->struct_size < sizeof(rsf_game_probe) || probe->pe_machine != 0x8664) {
        return RSF_GAME_UNKNOWN;
    }
    return equal_ascii(probe->executable_name_utf8, "ProjectWingman-Win64-Shipping.exe") &&
                   equal_ascii(probe->sha256_hex, rsf::wingman::known_sha256)
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
        detect, {rsf::wingman::prepare, rsf::wingman::start, rsf::wingman::quiesce,
                 rsf::wingman::stop, rsf::wingman::status}};
    return RSF_OK;
}
