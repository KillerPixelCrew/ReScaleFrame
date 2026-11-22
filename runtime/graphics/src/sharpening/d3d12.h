// SPDX-License-Identifier: MIT
#pragma once
#include "options.h"
#include "d3d12_platform.h"
#include <d3d12.h>

namespace rsf::sharpening {
// Caller serializes calls, retires slot 0..2 before reuse, and drains all work before destruction.
// Output enters/leaves UAV; optional inputs remain NON_PIXEL_SHADER_RESOURCE.
// Root constants are copied into the command recording; descriptors/scratch are per slot.
class D3D12 {
public:
    D3D12() = default;
    D3D12(const D3D12&) = delete;
    D3D12& operator=(const D3D12&) = delete;
    ~D3D12() { rsf_core_sharpen12_destroy(owner_); }

    bool run(ID3D12GraphicsCommandList* commands, ID3D12Resource* output, ID3D12Resource* motion,
        ID3D12Resource* depth, ID3D12Resource* exposure, const Options& request, Frame frame, uint32_t slot)
    {
        const auto options = sanitize(request);
        if (!active(options)) return true;
        const rsf_sharpen12_call call{commands, {output, motion, depth, exposure}, &options, &frame,
            static_cast<uint32_t>(options.mode), slot};
        return rsf_core_sharpen12_run(&owner_, &call) != 0;
    }
    uint64_t dispatches() const { return rsf_core_sharpen12_dispatches(owner_); }
private:
    rsf_sharpen12* owner_ = nullptr;
};
}
