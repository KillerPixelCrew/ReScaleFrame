// SPDX-License-Identifier: GPL-3.0-only
#pragma once
#include <rescaleframe/runtime.h>
#include <rescaleframe/game_api.h>
#ifdef __cplusplus
extern "C" {
#endif
/* Called outside DllMain. The runtime owns settings, backend contexts and GPU retirement.
   The returned services and their user owner stay alive until process termination. The caller
   selects/prepares the game plugin before any graphics context is created. */
RSF_RUNTIME_API int32_t rsf_rdr2_sr_host_create(const wchar_t* configuration_path,
    const wchar_t* runtime_directory, rsf_game_host_services* services);
#ifdef __cplusplus
}
#endif
