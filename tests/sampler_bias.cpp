// SPDX-License-Identifier: GPL-3.0-only
#include "../runtime/orchestrator/src/sampler_bias.h"
#include <cstdio>
#include <cmath>
#include <cstring>

using Microsoft::WRL::ComPtr;
int main() {
    ComPtr<ID3D11Device> device; ComPtr<ID3D11DeviceContext> context;
    if (FAILED(D3D11CreateDevice(nullptr, D3D_DRIVER_TYPE_WARP, nullptr, 0, nullptr, 0,
        D3D11_SDK_VERSION, &device, nullptr, &context))) return 77;
    int failures = 0;
    auto check = [&](bool value, const char* label) { if (!value) { ++failures; std::printf("FAIL %s\n", label); } };
    D3D11_SAMPLER_DESC desc{};
    desc.Filter = D3D11_FILTER_ANISOTROPIC;
    desc.AddressU = D3D11_TEXTURE_ADDRESS_WRAP; desc.AddressV = D3D11_TEXTURE_ADDRESS_CLAMP;
    desc.AddressW = D3D11_TEXTURE_ADDRESS_MIRROR; desc.MaxAnisotropy = 8;
    desc.ComparisonFunc = D3D11_COMPARISON_NEVER; desc.MaxLOD = 12; desc.MipLODBias = .25f;
    ComPtr<ID3D11SamplerState> native, comparison, foreign;
    check(SUCCEEDED(device->CreateSamplerState(&desc, &native)), "native sampler");
    desc.Filter = D3D11_FILTER_COMPARISON_MIN_MAG_LINEAR_MIP_POINT; desc.ComparisonFunc = D3D11_COMPARISON_LESS;
    check(SUCCEEDED(device->CreateSamplerState(&desc, &comparison)), "comparison sampler");
    desc.MipLODBias = 2;
    check(SUCCEEDED(device->CreateSamplerState(&desc, &foreign)), "foreign sampler");
    ID3D11SamplerState* initial[]{native.Get(), comparison.Get()}; context->PSSetSamplers(0, 2, initial);
    auto get = [&](UINT slot) { ComPtr<ID3D11SamplerState> value; context->PSGetSamplers(slot, 1, &value); return value; };
    SamplerBias cache;
    rsf_d3d11_sampler_bias request{sizeof(request), RSF_D3D11_HOST_ABI_VERSION, 1};
    request.samplers[0] = native.Get(); request.samplers[1] = comparison.Get(); request.slot_mask = 1; request.bias = -1.5f;
    check(cache.apply(context.Get(), request) && request.applied_mask == 1, "selected binding");
    auto changed = get(0); D3D11_SAMPLER_DESC actual{}; changed->GetDesc(&actual);
    D3D11_SAMPLER_DESC original{}; native->GetDesc(&original);
    check(std::abs(actual.MipLODBias + 1.25f) < 1e-6f, "additive bias");
    actual.MipLODBias = original.MipLODBias;
    check(std::memcmp(&actual, &original, sizeof(actual)) == 0, "all other fields preserved");
    check(get(1) == comparison, "unselected comparison untouched");
    check(cache.apply(context.Get(), request) && get(0) == changed, "immutable cached object reused");
    auto* foreign_pointer = foreign.Get(); context->PSSetSamplers(0, 1, &foreign_pointer);
    check(cache.apply(context.Get(), request) && request.applied_mask == 0 && get(0) == foreign, "foreign renderer binding preserved");
    context->PSSetSamplers(0, 1, initial);
    check(cache.apply(context.Get(), request), "reacquire native binding");
    for (unsigned i = 0; i < 80; ++i) {
        request.bias = -.01f * float(i + 1);
        check(cache.apply(context.Get(), request) && request.applied_mask == 1, "cache eviction while bound");
    }
    request.slot_mask = 0;
    check(cache.apply(context.Get(), request) && request.applied_mask == 0 && get(0) == native, "drain restores exact baseline after eviction");
    request.slot_mask = 0x10000;
    check(!cache.apply(context.Get(), request) && get(0) == native, "invalid mask leaves bindings unchanged");
    request.slot_mask = 0;
    ComPtr<ID3D11DeviceContext> deferred; device->CreateDeferredContext(0, &deferred);
    check(!cache.apply(deferred.Get(), request), "deferred context refused");
    std::printf("Sampler bias: %d failures\n", failures); return failures ? 1 : 0;
}
