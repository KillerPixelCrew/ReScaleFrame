// SPDX-License-Identifier: MIT
#include <rescaleframe/game_api.h>

rsf_result rsf_skyrim_publish_api(uint32_t requested_abi, rsf_game_plugin_api* api);

__declspec(dllexport) rsf_result rsf_get_game_plugin_api(uint32_t requested_abi, rsf_game_plugin_api* api)
{
    return rsf_skyrim_publish_api(requested_abi, api);
}
