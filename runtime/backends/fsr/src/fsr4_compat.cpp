// SPDX-License-Identifier: GPL-3.0-only
#include "fsr4_compat.h"
#include <MinHook.h>
#include <bcrypt.h>
#include <dxgi1_4.h>
#include <wrl/client.h>
#include <array>
#include <cstring>
#include <mutex>

using Microsoft::WRL::ComPtr;
namespace {
std::mutex ownership;
std::weak_ptr<Fsr4Compatibility> lease;
SRWLOCK registration = SRWLOCK_INIT;
ID3D12Device* allowed_device = nullptr;
const Fsr4Compatibility* registered_owner = nullptr;
HMODULE resident_module = nullptr;
using Capability = bool(*)(void*, ID3D12Device*);
Capability original = nullptr;
// AMD SDK2.3.0, upscaler4.1.1.2740. Capability predicate accepts gfx11 ASIC families for INT8;
// the separate FP8 predicate and every shader/device feature check remain native.
constexpr unsigned char expected[]{0x48,0x83,0xec,0x48,0x48,0x8b,0xc2,0x48,0x85,0xd2,0x74,0x54,0x48,0x8d,0x54,0x24};
constexpr unsigned char digest[]{0xd0,0xdc,0xcc,0xc7,0x4a,0x43,0xc4,0x4b,0xa4,0x35,0xb7,0xa3,0x69,0xb4,0x56,0xe0,
    0x97,0x0d,0x8a,0x44,0x64,0xe4,0xbd,0x68,0x31,0x19,0xb3,0x74,0xf2,0xc9,0xfb,0x46};
bool fingerprint(HMODULE module)
{
    wchar_t path[32768]{};
    const auto length = GetModuleFileNameW(module,path,32768);
    if (!length || length >= 32768) return false;
    HANDLE file = CreateFileW(path,GENERIC_READ,FILE_SHARE_READ,nullptr,OPEN_EXISTING,FILE_ATTRIBUTE_NORMAL,nullptr);
    if (file == INVALID_HANDLE_VALUE) return false;
    LARGE_INTEGER size{};
    bool ok = GetFileSizeEx(file,&size) && size.QuadPart == 28761864;
    BCRYPT_ALG_HANDLE algorithm = nullptr; BCRYPT_HASH_HANDLE hash = nullptr;
    ok = ok && BCryptOpenAlgorithmProvider(&algorithm,BCRYPT_SHA256_ALGORITHM,nullptr,0) >= 0;
    ok = ok && BCryptCreateHash(algorithm,&hash,nullptr,0,nullptr,0,0) >= 0;
    std::array<unsigned char,65536> bytes{}; DWORD read = 0;
    while (ok) {
        if (!ReadFile(file,bytes.data(),DWORD(bytes.size()),&read,nullptr)) { ok=false; break; }
        if (!read) break;
        ok = BCryptHashData(hash,bytes.data(),read,0) >= 0;
    }
    unsigned char actual[32]{};
    ok = ok && BCryptFinishHash(hash,actual,sizeof(actual),0) >= 0 && !std::memcmp(actual,digest,sizeof(digest));
    if (hash) BCryptDestroyHash(hash);
    if (algorithm) BCryptCloseAlgorithmProvider(algorithm,0);
    CloseHandle(file);
    return ok;
}
}
struct Fsr4Compatibility {
    HMODULE module = nullptr;
    ComPtr<ID3D12Device> device;
    ~Fsr4Compatibility();
};
namespace {
bool capability(void* api, ID3D12Device* device)
{
    AcquireSRWLockShared(&registration);
    const bool allowed = allowed_device && allowed_device == device;
    ReleaseSRWLockShared(&registration);
    return allowed ? true : original(api,device);
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
}
std::shared_ptr<Fsr4Compatibility> rsf_fsr4_enable_int8(HMODULE module, ID3D12Device* device,
    rsf_backend_log_fn log, void* user) try
{
    if (!module || !device) return {};
    ComPtr<IDXGIFactory4> factory; ComPtr<IDXGIAdapter1> adapter;
    DXGI_ADAPTER_DESC1 description{};
    if (FAILED(CreateDXGIFactory1(IID_PPV_ARGS(&factory))) ||
        FAILED(factory->EnumAdapterByLuid(device->GetAdapterLuid(),IID_PPV_ARGS(&adapter))) ||
        FAILED(adapter->GetDesc1(&description))) return {};
    // Supported Radeon devices retain AMD's own selection. Other AMD generations need their
    // own validation rather than assuming the NVIDIA/Intel compatibility result transfers.
    if (description.VendorId != 0x10de && description.VendorId != 0x8086) return {};
    D3D12_FEATURE_DATA_SHADER_MODEL shader_model{D3D_SHADER_MODEL_6_6};
    D3D12_FEATURE_DATA_D3D12_OPTIONS1 options{};
    if (FAILED(device->CheckFeatureSupport(D3D12_FEATURE_SHADER_MODEL,&shader_model,sizeof(shader_model))) ||
        shader_model.HighestShaderModel < D3D_SHADER_MODEL_6_6 ||
        FAILED(device->CheckFeatureSupport(D3D12_FEATURE_D3D12_OPTIONS1,&options,sizeof(options))) || !options.WaveOps) {
        if (log) log(user,"FSR4 INT8 compatibility refused: Shader Model6.6 and wave operations are required");
        return {};
    }
    std::lock_guard<std::mutex> lock(ownership);
    if (auto existing = lease.lock()) {
        if (existing->module == module && existing->device.Get() == device) return existing;
        if (log) log(user,"FSR4 INT8 compatibility refused: another runtime/device owns the capability hook");
        return {};
    }
    if (resident_module && resident_module != module) {
        if (log) log(user,"FSR4 INT8 compatibility refused: another SDK module owns the resident hook");
        return {};
    }
    if (!resident_module) {
        auto* target = reinterpret_cast<unsigned char*>(module)+0x8d70;
        if (!fingerprint(module) || std::memcmp(target,expected,sizeof(expected))) {
            if (log) log(user,"FSR4 INT8 compatibility refused: SDK binary fingerprint or capability bytes differ");
            return {};
        }
        const auto initialized = MH_Initialize();
        if (initialized != MH_OK && initialized != MH_ERROR_ALREADY_INITIALIZED) return {};
        if (MH_CreateHook(target,reinterpret_cast<void*>(&capability),reinterpret_cast<void**>(&original)) != MH_OK) return {};
        HMODULE implementation = nullptr, runtime = nullptr;
        // Both code owners must outlive any future caller already inside the SDK or trampoline.
        const DWORD pin = GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS|GET_MODULE_HANDLE_EX_FLAG_PIN;
        if (!GetModuleHandleExW(pin,reinterpret_cast<LPCWSTR>(&capability),&implementation) ||
            !GetModuleHandleExW(pin,reinterpret_cast<LPCWSTR>(target),&runtime) || MH_EnableHook(target) != MH_OK) {
            MH_RemoveHook(target);
            if (log) log(user,"FSR4 INT8 compatibility refused: resident hook preparation failed");
            return {};
        }
        resident_module = runtime;
    }
    auto result = std::make_shared<Fsr4Compatibility>();
    result->device = device; result->module = module;
    AcquireSRWLockExclusive(&registration);
    allowed_device = device; registered_owner = result.get();
    ReleaseSRWLockExclusive(&registration);
    lease = result;
    if (log) log(user,"FSR4 INT8 compatibility active: SDK4.1.1.2740, device-scoped predicate RVA0x8d70; real adapter identity preserved");
    return result;
}
catch (...) {
    if (log) log(user,"FSR4 INT8 compatibility preparation failed");
    return {};
}
