/* SPDX-License-Identifier: MIT */
/* Contain C++ exceptions from the Runtime chain service before control returns to Rust
   (rescaleframe/foreign_guard.h: access violations and other faults still propagate). */
#include <rescaleframe/d3d11_host.h>
#include <stddef.h>

#include <rescaleframe/foreign_guard.h>

_Static_assert(sizeof(rsf_d3d11_chain_dispatch) == 72 && offsetof(rsf_d3d11_chain_dispatch, args) == 24 &&
    offsetof(rsf_d3d11_chain_dispatch, result) == 64, "Steam chain dispatch layout");

typedef int32_t (*rsf_steam_chain_service)(void*, rsf_d3d11_chain_dispatch*);

int32_t rsf_steam_dispatch_guard(rsf_steam_chain_service service, void* user,
    rsf_d3d11_chain_dispatch* request)
{
    int32_t result = RSF_ERROR_NOT_READY;
    RSF_FOREIGN_TRY { result = service(user, request); }
    RSF_FOREIGN_CATCH { result = RSF_ERROR_NOT_READY; }
    return result;
}

