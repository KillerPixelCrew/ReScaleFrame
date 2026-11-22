// SPDX-License-Identifier: MIT
#pragma once
#include <rescaleframe/game_renderer.h>
#include <d3d11.h>
#include <dxgi.h>
#include <wrl/client.h>

namespace rsf::fallout4 {
struct NativeDepthSnapshot;
// Stack-owned synchronous callback lease. Never retain this snapshot after the
// native callback; allocation references do not retain the frame's contents.
struct NativeInputs {
    Microsoft::WRL::ComPtr<ID3D11DeviceContext> context;
    Microsoft::WRL::ComPtr<IDXGISwapChain> chain;
    Microsoft::WRL::ComPtr<ID3D11Texture2D> colour, depth, motion;
    // Optional full-output allocations. Native logical targets 14, 30 and 15;
    // 30 carries Pip-Boy material class, 15 carries VATS coverage. Their actual
    // formats remain native facts. A null lease excludes only its consumer.
    Microsoft::WRL::ComPtr<ID3D11Texture2D> refraction_normals, pipboy, vats;
    rsf_game_render_pass pass{};
};
struct NativeFramebuffer {
    Microsoft::WRL::ComPtr<ID3D11DeviceContext> context;
    Microsoft::WRL::ComPtr<IDXGISwapChain> chain;
    Microsoft::WRL::ComPtr<ID3D11Texture2D> texture;
    uint64_t native_frame{}, window_key{};
};
// Raster/resource lease only. No temporal camera/history/depth admission. All
// downstream allocations are conservatively required before native reduction.
bool read_native_raster_inputs(NativeInputs&, const rsf_game_render_pass& frame,
    const NativeDepthSnapshot* producer = nullptr) noexcept;
// Supplies native resource/camera/rect/frame facts only. Caller supplies the
// verified session/source/submission/scope, role, screen and eligibility facts.
bool read_native_inputs(NativeInputs&, void (*report)(const char*) = nullptr,
    const NativeDepthSnapshot* producer = nullptr) noexcept;
// Logical FrameBuffer0 resolved after imagespace. The caller owns the explicit
// FINAL_SCENE/WINDOW begin/end lease, including native Swap, and checks the
// retained pre-SR scalar identity before publishing it as this source's output.
// Optional receipts name the first failed native reader predicate per stage.
bool read_native_framebuffer(NativeFramebuffer&, void (*report)(const char*) = nullptr) noexcept;
}
