// SPDX-License-Identifier: MIT
#include "fsr4_compat_core.h"
#include "fsr4_compat_platform.h"
#include <rescaleframe/sha256_file.h>
#include <MinHook.h>
#include <algorithm>
#include <cstring>
#include <new>
#include <utility>

extern "C" uint32_t rsf_fsr4_compat_adapter_vendor(void*);
extern "C" bool rsf_fsr4_compat_device_supports_int8(void*);
extern "C" void rsf_fsr4_compat_retain_device(void*);
extern "C" void rsf_fsr4_compat_release_device(void*);
extern "C" const rsf_fsr4_compat_lease* rsf_fsr4_compat_acquire_device(
    const rsf_fsr4_compat_platform*, void*, void*, Fsr4Log, void*);

namespace {
bool module_digest(void* module, const uint32_t* sizes, size_t size_count,
    uint32_t* size, uint8_t digest[32])
{
    constexpr DWORD capacity = 32768;
    std::unique_ptr<wchar_t[]> path(new (std::nothrow) wchar_t[capacity]());
    if (!path) return false;
    const DWORD length = GetModuleFileNameW(static_cast<HMODULE>(module), path.get(), capacity);
    if (!length || length >= capacity) return false;
    WIN32_FILE_ATTRIBUTE_DATA attributes{};
    if (!GetFileAttributesExW(path.get(), GetFileExInfoStandard, &attributes) || attributes.nFileSizeHigh)
        return false;
    const auto file_size = static_cast<uint32_t>(attributes.nFileSizeLow);
    if (std::find(sizes, sizes + size_count, file_size) == sizes + size_count)
        return false;
    unsigned char actual[32]{};
    if (!rsf::sha256_file(path.get(), actual, FILE_SHARE_READ)) return false;
    *size = file_size;
    std::memcpy(digest, actual, sizeof(actual));
    return true;
}

void* match_entry(void* module, size_t rva, const uint8_t* expected, size_t length)
{
    auto* target = static_cast<unsigned char*>(module) + rva;
    return std::memcmp(target, expected, length) == 0 ? target : nullptr;
}

rsf_fsr4_compat_capability_fn create_hook(void* target, rsf_fsr4_compat_capability_fn detour)
{
    const auto initialized = MH_Initialize();
    if (initialized != MH_OK && initialized != MH_ERROR_ALREADY_INITIALIZED) return nullptr;
    void* original = nullptr;
    if (MH_CreateHook(target, reinterpret_cast<void*>(detour), &original) != MH_OK) return nullptr;
    return reinterpret_cast<rsf_fsr4_compat_capability_fn>(original);
}

void* enable_hook(void* target, rsf_fsr4_compat_capability_fn detour)
{
    HMODULE implementation = nullptr, runtime = nullptr;
    // Both code owners must outlive any future caller inside the SDK or trampoline.
    const DWORD pin = GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_PIN;
    if (!GetModuleHandleExW(pin, reinterpret_cast<LPCWSTR>(detour), &implementation) ||
        !GetModuleHandleExW(pin, reinterpret_cast<LPCWSTR>(target), &runtime) ||
        MH_EnableHook(target) != MH_OK) {
        MH_RemoveHook(target);
        return nullptr;
    }
    return runtime;
}

const rsf_fsr4_compat_platform platform{
    sizeof(rsf_fsr4_compat_platform), RSF_FSR4_COMPAT_PLATFORM_ABI_VERSION,
    rsf_fsr4_compat_adapter_vendor, rsf_fsr4_compat_device_supports_int8,
    rsf_fsr4_compat_retain_device, rsf_fsr4_compat_release_device,
    module_digest, match_entry, create_hook, enable_hook,
};

using NativeLease = std::unique_ptr<const rsf_fsr4_compat_lease, decltype(&rsf_fsr4_compat_release)>;
}

struct Fsr4Compatibility {
    NativeLease lease;
    explicit Fsr4Compatibility(NativeLease value) : lease(std::move(value)) {}
};

const rsf_fsr4_compat_lease* rsf_fsr4_acquire_int8_adapter(HMODULE module,
    IDXGIAdapter1* adapter, ID3D12Device* device, Fsr4Log log, void* user)
{
    return rsf_fsr4_compat_acquire_logged(&platform, module, adapter, device, log, user);
}


std::shared_ptr<Fsr4Compatibility> rsf_fsr4_enable_int8_adapter(HMODULE module,
    IDXGIAdapter1* adapter, ID3D12Device* device, Fsr4Log log, void* user) try
{
    NativeLease lease(rsf_fsr4_acquire_int8_adapter(module, adapter, device, log, user),
        rsf_fsr4_compat_release);
    if (!lease) return {};
    return std::make_shared<Fsr4Compatibility>(std::move(lease));
}
catch (...) {
    if (log) log(user, "FSR4 INT8 compatibility preparation failed");
    return {};
}

const rsf_fsr4_compat_lease* rsf_fsr4_acquire_int8(HMODULE module, ID3D12Device* device,
    Fsr4Log log, void* user)
{
    return rsf_fsr4_compat_acquire_device(&platform, module, device, log, user);
}
