// SPDX-License-Identifier: MIT
// ReScaleFrame.Game.RDR2.dll entry. Recognition, lifecycle and renderer policy are owned by the
// Rust crate rsf-rdr2 (games/rdr2/src/lib.rs); this file keeps the exported C ABI entry and lends
// the native leaf table (native_platform.cpp) to it.
#include <rescaleframe/game_api.h>
#include "native_platform.h"

extern "C" rsf_result rsf_rdr2_publish_api(uint32_t requested_abi, rsf_game_plugin_api* api,
    const rsf::rdr2::Platform* platform) noexcept;

extern "C" __declspec(dllexport) rsf_result rsf_get_game_plugin_api(
    uint32_t requested_abi, rsf_game_plugin_api* api) noexcept
{
    return rsf_rdr2_publish_api(requested_abi, api, &rsf::rdr2::platform());
}
