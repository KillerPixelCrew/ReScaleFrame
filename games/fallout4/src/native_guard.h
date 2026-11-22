/* SPDX-License-Identifier: MIT */
#pragma once
/* Containment of foreign C++ exceptions at the Fallout 4 native boundary. A native leaf calls
   engine or Runtime code that can throw a C++ exception; Rust frames must never see one. MSVC
   raises a C++ throw as the structured exception 0xE06D7363, so the leaves use __try with a filter
   on that code only. Access violations and other faults keep propagating exactly as they did past
   the former catch (...) under /EHsc. A leaf that must run cleanup on both paths uses __try with
   __finally. Other toolchains have no foreign unwinding to contain, so the same source degrades
   to the plain sequence. */
#include <windows.h>
#include <stdint.h>

#if defined(_MSC_VER)
#include <intrin.h>
#define RSF_FALLOUT4_TRY __try
#define RSF_FALLOUT4_FINALLY __finally
#define RSF_FALLOUT4_CATCH_CXX __except (rsf_fallout4_cxx_filter(GetExceptionCode()))
#define RSF_FALLOUT4_CALLER() ((uintptr_t)_ReturnAddress())
#define RSF_FALLOUT4_THREAD_LOCAL __declspec(thread)

static __inline int rsf_fallout4_cxx_filter(unsigned long code)
{
    return code == 0xE06D7363ul ? EXCEPTION_EXECUTE_HANDLER : EXCEPTION_CONTINUE_SEARCH;
}
#else
#define RSF_FALLOUT4_TRY
#define RSF_FALLOUT4_FINALLY
#define RSF_FALLOUT4_CATCH_CXX if (0)
#define RSF_FALLOUT4_CALLER() ((uintptr_t)__builtin_return_address(0))
#define RSF_FALLOUT4_THREAD_LOCAL _Thread_local
#endif

/* Best-effort receipt delivery: a throw from the sink is dropped. */
static __inline void rsf_fallout4_report_guarded(void (*report)(const char*), const char* text)
{
    RSF_FALLOUT4_TRY { report(text); }
    RSF_FALLOUT4_CATCH_CXX { }
}
