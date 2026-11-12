// SPDX-License-Identifier: GPL-3.0-only
#include "native_hooks.h"
#include <windows.h>
#include <MinHook.h>
#include <cstring>

namespace rsf::anomaly {
namespace {
bool mapped(const void* address, size_t size, const void* owner, bool executable) noexcept {
    if (!address || !size) return false;
    MEMORY_BASIC_INFORMATION info{};
    if (!VirtualQuery(address, &info, sizeof(info)) || info.State != MEM_COMMIT ||
        info.AllocationBase != owner || (info.Protect & (PAGE_GUARD | PAGE_NOACCESS))) return false;
    const auto start = reinterpret_cast<uintptr_t>(address);
    const auto region = reinterpret_cast<uintptr_t>(info.BaseAddress);
    if (start < region || start - region > info.RegionSize || size > info.RegionSize - (start - region))
        return false;
    const auto protection = info.Protect & 0xff;
    if (executable) return protection == PAGE_EXECUTE_READ ||
        protection == PAGE_EXECUTE_READWRITE || protection == PAGE_EXECUTE_WRITECOPY;
    return protection == PAGE_READONLY || protection == PAGE_READWRITE || protection == PAGE_WRITECOPY ||
        protection == PAGE_EXECUTE_READ || protection == PAGE_EXECUTE_READWRITE ||
        protection == PAGE_EXECUTE_WRITECOPY;
}
}
bool HookSet::install(std::byte* module, uint32_t image_size, std::span<const HookSite> sites) noexcept {
    if (count_ || sites.empty() || sites.size() > targets_.size() || image_size < 4096 ||
        !mapped(module, sizeof(IMAGE_DOS_HEADER), module, false)) return false;
    const auto* dos = reinterpret_cast<const IMAGE_DOS_HEADER*>(module);
    if (dos->e_magic != IMAGE_DOS_SIGNATURE || dos->e_lfanew < 0 ||
        static_cast<uint32_t>(dos->e_lfanew) > image_size - sizeof(IMAGE_NT_HEADERS64)) return false;
    const auto* pe = reinterpret_cast<const IMAGE_NT_HEADERS64*>(module + dos->e_lfanew);
    if (!mapped(pe, sizeof(*pe), module, false) || pe->Signature != IMAGE_NT_SIGNATURE ||
        pe->FileHeader.Machine != IMAGE_FILE_MACHINE_AMD64 ||
        pe->OptionalHeader.Magic != IMAGE_NT_OPTIONAL_HDR64_MAGIC ||
        pe->OptionalHeader.SizeOfImage != image_size) return false;
    for (size_t i = 0; i < sites.size(); ++i) {
        const auto& site = sites[i];
        if (!site.replacement || !site.original || site.rva > image_size - site.expected.size()) return false;
        const auto* address = module + site.rva;
        if (!mapped(address, site.expected.size(), module, true) ||
            std::memcmp(address, site.expected.data(), site.expected.size())) return false;
        for (size_t j = 0; j < i; ++j) if (sites[j].rva == site.rva) return false;
    }
    const auto initialized = MH_Initialize();
    if (initialized != MH_OK && initialized != MH_ERROR_ALREADY_INITIALIZED) return false;
    for (const auto& site : sites) {
        auto* address = module + site.rva;
        if (MH_CreateHook(address, site.replacement, site.original) != MH_OK) {
            for (size_t i = 0; i < count_; ++i) {
                (void)MH_RemoveHook(targets_[i]);
                *sites[i].original = nullptr;
            }
            count_ = 0;
            return false;
        }
        targets_[count_++] = address;
    }
    // After the first enable attempt, preserve trampolines even on an error:
    // another thread may already have entered one. The owner pins this DLL.
    if (!enable()) { (void)disable(); return false; }
    return true;
}
bool HookSet::enable() noexcept {
    if (!count_) return false;
    if (enabled_) return true;
    for (size_t i = 0; i < count_; ++i)
        if (MH_QueueEnableHook(targets_[i]) != MH_OK) return false;
    if (MH_ApplyQueued() != MH_OK) return false;
    enabled_ = true;
    return true;
}
bool HookSet::disable() noexcept {
    if (!count_) return true;
    for (size_t i = 0; i < count_; ++i)
        if (MH_QueueDisableHook(targets_[i]) != MH_OK) return false;
    if (MH_ApplyQueued() != MH_OK) return false;
    enabled_ = false;
    return true;
}
}
