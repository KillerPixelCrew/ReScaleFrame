// SPDX-License-Identifier: MIT
#include <windows.h>
#include <rescaleframe/ac7_carrier.h>

extern BOOL WINAPI rsf_ac7_proxy_process_event(HINSTANCE instance, DWORD event, LPVOID reserved);
extern BOOL rsf_shim_attach(HINSTANCE instance, DWORD event,
                          const wchar_t* (*ac7_executable)(void),
                          BOOL (WINAPI *ac7_attach)(HINSTANCE, DWORD, LPVOID));

BOOL WINAPI DllMain(HINSTANCE instance, DWORD event, LPVOID reserved)
{
    (void)reserved;
    return rsf_shim_attach(instance, event, rsf_ac7_carrier_executable, rsf_ac7_proxy_process_event);
}
