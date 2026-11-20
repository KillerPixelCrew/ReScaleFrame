// SPDX-License-Identifier: MIT
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
#include "motion_device_platform.h"
#include <d3d11.h>
#include <wrl/client.h>
#include <cstdint>

namespace rsf::anomaly::motion {
const rsf_anomaly_motion_device_platform& motion_device_platform() noexcept;
void set_shader_runtime(const rsf_shader_patch_api&) noexcept;
using Microsoft::WRL::ComPtr;
using LogFn = void (*)(void*, const char*);

enum class ReplayResult : std::uint8_t { Done, SkippedNoDepthWrite, Failed };
enum class TargetResult : std::uint8_t {
    Ready = RSF_MOTION_READY, Created = RSF_MOTION_CREATED,
    Multisampled = RSF_MOTION_MULTISAMPLED, Failed = RSF_MOTION_FAILED
};

class MotionDevice {
public:
    MotionDevice() = default;
    ~MotionDevice();
    MotionDevice(const MotionDevice&) = delete;
    MotionDevice& operator=(const MotionDevice&) = delete;
    bool create(ID3D11Device*, LogFn log, void* user) noexcept;
    void reset() noexcept;
    ID3D11Device* device() const noexcept { return static_cast<ID3D11Device*>(view().device); }
    // Matches the motion target to the G-buffer's RT0 texture: same size, one sample, R16G16B16A16_FLOAT.
    TargetResult ensure_target(const D3D11_TEXTURE2D_DESC& rt0) noexcept;
    ID3D11RenderTargetView* rtv() const noexcept { return static_cast<ID3D11RenderTargetView*>(view().rtv); }
    ID3D11Texture2D* texture() const noexcept { return static_cast<ID3D11Texture2D*>(view().texture); }
    std::uint32_t width() const noexcept { return view().width; }
    std::uint32_t height() const noexcept { return view().height; }
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
        return invalidate(view(), context, static_cast<Redraw&&>(redraw));
    }
    template<class Redraw> static ReplayResult invalidate(const rsf_anomaly_motion_device_view& resources,
        ID3D11DeviceContext* context, Redraw&& redraw) noexcept
    {
        Saved saved;
        const auto prepared = begin_replay(resources, context, saved);
        if (prepared != ReplayResult::Done) { end_replay(context, saved); return prepared; }
        redraw(context);
        end_replay(context, saved);
        return ReplayResult::Done;
    }
    std::uint64_t uploads() const noexcept { return view().uploads; }
    std::uint64_t upload_skips() const noexcept { return view().upload_skips; }
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
        bool modified{};
    };
    static ReplayResult begin_replay(const rsf_anomaly_motion_device_view&, ID3D11DeviceContext*, Saved&) noexcept;
    static void end_replay(ID3D11DeviceContext*, Saved&) noexcept;
    rsf_anomaly_motion_device_view view() const noexcept {
        rsf_anomaly_motion_device_view result;
        rsf_anomaly_motion_device_get(owner_, &result);
        return result;
    }
    void* owner_{};
};
// First-party HLSL of the invalidation writer (no inputs, so it links with any vertex shader).
inline constexpr char zero_motion_hlsl[] = "float4 main() : SV_Target2 { return float4(0, 0, 0, 0); }";
}
