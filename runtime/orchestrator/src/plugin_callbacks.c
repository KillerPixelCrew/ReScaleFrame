/* SPDX-License-Identifier: MIT */
/* Contain C++ exceptions from native plugins before control returns to Rust.
   MSVC raises a C++ exception as SEH code 0xE06D7363, so a filter on that code stops the same
   exceptions the former catch-all stopped. Access violations and other faults still propagate. */
#include <rescaleframe/game_api.h>

#ifndef WIN32_LEAN_AND_MEAN
#define WIN32_LEAN_AND_MEAN
#endif
#ifndef NOMINMAX
#define NOMINMAX
#endif
#include <windows.h>

#define RSF_CXX_EXCEPTION_CODE 0xE06D7363UL

typedef rsf_result (*rsf_plugin_control_fn)(const rsf_game_control_args* args);

#if defined(_MSC_VER)
#define RSF_FOREIGN_TRY __try
#define RSF_FOREIGN_CATCH(on_exception) \
    __except (GetExceptionCode() == RSF_CXX_EXCEPTION_CODE ? EXCEPTION_EXECUTE_HANDLER \
                                                           : EXCEPTION_CONTINUE_SEARCH) { on_exception; }
#else
#error "Foreign C++ exception containment needs MSVC structured exception handling"
#endif

void rsf_call_native_log(rsf_game_log_fn log, void* user, const char* text)
{
    RSF_FOREIGN_TRY {
        if (log) log(user, text);
    } RSF_FOREIGN_CATCH((void)0)
}

rsf_result rsf_plugin_call_query(rsf_get_game_plugin_api_fn entry, uint32_t version, rsf_game_plugin_api* api)
{
    RSF_FOREIGN_TRY {
        return entry(version, api);
    } RSF_FOREIGN_CATCH(return RSF_ERROR_NOT_READY)
}

rsf_result rsf_plugin_call_detect(rsf_detect_game_fn detect, const rsf_game_probe* probe, rsf_detection* detection)
{
    RSF_FOREIGN_TRY {
        *detection = detect(probe);
        return RSF_OK;
    } RSF_FOREIGN_CATCH(return RSF_ERROR_NOT_READY)
}

rsf_result rsf_plugin_call_prepare(const rsf_game_hooks_api* hooks, const rsf_game_prepare_args* args)
{
    RSF_FOREIGN_TRY {
        return hooks->prepare(args);
    } RSF_FOREIGN_CATCH(return RSF_ERROR_NOT_READY)
}

rsf_result rsf_plugin_call_start(const rsf_game_hooks_api* hooks, const rsf_game_start_args* args)
{
    RSF_FOREIGN_TRY {
        return hooks->start(args);
    } RSF_FOREIGN_CATCH(return RSF_ERROR_NOT_READY)
}

rsf_result rsf_plugin_call_control(rsf_plugin_control_fn callback, const rsf_game_control_args* args)
{
    RSF_FOREIGN_TRY {
        return callback(args);
    } RSF_FOREIGN_CATCH(return RSF_ERROR_NOT_READY)
}

rsf_result rsf_plugin_call_status(const rsf_game_hooks_api* hooks, rsf_game_renderer_status* out)
{
    RSF_FOREIGN_TRY {
        return hooks->status(out);
    } RSF_FOREIGN_CATCH(return RSF_ERROR_NOT_READY)
}
