// SPDX-License-Identifier: MIT
#include <windows.h>

extern BOOL rsf_shim_attach(HINSTANCE instance, DWORD event);

BOOL WINAPI DllMain(HINSTANCE instance, DWORD event, LPVOID reserved)
{
    (void)reserved;
    return rsf_shim_attach(instance, event);
}
