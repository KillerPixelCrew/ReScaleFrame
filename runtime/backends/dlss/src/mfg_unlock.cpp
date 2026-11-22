// SPDX-License-Identifier: MIT
#include "mfg_unlock.h"
#include "streamline_load.h"
#include "streamline_diagnostics.h"
#include <rescaleframe/frame_generation.h>
#include "../../common/sr_helpers.h"
#if defined(_MSC_VER) && defined(RSF_HAVE_STREAMLINE) && RSF_HAVE_STREAMLINE
#include "../third_party/mfg_unlocker/midpoint_fix.h"
#include "../third_party/mfg_unlocker/ngx_mfg_gate.h"
#include <cstring>
#include <cstdio>
#include <iterator>

namespace {
uint32_t supported(void* device)
{
    return midpoint_fix::ObserveD3D12Device(device);
}

uint32_t verify_signature(const wchar_t* path)
{
    return rsf_dlss_verify_runtime_signature(path);
}

uint32_t publish_wrapper(void* address)
{
    constexpr uint8_t expected[]{0x0f, 0x42, 0xd1};
    constexpr uint8_t replacement[]{0x90, 0x90, 0x90};
    // A foreign NOP is not success. Runtime transitions arrive here with
    // the presentation owner drained and no live FG allocation.
    DWORD previous = 0;
    if (!protected_pointer::ProtectionMatches(reinterpret_cast<uintptr_t>(address), PAGE_EXECUTE_READ, sizeof(expected)) ||
        std::memcmp(address, expected, sizeof(expected)) != 0 ||
        !VirtualProtect(address, sizeof(expected), PAGE_EXECUTE_WRITECOPY, &previous)) return 0;
    if (std::memcmp(address, expected, sizeof(expected)) == 0)
        std::memcpy(address, replacement, sizeof(replacement));
    FlushInstructionCache(GetCurrentProcess(), address, sizeof(replacement));
    DWORD ignored = 0;
    return VirtualProtect(address, sizeof(expected), previous, &ignored) &&
        std::memcmp(address, replacement, sizeof(replacement)) == 0;
}

uint32_t prepare_provider(void* module, uint32_t maximum, char* report)
{
    const auto provider = static_cast<HMODULE>(module);
    wchar_t path[32768]{};
    const auto length = GetModuleFileNameW(provider, path, DWORD(std::size(path)));
    const bool identified = length != 0 && length < std::size(path) &&
        dlssg_provider_policy::IsSupportedProvider(provider, path);
    const bool temporal = identified && midpoint_fix::PatchProvider(provider, path);
    const auto gate = temporal ? ngx_mfg_gate::Patch(provider, true) : ngx_mfg_gate::Result{};
    if (length >= std::size(path)) path[0] = L'\0';
    std::snprintf(report, 512, "RTX 40 MFG unlock %s: wrapper_capacity=%u temporal=%u gate=%u failure=%u provider=%ls",
        gate.patched ? "ready" : "refused", maximum, unsigned(temporal),
        unsigned(gate.patched), midpoint_fix::FailureCode(), path);
    return gate.patched ? 1 : 0;
}

} // namespace

struct rsf_mfg_platform {
    uint32_t (*supported)(void*);
    uint32_t (*verify_signature)(const wchar_t*);
    uint32_t (*publish_wrapper)(void*);
    uint32_t (*prepare_provider)(void*, uint32_t, char*);
};

static_assert(sizeof(rsf_mfg_platform) == 4 * sizeof(void*) && sizeof(wchar_t) == 2);
static_assert(RSF_MFG_UNLOCK_NOT_APPLICABLE == 0 && RSF_MFG_UNLOCK_READY == 1 && RSF_MFG_UNLOCK_REFUSED == 2);

extern "C" {
rsf_mfg_state* rsf_core_mfg_create();
uint32_t rsf_core_mfg_prime(const rsf_mfg_state*, const rsf_mfg_platform*, void*, const wchar_t*, uint32_t);
uint32_t rsf_core_mfg_prepare(const rsf_mfg_state*, const rsf_mfg_platform*, void*, void*, char*);
void rsf_core_mfg_status(const rsf_mfg_state*, uint32_t*, uint32_t*);
void rsf_core_mfg_release(const rsf_mfg_state*);
void rsf_core_mfg_destroy(rsf_mfg_state*);
}

namespace {
constexpr rsf_mfg_platform platform{supported, verify_signature, publish_wrapper, prepare_provider};
}
#endif

rsf::MfgUnlock::MfgUnlock()
{
#if defined(_MSC_VER) && defined(RSF_HAVE_STREAMLINE) && RSF_HAVE_STREAMLINE
    owner_ = rsf_core_mfg_create();
#endif
}

rsf::MfgUnlock::~MfgUnlock()
{
#if defined(_MSC_VER) && defined(RSF_HAVE_STREAMLINE) && RSF_HAVE_STREAMLINE
    rsf_core_mfg_destroy(owner_);
#endif
}

void rsf::MfgUnlock::prime(void* device, const wchar_t* directory, bool require_signature,
    rsf_backend_log_fn log, void* user)
{
#if defined(_MSC_VER) && defined(RSF_HAVE_STREAMLINE) && RSF_HAVE_STREAMLINE
    if (!owner_) return;
    const auto maximum = rsf_core_mfg_prime(owner_, &platform, device, directory, uint32_t(require_signature));
    if (maximum) {
        rsf_logf(log, user, "RTX 40 MFG wrapper prepared=%u capacity=%u before plugin initialization",
            unsigned(maximum > 1), maximum);
    }
#else
    (void)device; (void)directory; (void)require_signature; (void)log; (void)user;
#endif
}

void rsf::MfgUnlock::prepare(void* device, void* get_state, rsf_backend_log_fn log, void* user)
{
#if defined(_MSC_VER) && defined(RSF_HAVE_STREAMLINE) && RSF_HAVE_STREAMLINE
    if (!owner_) return;
    char report[512]{};
    if (rsf_core_mfg_prepare(owner_, &platform, device, get_state, report))
        emit_streamline_log(log, user, report);
#else
    (void)device; (void)get_state; (void)log; (void)user;
#endif
}

void rsf::MfgUnlock::read_status(uint32_t& status, uint32_t& maximum) const
{
#if defined(_MSC_VER) && defined(RSF_HAVE_STREAMLINE) && RSF_HAVE_STREAMLINE
    rsf_core_mfg_status(owner_, &status, &maximum);
#else
    status = RSF_MFG_UNLOCK_NOT_APPLICABLE;
    maximum = 1;
#endif
}

void rsf::MfgUnlock::release()
{
#if defined(_MSC_VER) && defined(RSF_HAVE_STREAMLINE) && RSF_HAVE_STREAMLINE
    rsf_core_mfg_release(owner_);
#endif
}
