// SPDX-License-Identifier: MIT
#include <windows.h>

extern BOOL rsf_plugin_carrier_attach(HINSTANCE instance, DWORD event);

BOOL WINAPI DllMain(HINSTANCE instance, DWORD event, LPVOID reserved)
{
    (void)reserved;
    return rsf_plugin_carrier_attach(instance, event);
}
