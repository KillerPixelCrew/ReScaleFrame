// SPDX-License-Identifier: GPL-3.0-only
#include "mfg_unlock.h"
#include "streamline_load.h"
#include <rescaleframe/frame_generation.h>
#include "../../common/sr_helpers.h"
#if defined(_MSC_VER)
#include "../third_party/mfg_unlocker/midpoint_fix.h"
#include "../third_party/mfg_unlocker/ngx_mfg_gate.h"
#include "../third_party/mfg_unlocker/universal_wrapper_profile.h"
#include <tlhelp32.h>
#include <algorithm>
#include <cstring>
#include <mutex>

namespace {
std::mutex unlock_guard;
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
HMODULE provider_module()
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
}
#endif

void rsf::MfgUnlock::prime(void* device, const wchar_t* directory, bool require_signature,
    rsf_backend_log_fn log, void* user)
{
#if defined(_MSC_VER)
    std::lock_guard lock(unlock_guard);
    if (!device || !midpoint_fix::ObserveD3D12Device(device)) {
        status = RSF_MFG_UNLOCK_NOT_APPLICABLE;
        return;
    }
    if (wrapper) return;
    status = RSF_MFG_UNLOCK_REFUSED;
    wchar_t path[1200]{};
    swprintf_s(path, L"%s\\sl.dlss_g.dll", directory);
    if (require_signature && !rsf_dlss_verify_runtime_signature(path)) return;
    wrapper = LoadLibraryExW(path, nullptr, LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_DEFAULT_DIRS);
    if (!wrapper) return;
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
                    maximum = capacity; wrapper_patched = true;
                }
            }
        }
    }
    rsf::say(log, user, "RTX 40 MFG wrapper prepared=%u capacity=%u before plugin initialization",
        unsigned(wrapper_patched), maximum);
#else
    (void)device; (void)directory; (void)require_signature; (void)log; (void)user;
#endif
}

void rsf::MfgUnlock::prepare(void* device, void* get_state, rsf_backend_log_fn log, void* user)
{
#if defined(_MSC_VER)
    std::lock_guard lock(unlock_guard);
    if (!wrapper_patched || !device || !get_state) return;
    HMODULE owner = nullptr;
    if (!GetModuleHandleExW(GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS,
        reinterpret_cast<LPCWSTR>(get_state), &owner)) return;
    const bool same_wrapper = owner == wrapper;
    FreeLibrary(owner);
    HMODULE provider = provider_module();
    wchar_t path[32768]{};
    const bool identified = provider && GetModuleFileNameW(provider, path, DWORD(std::size(path))) &&
        dlssg_provider_policy::IsSupportedProvider(provider, path);
    const bool temporal = same_wrapper && identified && midpoint_fix::PatchProvider(provider, path);
    const auto gate = temporal ? ngx_mfg_gate::Patch(provider, true) : ngx_mfg_gate::Result{};
    status = gate.patched ? RSF_MFG_UNLOCK_READY : RSF_MFG_UNLOCK_REFUSED;
    if (provider) FreeLibrary(provider);
    rsf::say(log, user, "RTX 40 MFG unlock %s: wrapper_capacity=%u temporal=%u gate=%u failure=%u provider=%ls",
        status == RSF_MFG_UNLOCK_READY ? "ready" : "refused", maximum, unsigned(temporal),
        unsigned(gate.patched), midpoint_fix::FailureCode(), path);
#else
    (void)device; (void)get_state; (void)log; (void)user;
    status = RSF_MFG_UNLOCK_NOT_APPLICABLE;
#endif
}

void rsf::MfgUnlock::release()
{
    if (wrapper) FreeLibrary(wrapper);
    wrapper = nullptr;
    status = RSF_MFG_UNLOCK_NOT_APPLICABLE;
    maximum = 1;
    wrapper_patched = false;
}
