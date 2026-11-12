// SPDX-License-Identifier: GPL-3.0-only
#include <rescaleframe/game_api.h>
#include <rescaleframe/game_plugin_util.h>
#include <rescaleframe/version.h>
#include "native_observer.h"
#include "build_profile.h"

namespace {
rsf_detection detect(const rsf_game_probe* probe) noexcept {
    if (!probe || probe->struct_size < sizeof(*probe) || probe->pe_machine != 0x8664)
        return RSF_GAME_UNKNOWN;
    // Exactly one of the profiled builds (build_profile.h): both name and SHA-256 must match the same entry.
    for (const auto& build : rsf::anomaly::build_profiles)
        if (rsf_game::equal_ascii(probe->executable_name_utf8, build.executable) &&
            rsf_game::equal_ascii(probe->sha256_hex, build.sha256)) return RSF_GAME_RECOGNIZED;
    return RSF_GAME_UNKNOWN;
}
}
extern "C" __declspec(dllexport) rsf_result rsf_get_game_plugin_api(
    uint32_t requested_abi, rsf_game_plugin_api* api) noexcept {
    if (!api || api->struct_size < sizeof(*api)) return RSF_ERROR_INVALID_ARGUMENT;
    if (requested_abi != RSF_GAME_ABI_VERSION) return RSF_ERROR_ABI_MISMATCH;
    *api = {sizeof(*api), RSF_GAME_ABI_VERSION,
        {"anomaly", "STALKER Anomaly", RSF_VERSION_STRING, 0,
         "Guarded native boundary observation; temporal SR/FG execution is not validated."}, detect,
        {rsf::anomaly::prepare, rsf::anomaly::start, rsf::anomaly::quiesce,
         rsf::anomaly::stop, rsf::anomaly::status}};
    return RSF_OK;
}
