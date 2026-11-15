// SPDX-License-Identifier: GPL-3.0-only
#include "sampler_bias.h"
#include <cmath>

bool SamplerBias::apply(ID3D11DeviceContext* context, rsf_d3d11_sampler_bias& request) {
    request.applied_mask = 0;
    if (!context || context->GetType() != D3D11_DEVICE_CONTEXT_IMMEDIATE ||
        (request.slot_mask & 0xffff0000u) || !std::isfinite(request.bias) || std::abs(request.bias) > 16) return false;
    Microsoft::WRL::ComPtr<ID3D11Device> device; context->GetDevice(&device);
    if (context_ && (context_ != context || thread_ != GetCurrentThreadId() || device_.Get() != device.Get())) return false;
    for (auto* pointer : request.samplers) if (pointer) {
        Microsoft::WRL::ComPtr<ID3D11Device> owner;
        static_cast<ID3D11SamplerState*>(pointer)->GetDevice(&owner);
        if (owner.Get() != device.Get()) return false;
    }
    std::array<Microsoft::WRL::ComPtr<ID3D11SamplerState>, 16> current, selected;
    std::array<ID3D11SamplerState*, 16> raw{};
    context->PSGetSamplers(0, 16, raw.data());
    for (unsigned i = 0; i < 16; ++i) current[i].Attach(raw[i]);
    for (unsigned i = 0; i < 16; ++i) {
        auto* original = static_cast<ID3D11SamplerState*>(request.samplers[i]);
        selected[i] = current[i];
        // Respect samplers installed by another renderer component.
        if (current[i].Get() != original && (!owned_[i] || current[i] != owned_[i])) continue;
        selected[i] = original;
        if (!original || !(request.slot_mask & (1u << i)) || request.bias == 0) continue;
        Entry* entry = nullptr;
        for (auto& candidate : entries_) if (candidate.original.Get() == original && candidate.bias == request.bias) {
            entry = &candidate; break;
        }
        if (!entry) {
            D3D11_SAMPLER_DESC d{}; original->GetDesc(&d);
            const float bias = d.MipLODBias + request.bias;
            if (!std::isfinite(bias) || bias < D3D11_MIP_LOD_BIAS_MIN || bias > D3D11_MIP_LOD_BIAS_MAX) return false;
            Microsoft::WRL::ComPtr<ID3D11SamplerState> replacement;
            d.MipLODBias = bias;
            if (FAILED(device->CreateSamplerState(&d, &replacement))) return false;
            entry = &entries_[next_++ % entries_.size()];
            *entry = {original, replacement, request.bias};
        }
        selected[i] = entry->replacement;
        request.applied_mask |= 1u << i;
    }
    context_ = context; device_ = device; thread_ = GetCurrentThreadId();
    for (unsigned i = 0; i < 16; ++i) {
        auto* sampler = selected[i].Get();
        if (sampler != current[i].Get()) context->PSSetSamplers(i, 1, &sampler);
        owned_[i] = (request.applied_mask & (1u << i)) ? selected[i] : nullptr;
    }
    return SUCCEEDED(device->GetDeviceRemovedReason());
}
