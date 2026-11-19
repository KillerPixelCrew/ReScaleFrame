// SPDX-License-Identifier: MIT
// Contain exceptions from native plugins before control returns to Rust.
#include <rescaleframe/game_api.h>
extern "C" rsf_result rsf_plugin_call_query(rsf_get_game_plugin_api_fn entry,
    rsf_game_plugin_api* api) noexcept try
{
    return entry(RSF_GAME_ABI_VERSION, api);
} catch (...) { return RSF_ERROR_NOT_READY; }

extern "C" rsf_result rsf_plugin_call_detect(rsf_detect_game_fn detect,
    const rsf_game_probe* probe) noexcept try
{
    return detect && detect(probe) == RSF_GAME_RECOGNIZED ? RSF_OK : RSF_ERROR_NATIVE_REFUSED;
} catch (...) { return RSF_ERROR_NOT_READY; }

extern "C" rsf_result rsf_plugin_call_prepare(const rsf_game_hooks_api* h,
    const rsf_game_host_services* services) noexcept try
{
    const rsf_game_prepare_args args{sizeof(args), RSF_GAME_ABI_VERSION, services};
    return h->prepare(&args);
} catch (...) { return RSF_ERROR_NOT_READY; }

extern "C" rsf_result rsf_plugin_call_start(const rsf_game_hooks_api* h) noexcept try
{
    const rsf_game_start_args args{sizeof(args), RSF_GAME_ABI_VERSION};
    return h->start(&args);
} catch (...) { return RSF_ERROR_NOT_READY; }

extern "C" rsf_result rsf_plugin_call_control(decltype(rsf_game_hooks_api::stop) callback) noexcept try
{
    const rsf_game_control_args args{sizeof(args), RSF_GAME_ABI_VERSION};
    return callback(&args);
} catch (...) { return RSF_ERROR_NOT_READY; }

extern "C" rsf_result rsf_plugin_call_status(const rsf_game_hooks_api* h, rsf_game_renderer_status* out) noexcept try
{
    return h->status(out);
} catch (...) { return RSF_ERROR_NOT_READY; }
