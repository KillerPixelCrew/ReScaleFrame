/* SPDX-License-Identifier: MIT */
#pragma once
/* The native boundary every C leaf uses. Rust frames must never see a foreign C++ exception.

   RSF_FOREIGN_CATCH handles only foreign C++ exceptions. MSVC raises a C++ throw as the
   structured exception 0xE06D7363, so access violations and other faults keep propagating
   exactly as they did past the former catch (...) under /EHsc. Use it around calls into the
   Runtime, the host, vendor SDKs and engine code that may throw.

   RSF_FAULT_CATCH handles every structured exception. Use it only around reads or calls on
   borrowed foreign objects whose memory may already be gone (engine pointers, private renderer
   objects), where the leaf reports "unavailable" instead.

   Write RSF_FOREIGN_TRY { ... } RSF_FOREIGN_CATCH { ... }, and RSF_FOREIGN_TRY { ... }
   RSF_FOREIGN_FINALLY { ... } for cleanup on both paths. Other toolchains have no structured
   exceptions: the guarded block runs as a plain block and the handler never runs. */
#include <stdbool.h>
#include <rescaleframe/game_api.h>

/* The structured exception code of an MSVC C++ throw (fixtures raise it directly). */
#define RSF_CXX_EXCEPTION_CODE 0xE06D7363ul

#if defined(_MSC_VER)
#include <excpt.h>
#define RSF_FOREIGN_TRY __try
#define RSF_FOREIGN_FINALLY __finally
#define RSF_FOREIGN_CATCH \
    __except (GetExceptionCode() == RSF_CXX_EXCEPTION_CODE ? EXCEPTION_EXECUTE_HANDLER : EXCEPTION_CONTINUE_SEARCH)
#define RSF_FAULT_CATCH __except (EXCEPTION_EXECUTE_HANDLER)
#else
#define RSF_FOREIGN_TRY
#define RSF_FOREIGN_FINALLY
#define RSF_FOREIGN_CATCH if (0)
#define RSF_FAULT_CATCH if (0)
#endif

/* Calls call(context); false when it threw a foreign C++ exception. */
static __inline bool rsf_foreign_call(void (*call)(void*), void* context)
{
    bool returned = true;
    RSF_FOREIGN_TRY { call(context); }
    RSF_FOREIGN_CATCH { returned = false; }
    return returned;
}

#ifdef __cplusplus
extern "C" {
#endif
/* Calls the host log with message; false when it threw. A null log succeeds without a call.
   Defined once (sdk/game/src/foreign_guard.c) for rsf_game_sys::host_log. */
bool rsf_game_log_guarded(rsf_game_log_fn log, void* user, const char* message);
#ifdef __cplusplus
}
#endif