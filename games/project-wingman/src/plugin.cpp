// SPDX-License-Identifier: MIT
#include <rescaleframe/game_api.h>
#include "native_observer.h"

extern "C" rsf_result rsf_wingman_publish_api(
    uint32_t requested_abi, rsf_game_plugin_api* api, const rsf::wingman::ObserverPlatform* platform) noexcept;

extern "C" __declspec(dllexport) rsf_result rsf_get_game_plugin_api(
    uint32_t requested_abi, rsf_game_plugin_api* api) noexcept
{
    return rsf_wingman_publish_api(requested_abi, api, &rsf::wingman::observer_platform());
}
