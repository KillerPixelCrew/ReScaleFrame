// SPDX-License-Identifier: GPL-3.0-only
#pragma once
#include <rescaleframe/shader_patch.h>
// D3D11 side of the motion MRT: the RGBA16F target, the b13 buffers and the invalidation replay for occluding draws
// whose shaders have no motion variant.
//
// Invalidation replay: right after such a draw, the same draw is issued again with only the motion RTV bound in slot
// 2 (slots 0/1 null), an RSF pixel shader with no inputs that writes (0,0,0,0) to SV_Target2, blending off, and a
// depth-stencil state derived from the draw's own: depth test EQUAL without writes, stencil off. The same VS, input
// and rasterizer state produce bit-identical depth, so exactly the samples the draw won the depth test for become
// invalid, and samples it discarded (alpha test) keep the surface behind them. A draw that does not write depth
// (decal-like: the visible surface is unchanged) is not replayed; one without a depth test invalidates everything it
// covers. Output-merger state and the pixel shader are restored afterwards.
#include "motion_constants.h"
#include <d3d11.h>
#include <wrl/client.h>
#include <cstdint>

namespace rsf::anomaly::motion {
void set_shader_runtime(const rsf_shader_patch_api&) noexcept;
using Microsoft::WRL::ComPtr;
using LogFn = void (*)(void*, const char*);

enum class ReplayResult : std::uint8_t { Done, SkippedNoDepthWrite, Failed };
enum class TargetResult : std::uint8_t { Ready, Created, Multisampled, Failed };

class MotionDevice {
public:
    bool create(ID3D11Device*, LogFn log, void* user) noexcept;
    void reset() noexcept;
    ID3D11Device* device() const noexcept { return device_.Get(); }
    // Matches the motion target to the G-buffer's RT0 texture: same size, one sample, R16G16B16A16_FLOAT.
    TargetResult ensure_target(const D3D11_TEXTURE2D_DESC& rt0) noexcept;
    ID3D11RenderTargetView* rtv() const noexcept { return rtv_.Get(); }
    ID3D11Texture2D* texture() const noexcept { return texture_.Get(); }
    std::uint32_t width() const noexcept { return width_; }
    std::uint32_t height() const noexcept { return height_; }
    void clear(ID3D11DeviceContext*) noexcept;
    // Map(WRITE_DISCARD) unless the bytes equal the previous upload; binds VS or PS slot 13. Returns false on failure.
    bool set_core_vertex(ID3D11DeviceContext*, const CoreVertexBytes&) noexcept;
    bool set_foliage_vertex(ID3D11DeviceContext*, const FoliageVertexBytes&) noexcept;
    bool set_pixel(ID3D11DeviceContext*, const PixelBytes&) noexcept;
    // DS slot 13 (RSFTessPrevious) for a swapped tessellation chain.
    bool set_tess_domain(ID3D11DeviceContext*, const TessDomainBytes&) noexcept;
    // Re-issues the draw (`redraw(context)`) as the invalidation pass described above.
    template<class Redraw> ReplayResult invalidate(ID3D11DeviceContext* context, Redraw&& redraw) noexcept
    {
        Saved saved;
        const auto prepared = begin_replay(context, saved);
        if (prepared != ReplayResult::Done) { end_replay(context, saved); return prepared; }
        redraw(context);
        end_replay(context, saved);
        return ReplayResult::Done;
    }
    std::uint64_t uploads() const noexcept { return uploads_; }
    std::uint64_t upload_skips() const noexcept { return upload_skips_; }
private:
    struct Saved {
        ID3D11RenderTargetView* targets[D3D11_SIMULTANEOUS_RENDER_TARGET_COUNT]{};
        ID3D11DepthStencilView* depth{};
        ID3D11DepthStencilState* depth_state{};
        UINT reference{};
        ID3D11BlendState* blend{};
        FLOAT factor[4]{};
        UINT mask{};
        ID3D11PixelShader* shader{};
        ID3D11ClassInstance* classes[D3D11_SHADER_MAX_INTERFACES]{};
        UINT class_count{};
        bool taken{};
    };
    ReplayResult begin_replay(ID3D11DeviceContext*, Saved&) noexcept;
    void end_replay(ID3D11DeviceContext*, Saved&) noexcept;
    bool upload(ID3D11DeviceContext*, ID3D11Buffer*, const void*, std::size_t, std::byte* last, bool& have) noexcept;
    ComPtr<ID3D11Device> device_;
    ComPtr<ID3D11Texture2D> texture_;
    ComPtr<ID3D11RenderTargetView> rtv_;
    std::uint32_t width_{}, height_{};
    ComPtr<ID3D11Buffer> core_vs_, foliage_vs_, pixel_, tess_ds_;
    CoreVertexBytes last_core_{};
    FoliageVertexBytes last_foliage_{};
    PixelBytes last_pixel_{};
    TessDomainBytes last_tess_{};
    bool have_core_{}, have_foliage_{}, have_pixel_{}, have_tess_{};
    ComPtr<ID3D11PixelShader> zero_ps_;
    ComPtr<ID3D11DepthStencilState> no_depth_;
    ComPtr<ID3D11DepthStencilState> equal_;
    std::uint64_t uploads_{}, upload_skips_{};
};
// First-party HLSL of the invalidation writer (no inputs, so it links with any vertex shader).
inline constexpr char zero_motion_hlsl[] = "float4 main() : SV_Target2 { return float4(0, 0, 0, 0); }";
}
