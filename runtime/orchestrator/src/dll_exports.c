/* SPDX-License-Identifier: MIT */
#include <rescaleframe/d3d11_fg_api.h>
#include <rescaleframe/d3d11_sr_api.h>
#include <windows.h>
#include "rust_export.h"

/* The query is a Runtime DLL entry. Embedded carriers use the native callbacks. */
RSF_EXPORT(rsf_get_d3d11_fg_api)
RSF_EXPORT(rsf_get_d3d11_sr_api)

/* Rust (lib.rs). */
void rsf_core_install_panic_record(void);

/* The Runtime DLL has no other entry every host passes through first: a Rust panic in it, which
   aborts the game, leaves a record from load on. */
BOOL WINAPI DllMain(HINSTANCE instance, DWORD reason, LPVOID reserved)
{
    (void)instance;
    (void)reserved;
    if (reason == DLL_PROCESS_ATTACH) rsf_core_install_panic_record();
    return TRUE;
}
