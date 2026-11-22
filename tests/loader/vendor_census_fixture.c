/* SPDX-License-Identifier: MIT */
#include <windows.h>

#ifdef FIXTURE_CREATION
void rsf_test_census_creation_attach(HMODULE module);
#endif

BOOL WINAPI DllMain(HMODULE module, DWORD reason, LPVOID reserved)
{
    (void)module;
    (void)reason;
    (void)reserved;
#ifdef FIXTURE_CREATION
    if (reason == DLL_PROCESS_ATTACH) {
        rsf_test_census_creation_attach(module);
    }
#endif
    return TRUE;
}
