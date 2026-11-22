// SPDX-License-Identifier: MIT
#include <rescaleframe/game_api.h>

extern "C" {
bool rsf_test_session_detect(const rsf_game_probe*, rsf_detection*);
bool rsf_test_session_prepare(const rsf_game_prepare_args*, rsf_result*);
rsf_result rsf_test_session_query(uint32_t, rsf_game_plugin_api*, rsf_detect_game_fn,
    decltype(rsf_game_hooks_api::prepare));
}

namespace {
// Throw only after the Rust callback returns, so the Runtime exercises native unwinding.
rsf_detection detect(const rsf_game_probe* probe)
{
    rsf_detection result{};
    if (!rsf_test_session_detect(probe, &result)) throw 1;
    return result;
}

rsf_result prepare(const rsf_game_prepare_args* args)
{
    rsf_result result{};
    if (!rsf_test_session_prepare(args, &result)) throw 1;
    return result;
}
}

extern "C" __declspec(dllexport) rsf_result rsf_get_game_plugin_api(uint32_t abi, rsf_game_plugin_api* api)
{
    return rsf_test_session_query(abi, api, detect, prepare);
}
