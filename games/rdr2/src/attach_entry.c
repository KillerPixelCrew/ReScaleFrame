/* SPDX-License-Identifier: MIT */
#include "runtime_setup.h"
#include <rescaleframe/sha256_file.h>

/* The CRT invokes DllMain. The Rust worker owns the thread and session. */
BOOL WINAPI DllMain(HINSTANCE instance, DWORD reason, LPVOID reserved)
{
    (void)reserved;
    if (reason == DLL_PROCESS_ATTACH) {
        rsf_rdr2_attach_start_runtime(instance, rsf_common_sha256_file);
    }
    return TRUE;
}
