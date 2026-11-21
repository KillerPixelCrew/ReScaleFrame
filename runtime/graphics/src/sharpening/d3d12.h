// SPDX-License-Identifier: MIT
#pragma once
#include "options.h"
#include <d3d12.h>
struct rsf_sharpen12;

namespace rsf::sharpening {
// Caller serializes calls, retires slot 0..2 before reuse, and drains all work before destruction.
// Output enters/leaves UAV; optional inputs remain NON_PIXEL_SHADER_RESOURCE.
// Root constants are copied into the command recording; descriptors/scratch are per slot.
class D3D12 {
public:
    D3D12() = default;
    D3D12(const D3D12&) = delete;
    D3D12& operator=(const D3D12&) = delete;
    ~D3D12();

    bool run(ID3D12GraphicsCommandList*, ID3D12Resource* output, ID3D12Resource* motion,
        ID3D12Resource* depth, ID3D12Resource* exposure, const Options&, Frame, uint32_t slot);
    uint64_t dispatches() const;
private:
    rsf_sharpen12* owner_ = nullptr;
};
}
