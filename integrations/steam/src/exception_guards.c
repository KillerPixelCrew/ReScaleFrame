/* SPDX-License-Identifier: MIT */
/* Contain C++ exceptions from the Runtime service and host log callback before control returns to
   Rust. MSVC raises a C++ exception as SEH code 0xE06D7363, so a filter on that code stops the
   same exceptions the former catch-all stopped. Access violations and other faults still propagate. */
#include <rescaleframe/d3d11_host.h>
#include <stddef.h>

#ifndef WIN32_LEAN_AND_MEAN
#define WIN32_LEAN_AND_MEAN
#endif
#ifndef NOMINMAX
#define NOMINMAX
#endif
#include <windows.h>

#if !defined(_MSC_VER)
#error "Foreign C++ exception containment needs MSVC structured exception handling"
#endif

#define RSF_CXX_EXCEPTION_CODE 0xE06D7363UL

_Static_assert(sizeof(rsf_d3d11_chain_dispatch) == 72 && offsetof(rsf_d3d11_chain_dispatch, args) == 24 &&
    offsetof(rsf_d3d11_chain_dispatch, result) == 64, "Steam chain dispatch layout");

typedef int32_t (*rsf_steam_chain_service)(void*, rsf_d3d11_chain_dispatch*);

int32_t rsf_steam_dispatch_guard(rsf_steam_chain_service service, void* user,
    rsf_d3d11_chain_dispatch* request)
{
    __try {
        return service(user, request);
    } __except (GetExceptionCode() == RSF_CXX_EXCEPTION_CODE ? EXCEPTION_EXECUTE_HANDLER
                                                              : EXCEPTION_CONTINUE_SEARCH) {
        return RSF_ERROR_NOT_READY;
    }
}

void rsf_steam_log_guard(const rsf_game_host_services* host, const char* message)
{
    if (!host->log) return;
    __try {
        host->log(host->user, message);
    } __except (GetExceptionCode() == RSF_CXX_EXCEPTION_CODE ? EXCEPTION_EXECUTE_HANDLER
                                                              : EXCEPTION_CONTINUE_SEARCH) {
    }
}
