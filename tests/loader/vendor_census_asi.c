/* SPDX-License-Identifier: MIT */
#include <windows.h>
#include <rescaleframe/vendor_census.h>

void rsf_test_census_attach(rsf_census_result (*install)(const rsf_census_options*));

/* Exercise installation while Windows holds the loader lock. */
BOOL APIENTRY DllMain(HMODULE module, DWORD reason, LPVOID reserved)
{
    (void)module;
    (void)reserved;
    if (reason == DLL_PROCESS_ATTACH) {
        rsf_test_census_attach(rsf_census_install);
    }
    return TRUE;
}
