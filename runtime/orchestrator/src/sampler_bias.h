// SPDX-License-Identifier: GPL-3.0-only
#pragma once
#include <rescaleframe/d3d11_host.h>
#include <d3d11.h>
#include <wrl/client.h>
#include <array>

class SamplerBias {
    struct Entry {
        Microsoft::WRL::ComPtr<ID3D11SamplerState> original, replacement;
        float bias{};
    };
    std::array<Entry, 32> entries_{};
    std::array<Microsoft::WRL::ComPtr<ID3D11SamplerState>, 16> owned_{};
    Microsoft::WRL::ComPtr<ID3D11Device> device_;
    ID3D11DeviceContext* context_{};
    DWORD thread_{};
    unsigned next_{};
public:
    bool apply(ID3D11DeviceContext*, rsf_d3d11_sampler_bias&);
};
