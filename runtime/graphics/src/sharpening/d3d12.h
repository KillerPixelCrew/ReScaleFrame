// SPDX-License-Identifier: GPL-3.0-only
#pragma once
#include "options.h"
#include <d3d12.h>
#include <wrl/client.h>
#include <array>
namespace rsf::sharpening {
// Caller retires slot 0..2 before reuse, and all submitted work before destruction.
// Output enters/leaves UAV; optional inputs remain NON_PIXEL_SHADER_RESOURCE.
// Root constants are copied into the command recording; descriptors/scratch are per slot.
class D3D12 {
public:
    bool run(ID3D12GraphicsCommandList*, ID3D12Resource* output, ID3D12Resource* motion,
        ID3D12Resource* depth, ID3D12Resource* exposure, const Options&, Frame, uint32_t slot);
    uint64_t dispatches() const { return dispatches_; }
private:
    bool prepare(ID3D12Device*, const D3D12_RESOURCE_DESC&, uint32_t slot);
    Microsoft::WRL::ComPtr<ID3D12Device> device_;
    Microsoft::WRL::ComPtr<ID3D12RootSignature> root_;
    Microsoft::WRL::ComPtr<ID3D12PipelineState> pipelines_[3];
    std::array<Microsoft::WRL::ComPtr<ID3D12DescriptorHeap>,3> heaps_;
    std::array<Microsoft::WRL::ComPtr<ID3D12Resource>,3> scratch_;
    uint64_t dispatches_=0;
};
}
