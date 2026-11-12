// SPDX-License-Identifier: GPL-3.0-only
#include "fsr4_compat_core.h"
#include <rescaleframe/sha256_file.h>
#include <bcrypt.h>
#include <MinHook.h>
#include <cstdint>
#include <cstdio>
#include <cstring>
#include <new>

namespace {
/// @brief Exclusive SRWLOCK scope.
struct Exclusive {
    SRWLOCK& lock;
    explicit Exclusive(SRWLOCK& value) : lock(value) { AcquireSRWLockExclusive(&lock); }
    ~Exclusive() { ReleaseSRWLockExclusive(&lock); }
    Exclusive(const Exclusive&) = delete;
    Exclusive& operator=(const Exclusive&) = delete;
};

SRWLOCK ownership = SRWLOCK_INIT;
std::weak_ptr<Fsr4Compatibility> lease;
SRWLOCK registration = SRWLOCK_INIT;
ID3D12Device* allowed_device = nullptr;
const Fsr4Compatibility* registered_owner = nullptr;
HMODULE resident_module = nullptr;
using Capability = bool(*)(void*, ID3D12Device*);
Capability original = nullptr;
// The INT8 provider's capability predicate: the third virtual entry after its RTTI pointer.
// It accepts gfx11 (and from 4.1.2 also gfx12) ASIC families for INT8; the separate FP8
// predicate and every shader/device feature check remain native.
struct Build {
    const char* name;
    uintptr_t capability_rva;
    unsigned long long size;
    unsigned char expected[16];
    unsigned char digest[32];
};
constexpr Build builds[]{
    // AMD SDK 2.3.0 (60f4ea81), vtable entry .rdata+1af36b8.
    {"4.1.1.2740", 0x8d70, 28761864ull,
        {0x48,0x83,0xec,0x48,0x48,0x8b,0xc2,0x48,0x85,0xd2,0x74,0x54,0x48,0x8d,0x54,0x24},
        {0xd0,0xdc,0xcc,0xc7,0x4a,0x43,0xc4,0x4b,0xa4,0x35,0xb7,0xa3,0x69,0xb4,0x56,0xe0,
         0x97,0x0d,0x8a,0x44,0x64,0xe4,0xbd,0x68,0x31,0x19,0xb3,0x74,0xf2,0xc9,0xfb,0x46}},
    // AMD SDK 2.4.0 (7539ae8b), vtable entry .rdata+3635270. Adds a native Shader Model 6.6
    // query, which enable_int8_adapter already requires before leasing the device.
    {"4.1.2.4555", 0x9960, 57664272ull,
        {0x40,0x53,0x48,0x83,0xec,0x50,0x48,0x8b,0x05,0x93,0x76,0x63,0x03,0x48,0x33,0xc4},
        {0xaa,0xd4,0xfc,0xcc,0xcb,0x29,0x08,0x4a,0x65,0x5f,0x9a,0x4f,0x9f,0x61,0x3b,0xd2,
         0x2c,0x8c,0x03,0xfb,0x4a,0x0f,0x88,0x40,0x14,0x8c,0xc6,0x54,0x3b,0x4c,0xca,0xce}}};
const Build* resident_build = nullptr;

/// @brief Returns the pinned upscaler build matching on-disk size and SHA-256, or null.
const Build* Fingerprint(HMODULE module)
{
    constexpr DWORD capacity = 32768;
    std::unique_ptr<wchar_t[]> path(new (std::nothrow) wchar_t[capacity]());
    if (!path) return nullptr;
    const DWORD length = GetModuleFileNameW(module, path.get(), capacity);
    if (!length || length >= capacity) return nullptr;
    WIN32_FILE_ATTRIBUTE_DATA attributes{};
    if (!GetFileAttributesExW(path.get(), GetFileExInfoStandard, &attributes) || attributes.nFileSizeHigh) return nullptr;
    for (const auto& build : builds) {
        if (attributes.nFileSizeLow != build.size) continue;
        unsigned char actual[32]{};
        if (!rsf::sha256_file(path.get(), actual, FILE_SHARE_READ)) return nullptr;
        return std::memcmp(actual, build.digest, sizeof(build.digest)) ? nullptr : &build;
    }
    return nullptr;
}
}

/// @brief Device-scoped capability lease; holds a device reference so its address cannot be reused.
struct Fsr4Compatibility {
    HMODULE module = nullptr;
    ID3D12Device* device = nullptr;
    Fsr4Compatibility() = default;
    Fsr4Compatibility(const Fsr4Compatibility&) = delete;
    Fsr4Compatibility& operator=(const Fsr4Compatibility&) = delete;
    ~Fsr4Compatibility();
};

namespace {
/// @brief Detour: true only for the leased device, otherwise the native SDK answer.
bool capability(void* api, ID3D12Device* device)
{
    AcquireSRWLockShared(&registration);
    const bool allowed = allowed_device && allowed_device == device;
    ReleaseSRWLockShared(&registration);
    return allowed ? true : original(api, device);
}
}

Fsr4Compatibility::~Fsr4Compatibility()
{
    AcquireSRWLockExclusive(&registration);
    if (registered_owner == this) {
        allowed_device = nullptr;
        registered_owner = nullptr;
    }
    ReleaseSRWLockExclusive(&registration);
    // The private hook and its trampoline remain resident and forward unchanged without a
    // device lease. Retiring them while an unrelated SDK query is in flight would be unsafe.
    if (device) device->Release();
}

std::shared_ptr<Fsr4Compatibility> rsf_fsr4_enable_int8_adapter(HMODULE module,
    IDXGIAdapter1* adapter, ID3D12Device* device, Fsr4Log log, void* user) try
{
    if (!module || !adapter || !device) return {};
    DXGI_ADAPTER_DESC1 description{};
    if (FAILED(adapter->GetDesc1(&description))) return {};
    // Supported Radeon devices retain AMD's own selection. Other AMD generations need their
    // own validation rather than assuming the NVIDIA/Intel compatibility result transfers.
    if (description.VendorId != 0x10de && description.VendorId != 0x8086) return {};
    D3D12_FEATURE_DATA_SHADER_MODEL shader_model{D3D_SHADER_MODEL_6_6};
    D3D12_FEATURE_DATA_D3D12_OPTIONS1 options{};
    if (FAILED(device->CheckFeatureSupport(D3D12_FEATURE_SHADER_MODEL, &shader_model, sizeof(shader_model))) ||
        shader_model.HighestShaderModel < D3D_SHADER_MODEL_6_6 ||
        FAILED(device->CheckFeatureSupport(D3D12_FEATURE_D3D12_OPTIONS1, &options, sizeof(options))) ||
        !options.WaveOps) {
        if (log) log(user, "FSR4 INT8 compatibility refused: Shader Model 6.6 and wave operations are required");
        return {};
    }
    Exclusive lock(ownership);
    if (auto existing = lease.lock()) {
        if (existing->module == module && existing->device == device) return existing;
        if (log) log(user, "FSR4 INT8 compatibility refused: another runtime/device owns the capability hook");
        return {};
    }
    if (resident_module && resident_module != module) {
        if (log) log(user, "FSR4 INT8 compatibility refused: another SDK module owns the resident hook");
        return {};
    }
    if (!resident_module) {
        const auto* build = Fingerprint(module);
        auto* target = build ? reinterpret_cast<unsigned char*>(module) + build->capability_rva : nullptr;
        if (!build || std::memcmp(target, build->expected, sizeof(build->expected))) {
            if (log) log(user, "FSR4 INT8 compatibility refused: SDK binary fingerprint or capability bytes differ");
            return {};
        }
        const auto initialized = MH_Initialize();
        if (initialized != MH_OK && initialized != MH_ERROR_ALREADY_INITIALIZED) return {};
        if (MH_CreateHook(target, reinterpret_cast<void*>(&capability),
                reinterpret_cast<void**>(&original)) != MH_OK) return {};
        HMODULE implementation = nullptr, runtime = nullptr;
        // Both code owners must outlive any future caller already inside the SDK or trampoline.
        const DWORD pin = GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_PIN;
        if (!GetModuleHandleExW(pin, reinterpret_cast<LPCWSTR>(&capability), &implementation) ||
            !GetModuleHandleExW(pin, reinterpret_cast<LPCWSTR>(target), &runtime) ||
            MH_EnableHook(target) != MH_OK) {
            MH_RemoveHook(target);
            if (log) log(user, "FSR4 INT8 compatibility refused: resident hook preparation failed");
            return {};
        }
        resident_module = runtime;
        resident_build = build;
    }
    auto result = std::make_shared<Fsr4Compatibility>();
    device->AddRef();
    result->device = device;
    result->module = module;
    AcquireSRWLockExclusive(&registration);
    allowed_device = device;
    registered_owner = result.get();
    ReleaseSRWLockExclusive(&registration);
    lease = result;
    if (log) {
        char message[160]{};
        std::snprintf(message, sizeof(message), "FSR4 INT8 compatibility active: SDK %s, device-scoped predicate RVA 0x%llx; real adapter identity preserved",
            resident_build->name, static_cast<unsigned long long>(resident_build->capability_rva));
        log(user, message);
    }
    return result;
}
catch (...) {
    if (log) log(user, "FSR4 INT8 compatibility preparation failed");
    return {};
}
