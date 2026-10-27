// SPDX-License-Identifier: GPL-3.0-only
// A stand-in for an ASI plugin: it installs the census from DllMain, which is how a game's ASI
// loader would start it, so the test can check that install is safe under the loader lock.
#include <windows.h>

#include "rescaleframe/vendor_census.h"

namespace {
rsf_census_result g_install_result = -100;
}

BOOL APIENTRY DllMain(HMODULE, DWORD reason, LPVOID) {
    if (reason == DLL_PROCESS_ATTACH) {
        char dir[MAX_PATH];
        const DWORD n = GetEnvironmentVariableA("RSF_CENSUS_OUT", dir, MAX_PATH);
        rsf_census_options options{};
        options.struct_size = sizeof(options);
        options.abi_version = RSF_CENSUS_ABI_VERSION;
        options.output_dir_utf8 = (n > 0 && n < MAX_PATH) ? dir : nullptr;
        options.full_record_cap = 8;
        options.summary_period_ms = 50;
        g_install_result = rsf_census_install(&options);
    }
    return TRUE;
}

extern "C" {
__declspec(dllexport) int asi_install_result() { return g_install_result; }
__declspec(dllexport) int asi_flush() { return rsf_census_flush(); }
__declspec(dllexport) int asi_counters(rsf_census_counters* counters) { return rsf_census_get_counters(counters); }
__declspec(dllexport) int asi_shutdown() { return rsf_census_shutdown(); }
}
