// SPDX-License-Identifier: GPL-3.0-only
/**
 * @file
 * Check D3D12 resource leases with WARP and independently signalled fences.
 * A one-slot lease must retain a resource after the caller drops its reference and
 * wait for both submission and vendor completion. Stale IDs, cancellation and busy
 * destruction are covered; CPU fence signals isolate lifetime rules from GPU work.
 */
#include <rescaleframe/fg_leases.h>
#include <windows.h>
#include <d3d12.h>
#include <dxgi1_4.h>
#include <wrl/client.h>
#include <cstdio>
#include <cstdlib>
using Microsoft::WRL::ComPtr;
void check(bool ok, int line) { if (!ok) { std::fprintf(stderr, "Lease check failed at %d\n", line); std::exit(1); } }
#define CHECK(x) check((x), __LINE__)
int main()
{
    ComPtr<IDXGIFactory4> factory; ComPtr<IDXGIAdapter> warp; ComPtr<ID3D12Device> device;
    if (FAILED(CreateDXGIFactory2(0, IID_PPV_ARGS(&factory))) ||
        FAILED(factory->EnumWarpAdapter(IID_PPV_ARGS(&warp))) ||
        FAILED(D3D12CreateDevice(warp.Get(), D3D_FEATURE_LEVEL_11_0, IID_PPV_ARGS(&device)))) return 77;
    ComPtr<ID3D12Resource> resource;
    D3D12_HEAP_PROPERTIES heap{}; heap.Type = D3D12_HEAP_TYPE_DEFAULT;
    D3D12_RESOURCE_DESC desc{}; desc.Dimension = D3D12_RESOURCE_DIMENSION_TEXTURE2D;
    desc.Width = 16; desc.Height = 16; desc.DepthOrArraySize = 1; desc.MipLevels = 1;
    desc.Format = DXGI_FORMAT_R8G8B8A8_UNORM; desc.SampleDesc.Count = 1;
    CHECK(SUCCEEDED(device->CreateCommittedResource(&heap, D3D12_HEAP_FLAG_NONE, &desc,
        D3D12_RESOURCE_STATE_COMMON, nullptr, IID_PPV_ARGS(&resource))));
    ComPtr<ID3D12Fence> gpu, vendor;
    CHECK(SUCCEEDED(device->CreateFence(0, D3D12_FENCE_FLAG_NONE, IID_PPV_ARGS(&gpu))));
    CHECK(SUCCEEDED(device->CreateFence(0, D3D12_FENCE_FLAG_NONE, IID_PPV_ARGS(&vendor))));
    rsf_fg_leases* leases = nullptr;
    CHECK(rsf_fg_leases_create(1, &leases) == 0);
    void* resources[] = {resource.Get()};
    CHECK(rsf_fg_leases_acquire(leases, 1, 1, resources, 1) == 0);
    auto* retained = resource.Get(); resource.Reset();
    CHECK(retained->GetDesc().Width == 16);
    CHECK(rsf_fg_leases_acquire(leases, 2, 1, resources, 1) == RSF_BACKEND_ERROR_NOT_READY);
    CHECK(rsf_fg_leases_destroy(leases) == RSF_BACKEND_ERROR_NOT_READY);
    CHECK(rsf_fg_leases_submit(leases, 1, gpu.Get(), 1) == 0);
    CHECK(rsf_fg_leases_cancel(leases, 1) == RSF_BACKEND_ERROR_NOT_READY);
    uint32_t pending = 0;
    CHECK(rsf_fg_leases_collect(leases, &pending) == 0 && pending == 1);
    rsf_fg_retirement retirement{sizeof(retirement), vendor.Get(), 2};
    CHECK(rsf_fg_leases_seal(leases, 1, &retirement) == 0);
    // CPU signaling is deliberate: this tests independent completion gates, not GPU submission.
    CHECK(SUCCEEDED(gpu->Signal(1)));
    CHECK(rsf_fg_leases_collect(leases, &pending) == 0 && pending == 1);
    CHECK(retained->GetDesc().Width == 16);
    CHECK(rsf_fg_leases_destroy(leases) == RSF_BACKEND_ERROR_NOT_READY);
    CHECK(SUCCEEDED(vendor->Signal(2)));
    CHECK(rsf_fg_leases_collect(leases, &pending) == 0 && pending == 0);
    CHECK(rsf_fg_leases_acquire(leases, 1, 1, resources, 1) == RSF_BACKEND_ERROR_STALE_RESOURCES);
    CHECK(SUCCEEDED(device->CreateCommittedResource(&heap, D3D12_HEAP_FLAG_NONE, &desc,
        D3D12_RESOURCE_STATE_COMMON, nullptr, IID_PPV_ARGS(&resource))));
    resources[0] = resource.Get();
    CHECK(rsf_fg_leases_acquire(leases, 2, 2, resources, 1) == 0);
    CHECK(rsf_fg_leases_cancel(leases, 2) == 0);
    CHECK(rsf_fg_leases_destroy(leases) == 0);
    std::puts("WARP COM retention and independent synthetic fence completion gates passed");
}
