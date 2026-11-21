// SPDX-License-Identifier: MIT
// Research-only late attachment. The Rust worker borrows the already loaded Runtime.
#include "runtime_worker_native.h"
#include <windows.h>

namespace {
DWORD WINAPI attach(void* module) noexcept
{
    return rsf_rdr2_attach_start_runtime(module, rsf_rdr2_fingerprint);
}
}

BOOL WINAPI DllMain(HINSTANCE instance, DWORD reason, void*)
{
    if (reason == DLL_PROCESS_ATTACH) {
        DisableThreadLibraryCalls(instance);
        if (auto thread = CreateThread(nullptr, 0, attach, instance, 0, nullptr)) CloseHandle(thread);
    }
    return TRUE;
}
