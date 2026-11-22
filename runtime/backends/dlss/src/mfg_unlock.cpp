// SPDX-License-Identifier: MIT
#include "mfg_unlock.h"
#include "streamline_load.h"
#include "streamline_diagnostics.h"
#include <rescaleframe/frame_generation.h>
#include "../../common/sr_helpers.h"
#if defined(_MSC_VER) && defined(RSF_HAVE_STREAMLINE) && RSF_HAVE_STREAMLINE
#include "../third_party/mfg_unlocker/midpoint_fix.h"
#include "../third_party/mfg_unlocker/ngx_mfg_gate.h"
#include "../third_party/mfg_unlocker/universal_wrapper_profile.h"
#include <tlhelp32.h>
#include <algorithm>
#include <cstring>
#include <cstdio>

namespace {
uint8_t* wrapper_limit(HMODULE module, uint32_t& maximum)
{
    const auto* base = reinterpret_cast<const uint8_t*>(module);
    const auto* dos = reinterpret_cast<const IMAGE_DOS_HEADER*>(base);
    if (dos->e_magic != IMAGE_DOS_SIGNATURE || dos->e_lfanew <= 0) return nullptr;
    const auto* nt = reinterpret_cast<const IMAGE_NT_HEADERS64*>(base + dos->e_lfanew);
    if (nt->Signature != IMAGE_NT_SIGNATURE || nt->OptionalHeader.Magic != IMAGE_NT_OPTIONAL_HDR64_MAGIC)
        return nullptr;
    const auto* section = IMAGE_FIRST_SECTION(nt);
    uint8_t* found = nullptr;
    for (unsigned i = 0; i < nt->FileHeader.NumberOfSections; ++i, ++section) {
        if (!(section->Characteristics & IMAGE_SCN_MEM_EXECUTE) ||
            section->VirtualAddress >= nt->OptionalHeader.SizeOfImage) continue;
        const size_t size = std::min<size_t>(nt->OptionalHeader.SizeOfImage - section->VirtualAddress,
            std::max(section->Misc.VirtualSize, section->SizeOfRawData));
        auto* begin = const_cast<uint8_t*>(base + section->VirtualAddress);
        for (size_t j = 0; j + universal_wrapper_profile::kPatternSize <= size; ++j) {
            if (!universal_wrapper_profile::Matches(begin + j, size - j)) continue;
            if (found) return nullptr;
            found = begin + j;
        }
    }
    if (found) std::memcpy(&maximum, found + universal_wrapper_profile::kMaximumOffset, sizeof(maximum));
    return found;
}

// NGX chooses and loads its implementation during capability initialization. Inspect the
// actual loaded export owner, including driver-delivered paths, rather than guessing a DLL.
void* provider_module()
{
    HANDLE snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPMODULE, GetCurrentProcessId());
    if (snapshot == INVALID_HANDLE_VALUE) return nullptr;
    MODULEENTRY32W entry{}; entry.dwSize = sizeof(entry);
    HMODULE found = nullptr;
    bool ambiguous = false;
    for (BOOL more = Module32FirstW(snapshot, &entry); more; more = Module32NextW(snapshot, &entry)) {
        HMODULE held = nullptr;
        if (!GetModuleHandleExW(GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS,
            reinterpret_cast<LPCWSTR>(entry.modBaseAddr), &held)) continue;
        if (dlssg_provider_policy::IsDlssgImplementationModule(held)) {
            if (found) { ambiguous = true; FreeLibrary(held); break; }
            found = held;
        } else FreeLibrary(held);
    }
    CloseHandle(snapshot);
    if (ambiguous && found) { FreeLibrary(found); return nullptr; }
    return found;
}
uint32_t supported(void* device)
{
    return midpoint_fix::ObserveD3D12Device(device);
}

void* load_wrapper(const wchar_t* path, uint32_t require_signature)
{
    if (require_signature && !rsf_dlss_verify_runtime_signature(path)) return nullptr;
    return LoadLibraryExW(path, nullptr, LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_DEFAULT_DIRS);
}

uint32_t prepare_wrapper(void* module)
{
    const auto wrapper = static_cast<HMODULE>(module);
    uint32_t capacity = 0;
    auto* limit = wrapper_limit(wrapper, capacity);
    if (limit && capacity > 1) {
        auto* address = limit + universal_wrapper_profile::kPatchOffset;
        constexpr uint8_t expected[]{0x0f, 0x42, 0xd1};
        constexpr uint8_t replacement[]{0x90, 0x90, 0x90};
        // Only adopt our existing publication. A foreign NOP is not success.
        if (std::memcmp(address, expected, sizeof(expected)) == 0) {
            DWORD previous = 0;
            if (protected_pointer::ProtectionMatches(reinterpret_cast<uintptr_t>(address), PAGE_EXECUTE_READ, sizeof(expected)) &&
                VirtualProtect(address, sizeof(expected), PAGE_EXECUTE_WRITECOPY, &previous)) {
                // This three-byte instruction is changed only before any live FG allocation.
                // Runtime transitions arrive here with the presentation owner drained.
                if (std::memcmp(address, expected, sizeof(expected)) == 0)
                    std::memcpy(address, replacement, sizeof(replacement));
                FlushInstructionCache(GetCurrentProcess(), address, sizeof(replacement));
                DWORD ignored = 0;
                if (VirtualProtect(address, sizeof(expected), previous, &ignored) &&
                    std::memcmp(address, replacement, sizeof(replacement)) == 0) {
                    return capacity;
                }
            }
        }
    }
    return 0;
}

uint32_t prepare_provider(void* module, uint32_t maximum, char* report)
{
    const auto provider = static_cast<HMODULE>(module);
    wchar_t path[32768]{};
    const bool identified = GetModuleFileNameW(provider, path, DWORD(std::size(path))) &&
        dlssg_provider_policy::IsSupportedProvider(provider, path);
    const bool temporal = identified && midpoint_fix::PatchProvider(provider, path);
    const auto gate = temporal ? ngx_mfg_gate::Patch(provider, true) : ngx_mfg_gate::Result{};
    std::snprintf(report, 512, "RTX 40 MFG unlock %s: wrapper_capacity=%u temporal=%u gate=%u failure=%u provider=%ls",
        gate.patched ? "ready" : "refused", maximum, unsigned(temporal),
        unsigned(gate.patched), midpoint_fix::FailureCode(), path);
    return gate.patched ? 1 : 0;
}

} // namespace

struct rsf_mfg_platform {
    uint32_t (*supported)(void*);
    void* (*load_wrapper)(const wchar_t*, uint32_t);
    uint32_t (*prepare_wrapper)(void*);
    // Transfers one module reference on success. The Rust compatibility owner releases it.
    void* (*provider_module)();
    uint32_t (*prepare_provider)(void*, uint32_t, char*);
};

static_assert(sizeof(rsf_mfg_platform) == 5 * sizeof(void*) && sizeof(wchar_t) == 2);
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
constexpr rsf_mfg_platform platform{supported, load_wrapper, prepare_wrapper, provider_module, prepare_provider};
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
