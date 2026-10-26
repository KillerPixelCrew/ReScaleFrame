// SPDX-License-Identifier: GPL-3.0-only
#ifndef WIN32_LEAN_AND_MEAN
#define WIN32_LEAN_AND_MEAN
#endif
#include <windows.h>
#include <dxgi1_6.h>
#include <cstdio>
#include <cstring>
#include <cwchar>
#include "../runtime/vulkan/fsr4-fg/bridge_api.h"

int wmain(int argc, wchar_t** argv) {
    rsf_fsr4fg_bridge* owner = reinterpret_cast<rsf_fsr4fg_bridge*>(1);
    rsf_fsr4fg_create setup{sizeof(setup) - 1, 99, 1, nullptr, 640, 360, 1, 0, 0, 1};
    if (RsfFsr4FgCreate(&setup, &owner) != RSF_FSR4FG_INVALID_ARGUMENT || owner) return 10;
    setup.struct_size = sizeof(setup);
    if (RsfFsr4FgCreate(&setup, &owner) != RSF_FSR4FG_ABI_MISMATCH || owner) return 11;
    setup.abi_version = 1; setup.runtime_directory = reinterpret_cast<const uint16_t*>(L"relative");
    if (RsfFsr4FgCreate(&setup, &owner) != RSF_FSR4FG_INVALID_ARGUMENT || owner) return 12;
    setup.runtime_directory = reinterpret_cast<const uint16_t*>(L"C:\\fixture"); setup.generated_count = 2;
    if (RsfFsr4FgCreate(&setup, &owner) != RSF_FSR4FG_INVALID_ARGUMENT || owner) return 13;
    rsf_fsr4fg_info info{sizeof(info), 1, 8, 9, 10, 11};
    if (RsfFsr4FgProbe(1, reinterpret_cast<const uint16_t*>(L"relative"), &info) != RSF_FSR4FG_INVALID_ARGUMENT ||
        info.adapter_luid || info.provider_version || info.vendor_id || info.max_generated) return 14;
    if (RsfFsr4FgStart(nullptr, reinterpret_cast<void*>(1)) != RSF_FSR4FG_INVALID_ARGUMENT) return 15;
    if (argc == 1) { std::puts("{\"contract_passed\":true,\"device_query\":false}"); return 0; }
    if (argc != 3 || std::wcscmp(argv[1], L"--device")) return 16;
    IDXGIFactory4* factory{};
    if (FAILED(CreateDXGIFactory2(0, IID_PPV_ARGS(&factory)))) return 20;
    uint32_t queried = 0;
    for (UINT index = 0; ; ++index) {
        IDXGIAdapter1* adapter{};
        if (factory->EnumAdapters1(index, &adapter) == DXGI_ERROR_NOT_FOUND) break;
        if (!adapter) { factory->Release(); return 21; }
        DXGI_ADAPTER_DESC1 desc{}; const HRESULT description = adapter->GetDesc1(&desc); adapter->Release();
        if (FAILED(description)) { factory->Release(); return 22; }
        if (desc.Flags & DXGI_ADAPTER_FLAG_SOFTWARE) continue;
        uint64_t luid = 0; std::memcpy(&luid, &desc.AdapterLuid, sizeof(luid));
        info = {sizeof(info), 1};
        const int result = RsfFsr4FgProbe(luid, reinterpret_cast<const uint16_t*>(argv[2]), &info);
        std::printf("{\"device_query\":true,\"vendor\":%u,\"device\":%u,\"adapter_luid\":%llu,\"probe\":%d,\"provider_version\":%llu,\"maximum_generated\":%u,\"hwnd_supplied\":false}\n",
            desc.VendorId, desc.DeviceId, static_cast<unsigned long long>(luid), result,
            static_cast<unsigned long long>(info.provider_version), info.max_generated);
        if (result == RSF_FSR4FG_NOT_SUPPORTED) {
            setup = {sizeof(setup), 1, luid, reinterpret_cast<const uint16_t*>(argv[2]), 640, 360, 1, 0, 0, 1};
            if (RsfFsr4FgCreate(&setup, &owner) != RSF_FSR4FG_NOT_SUPPORTED || owner || info.provider_version) {
                factory->Release(); return 23;
            }
        } else if (result || info.adapter_luid != luid || !info.provider_version || info.max_generated != 1) {
            factory->Release(); return 24;
        }
        ++queried;
    }
    factory->Release();
    if (!queried) return 25;
    std::puts("{\"admission_passed\":true,\"rendering_executed\":false,\"presentation_replaced\":false}");
    return 0;
}
