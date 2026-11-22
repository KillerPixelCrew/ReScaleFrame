/* SPDX-License-Identifier: MIT */
/* Deliberate foreign C++ exception fixture. C cannot throw a C++ object, so the callbacks raise the
   MSVC C++ exception code directly. The Runtime guard filters on that code, so this exercises the
   same containment path as a thrown object. */
#include <rescaleframe/game_api.h>
#include <stdbool.h>

#ifndef WIN32_LEAN_AND_MEAN
#define WIN32_LEAN_AND_MEAN
#endif
#ifndef NOMINMAX
#define NOMINMAX
#endif
#include <windows.h>

#define RSF_CXX_EXCEPTION_CODE 0xE06D7363UL
#define RSF_CXX_EXCEPTION_MAGIC 0x19930520UL

bool rsf_test_session_detect(const rsf_game_probe*, rsf_detection*);
bool rsf_test_session_prepare(const rsf_game_prepare_args*, rsf_result*);
rsf_result rsf_test_session_query(uint32_t, rsf_game_plugin_api*, rsf_detect_game_fn,
    rsf_result (*)(const rsf_game_prepare_args*));

static void raise_foreign_exception(void)
{
    ULONG_PTR parameters[4] = { RSF_CXX_EXCEPTION_MAGIC, 0, 0, 0 };
    RaiseException(RSF_CXX_EXCEPTION_CODE, EXCEPTION_NONCONTINUABLE, 4, parameters);
}

/* Raise only after the Rust callback returns, so the Runtime exercises native unwinding. */
static rsf_detection detect(const rsf_game_probe* probe)
{
    rsf_detection result = 0;
    if (!rsf_test_session_detect(probe, &result)) raise_foreign_exception();
    return result;
}

static rsf_result prepare(const rsf_game_prepare_args* args)
{
    rsf_result result = 0;
    if (!rsf_test_session_prepare(args, &result)) raise_foreign_exception();
    return result;
}

__declspec(dllexport) rsf_result rsf_get_game_plugin_api(uint32_t abi, rsf_game_plugin_api* api)
{
    return rsf_test_session_query(abi, api, detect, prepare);
}
