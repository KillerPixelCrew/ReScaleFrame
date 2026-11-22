// SPDX-License-Identifier: MIT
#include <windows.h>

BOOL WINAPI DllMain(HINSTANCE instance, DWORD event, LPVOID reserved)
{
    (void)instance;
    (void)event;
    (void)reserved;
    // Activation runs from DirectInput8Create, outside the loader lock.
    return TRUE;
}
