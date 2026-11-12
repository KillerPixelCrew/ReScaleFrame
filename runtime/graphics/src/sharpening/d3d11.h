// SPDX-License-Identifier: GPL-3.0-only
#pragma once
#include "options.h"
#include <d3d11.h>
#include <wrl/client.h>
namespace rsf::sharpening {
// Internal device helper. Call on the owning immediate context; output is updated only on success.
// No shader compilation, allocation, dispatch or copy occurs when disabled.
class D3D11 {
public:
    bool run(ID3D11DeviceContext* context, ID3D11Texture2D* output, ID3D11Texture2D* motion,
        ID3D11Texture2D* depth, ID3D11Texture2D* exposure, const Options& options, Frame frame);
    uint64_t dispatches() const { return dispatches_; }
private:
    bool prepare(ID3D11Device* device, const D3D11_TEXTURE2D_DESC& output);
    Microsoft::WRL::ComPtr<ID3D11Device> device_;
    Microsoft::WRL::ComPtr<ID3D11ComputeShader> shaders_[3];
    Microsoft::WRL::ComPtr<ID3D11Buffer> constants_;
    Microsoft::WRL::ComPtr<ID3D11Texture2D> scratch_;
    Microsoft::WRL::ComPtr<ID3D11UnorderedAccessView> target_;
    uint64_t dispatches_ = 0;
};
}
