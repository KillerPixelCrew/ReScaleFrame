/* SPDX-License-Identifier: MIT */
#ifndef RSF_DLSS_MFG_UNLOCK_H
#define RSF_DLSS_MFG_UNLOCK_H
#include <rescaleframe/backend.h>
#include <stdbool.h>
#include <stdint.h>
#include <wchar.h>

#ifdef __cplusplus
extern "C" {
#endif

/* RTX 40 MFG compatibility owner, implemented in mfg_state.rs. Prime runs before
 * the first feature allocation, or after a drained provider reload. The owner's
 * wrapper reference prevents module replacement while its code is patched. */
typedef struct rsf_mfg_state rsf_mfg_state;
/* Streamline's embedded-signature check. Null when no signature is required. */
typedef bool (*rsf_mfg_verify_signature)(const wchar_t* path);

rsf_mfg_state* rsf_core_mfg_create(void);
void rsf_core_mfg_prime(const rsf_mfg_state* owner, void* native_device, const wchar_t* directory,
    rsf_mfg_verify_signature verify_signature, rsf_backend_log_fn log, void* user);
void rsf_core_mfg_prepare(const rsf_mfg_state* owner, void* native_device, void* get_state,
    rsf_backend_log_fn log, void* user);
void rsf_core_mfg_status(const rsf_mfg_state* owner, uint32_t* status, uint32_t* maximum);
void rsf_core_mfg_release(const rsf_mfg_state* owner);
void rsf_core_mfg_destroy(rsf_mfg_state* owner);

#ifdef __cplusplus
}

/* Lease adapter kept only for the D3D12 Streamline host in dlss_generation.cpp.
 * Remove it once that caller owns an rsf_mfg_state directly. */
bool rsf_dlss_verify_runtime_signature(const wchar_t* path);

namespace rsf {
class MfgUnlock {
public:
    MfgUnlock() : owner_(rsf_core_mfg_create()) {}
    ~MfgUnlock() { rsf_core_mfg_destroy(owner_); }
    MfgUnlock(const MfgUnlock&) = delete;
    MfgUnlock& operator=(const MfgUnlock&) = delete;
    void prime(void* device, const wchar_t* directory, bool require_signature, rsf_backend_log_fn log,
        void* user)
    {
        if (owner_)
            rsf_core_mfg_prime(owner_, device, directory,
                require_signature ? &rsf_dlss_verify_runtime_signature : nullptr, log, user);
    }
    void prepare(void* device, void* get_state, rsf_backend_log_fn log, void* user)
    {
        if (owner_) rsf_core_mfg_prepare(owner_, device, get_state, log, user);
    }
    void read_status(uint32_t& status, uint32_t& maximum) const { rsf_core_mfg_status(owner_, &status, &maximum); }
    void release() { rsf_core_mfg_release(owner_); }
private:
    rsf_mfg_state* owner_;
};
}
#endif
#endif
