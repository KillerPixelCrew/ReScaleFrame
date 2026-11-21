// SPDX-License-Identifier: MIT
#pragma once
#include <rescaleframe/backend.h>
#include <windows.h>

struct rsf_mfg_state;

namespace rsf {
// Called before the first feature allocation, or after a drained provider reload.
// The wrapper lease prevents module replacement while its instructions are patched.
class MfgUnlock {
public:
    MfgUnlock();
    ~MfgUnlock();
    MfgUnlock(const MfgUnlock&) = delete;
    MfgUnlock& operator=(const MfgUnlock&) = delete;
    void prime(void* native_device, const wchar_t* directory, bool require_signature,
        rsf_backend_log_fn log, void* user);
    void prepare(void* native_device, void* get_state, rsf_backend_log_fn log, void* user);
    void read_status(uint32_t& status, uint32_t& maximum) const;
    void release();
private:
    [[maybe_unused]] rsf_mfg_state* owner_{};
};
}
