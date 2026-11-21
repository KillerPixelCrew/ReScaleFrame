// SPDX-License-Identifier: MIT
#pragma once
#include "options.h"
#include <d3d11.h>

struct rsf_sharpen11;
namespace rsf::sharpening {
// Internal device helper. Call on the owning immediate context; output is updated only on success.
// No shader compilation, allocation, dispatch or copy occurs when disabled.
class D3D11 {
public:
    D3D11() = default;
    ~D3D11();
    D3D11(const D3D11&) = delete;
    D3D11& operator=(const D3D11&) = delete;
    bool run(ID3D11DeviceContext* context, ID3D11Texture2D* output, ID3D11Texture2D* motion,
        ID3D11Texture2D* depth, ID3D11Texture2D* exposure, const Options& options, Frame frame);
    uint64_t dispatches() const;
private:
    rsf_sharpen11* owner_{};
};
}
