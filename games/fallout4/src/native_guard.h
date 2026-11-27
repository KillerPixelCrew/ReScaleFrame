/* SPDX-License-Identifier: MIT */
#pragma once
/* The Fallout 4 native leaves' boundary: the shared foreign-exception guard
   (rescaleframe/foreign_guard.h) plus return-address capture and thread-local storage. */
#include <windows.h>
#include <stdint.h>
#include <rescaleframe/foreign_guard.h>

#if defined(_MSC_VER)
#include <intrin.h>
#define RSF_FALLOUT4_CALLER() ((uintptr_t)_ReturnAddress())
#define RSF_FALLOUT4_THREAD_LOCAL __declspec(thread)
#else
#define RSF_FALLOUT4_CALLER() ((uintptr_t)__builtin_return_address(0))
#define RSF_FALLOUT4_THREAD_LOCAL _Thread_local
#endif

/* Best-effort receipt delivery: a throw from the sink is dropped. */
static __inline void rsf_fallout4_report_guarded(void (*report)(const char*), const char* text)
{
    RSF_FOREIGN_TRY { report(text); }
    RSF_FOREIGN_CATCH { }
}