// SPDX-License-Identifier: GPL-3.0-only
#pragma once
#include <rescaleframe/backend.h>
#include <windows.h>

namespace rsf {
// Called before the first feature allocation, or after a drained provider reload.
// The wrapper lease prevents module replacement while its instructions are patched.
struct MfgUnlock {
    HMODULE wrapper = nullptr;
    uint32_t status = 0;
    uint32_t maximum = 1;
    bool wrapper_patched = false;
    void prime(void* native_device, const wchar_t* directory, bool require_signature,
        rsf_backend_log_fn log, void* user);
    void prepare(void* native_device, void* get_state, rsf_backend_log_fn log, void* user);
    void release();
};
}
