/* SPDX-License-Identifier: MIT */
#include <windows.h>

void rsf_wingman_carrier_attach(HMODULE module);

BOOL WINAPI DllMain(HINSTANCE instance, DWORD reason, LPVOID reserved)
{
    (void)reserved;
    if (reason == DLL_PROCESS_ATTACH) rsf_wingman_carrier_attach(instance);
    return TRUE;
}
