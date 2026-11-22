// SPDX-License-Identifier: MIT
// Contain exceptions from native plugins before control returns to Rust.
#include <rescaleframe/game_api.h>
extern "C" void rsf_call_native_log(rsf_game_log_fn log, void* user, const char* text) noexcept try
{
    if (log) log(user, text);
} catch (...) {}

extern "C" rsf_result rsf_plugin_call_query(rsf_get_game_plugin_api_fn entry,
    uint32_t version, rsf_game_plugin_api* api) noexcept try
{
    return entry(version, api);
} catch (...) { return RSF_ERROR_NOT_READY; }

extern "C" rsf_result rsf_plugin_call_detect(rsf_detect_game_fn detect,
    const rsf_game_probe* probe, rsf_detection* detection) noexcept try
{
    *detection = detect(probe);
    return RSF_OK;
} catch (...) { return RSF_ERROR_NOT_READY; }

extern "C" rsf_result rsf_plugin_call_prepare(const rsf_game_hooks_api* h,
    const rsf_game_prepare_args* args) noexcept try
{
    return h->prepare(args);
} catch (...) { return RSF_ERROR_NOT_READY; }

extern "C" rsf_result rsf_plugin_call_start(const rsf_game_hooks_api* h,
    const rsf_game_start_args* args) noexcept try
{
    return h->start(args);
} catch (...) { return RSF_ERROR_NOT_READY; }

extern "C" rsf_result rsf_plugin_call_control(decltype(rsf_game_hooks_api::stop) callback,
    const rsf_game_control_args* args) noexcept try
{
    return callback(args);
} catch (...) { return RSF_ERROR_NOT_READY; }

extern "C" rsf_result rsf_plugin_call_status(const rsf_game_hooks_api* h, rsf_game_renderer_status* out) noexcept try
{
    return h->status(out);
} catch (...) { return RSF_ERROR_NOT_READY; }
