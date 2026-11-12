// SPDX-License-Identifier: GPL-3.0-only
#pragma once
// FG depth and motion when no SR evaluation produced them (SR off: SRStage 0 or 1, the world at output size).
// One compute pass on the engine's immediate context at the HUD-less boundary:
//   depth   HW.pBaseZB (D24S8, standard device depth, finite far) copied to an R24G8 twin and written as R32_FLOAT;
//   motion  the motion target (Motion=1, RGBA16F current-minus-previous UV, Y down, validity in z) where valid,
//           zero for unwritten first-person pixels (depth < sr::first_person_depth_limit, the rmNear range), and camera
//           motion reconstructed from depth and clip_to_previous_clip everywhere else; written as R16G16_FLOAT render
//           pixels, previous minus current, the convention every FG provider reads (motion scale 1).
// Without a motion target every pixel is camera motion (no object motion). Render thread only.
#include <d3d11.h>
#include <wrl/client.h>
#include <cstdint>

namespace rsf::anomaly::fg {
struct InputFrame {
    ID3D11Texture2D* depth{};          // the engine depth resource (D24_UNORM_S8_UINT or R24G8_TYPELESS, one sample)
    ID3D11Texture2D* motion{};         // optional RGBA16F motion target at the same size
    float clip_to_previous_clip[16]{}; // row-vector convention (clip_row * M), as rsf_camera_frame stores it
    uint32_t width{}, height{};
};
class InputPass {
public:
    enum class Result : uint8_t { Ok, Shader, Format, Size, Resources };
    // Builds lazily on the first run for this device. The outputs stay owned here until the next size change.
    Result run(ID3D11DeviceContext* context, const InputFrame& frame, void (*log)(void*, const char*), void* user) noexcept;
    ID3D11Texture2D* depth() const noexcept { return depth_out_.Get(); }
    ID3D11Texture2D* motion() const noexcept { return motion_out_.Get(); }
    void release() noexcept;

private:
    bool build(ID3D11Device*, void (*log)(void*, const char*), void* user) noexcept;
    bool size(ID3D11Device*, uint32_t width, uint32_t height) noexcept;
    Microsoft::WRL::ComPtr<ID3D11Device> device_;
    Microsoft::WRL::ComPtr<ID3D11ComputeShader> shader_;
    Microsoft::WRL::ComPtr<ID3D11Buffer> constants_;
    Microsoft::WRL::ComPtr<ID3D11Texture2D> depth_copy_, depth_out_, motion_out_;
    Microsoft::WRL::ComPtr<ID3D11ShaderResourceView> depth_view_;
    Microsoft::WRL::ComPtr<ID3D11UnorderedAccessView> depth_uav_, motion_uav_;
    uint32_t width_{}, height_{};
    bool failed_{};
};
}
