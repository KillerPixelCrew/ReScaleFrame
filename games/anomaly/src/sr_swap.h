// SPDX-License-Identifier: MIT
#pragma once
// SR world-window twin swap, mechanics (b) of docs/research/anomaly-sr-seam.md 1.4.
//
// Between the prologue of CRenderTarget::phase_scene_prepare (H1) and the S-B seam (first of phase_sunshafts /
// phase_blur, H2), engine render targets point at RSF-owned twins. The exchange is a plain field swap: no reference
// count changes, no SRV is created per frame.
//
// Stage 1 (section 9): generic0 and generic_temp only, FP16, at device size. At the seam the host runs the f^-1/f
// round trip into the engine's UNORM8 generic0.
// Stage 2 (section 10): every world target in sr_world_targets at render size, plus Device.dwWidth/dwHeight, HW.pBaseZB
// (a render-size R24G8 depth twin), HW.pBaseRT (a render-size scratch view: phase_occq binds it with HW.pBaseZB, and
// D3D11 rejects an output-size RTV with a render-size DSV) and the SSAO helper's static viewport. After H1 the
// viewport is set to render size.
// At the seam everything is restored, then the host upscales generic0 (bilinear in linear space), and nearest-copies
// rt_Position, generic1 and depth into the engine's output-size originals, and the viewport returns to output size.
// Stage 3 (section 11) adds, inside the same window: Device.mProject/mFullTransform jittered and re-uploaded through
// R_xforms::set_P, the six first-person projections jittered once each from the rmNear slot, and the sampler mip
// bias lowered through SetMipLODBias. Every exit path restores them with the twins.
//
// Every other exit (end of phase_combine without a seam, a stale window at the next prepare, end of frame, shutdown, a
// or failed write) also restores. sr_swap.rs owns transaction and recovery ordering.
#include "sr_swap_platform.h"
#include <cstddef>
#include <cstdint>
#include <cstring>

// Field-wise equality of the shared record (padding is not compared).
inline bool operator==(const rsf_anomaly_swap_fields& a, const rsf_anomaly_swap_fields& b) noexcept {
    return a.surface == b.surface && a.rtv == b.rtv && a.dsv == b.dsv && a.uav == b.uav && a.width == b.width &&
        a.height == b.height && a.format == b.format && a.texture_surface == b.texture_surface &&
        a.desc_cache == b.desc_cache && std::memcmp(a.desc, b.desc, sizeof(a.desc)) == 0 && a.srv == b.srv;
}

namespace rsf::anomaly::sr {
inline constexpr std::size_t texture2d_desc_size = RSF_ANOMALY_SWAP_DESC_SIZE;
inline constexpr uint32_t d3dfmt_a8r8g8b8 = 21, d3dfmt_a16b16g16r16f = 113;

// The swapped values of one render target (sr_swap_platform.h).
using TargetFields = rsf_anomaly_swap_fields;
// One engine target, its CRT and CTexture and the values read from them.
using EngineTarget = rsf_anomaly_swap_target;
// What H1 read for this frame. A zero `stage` means Stage 1.
using FrameFacts = rsf_anomaly_swap_facts;
using TwinSpec = rsf_anomaly_swap_spec;
using TwinRequest = rsf_anomaly_swap_request;
using TwinSet = rsf_anomaly_swap_twins;
using UpscaleInputs = rsf_anomaly_swap_upscale;
using SwapCounters = rsf_anomaly_swap_counters;

// Order of sr_world_targets (sr_profile.h). Stage 1 uses the first stage1_slot_count slots.
enum class Slot : uint8_t { Generic0, GenericTemp, Position, Albedo, Accumulator, Generic1, Generic2, HalfDepth, SsaoTemp, Count };
inline constexpr std::size_t slot_count = RSF_ANOMALY_SWAP_SLOT_COUNT;
inline constexpr std::size_t stage1_slot_count = RSF_ANOMALY_SWAP_STAGE1_SLOT_COUNT;
inline constexpr std::size_t slot_index(Slot slot) noexcept { return static_cast<std::size_t>(slot); }
static_assert(static_cast<std::size_t>(Slot::Count) == slot_count);

// Why an H1 call did not open a window.
enum class Skip : uint8_t {
    Off,      // swap not selected (capture-only stock run) or refused earlier
    Scope,    // second-viewport (scope) frame: stays stock at output size
    Msaa,     // o.dx10_msaa: refused, not patched
    Layout,   // engine fields did not match the verified layout; refuses further swaps
    Twins,    // twins could not be allocated: this frame and every frame with the same twin key stay stock
    Depth,    // Stage 2: HW.pBaseZB is not a single-sample D24S8 view of output size; refuses further swaps
    Count
};
// Which exit path restored the engine fields.
enum class Restore : uint8_t {
    Seam,          // S-B seam (normal)
    CombineEnd,    // phase_combine returned with the window open: the seam did not fire; refuses further swaps
    StalePrepare,  // the next H1 found the window still open
    Shutdown,      // quiesce/stop or capture teardown
    Rollback,      // an exchange or sun cascade write failed: refuses further swaps
    FrameEnd,      // IRenderDeviceRender::End found the window open (defensive; combine_end should have run)
    Count
};
static_assert(static_cast<uint32_t>(Skip::Depth) == RSF_ANOMALY_SWAP_SKIP_DEPTH &&
    static_cast<uint32_t>(Skip::Count) == RSF_ANOMALY_SWAP_SKIP_COUNT);
static_assert(static_cast<uint32_t>(Restore::Rollback) == RSF_ANOMALY_SWAP_RESTORE_ROLLBACK &&
    static_cast<uint32_t>(Restore::FrameEnd) == RSF_ANOMALY_SWAP_RESTORE_FRAME_END &&
    static_cast<uint32_t>(Restore::Count) == RSF_ANOMALY_SWAP_RESTORE_COUNT);
const char* skip_name(Skip) noexcept;
const char* restore_name(Restore) noexcept;

enum class MakeResult : uint8_t {
    Created,
    Failed,     // allocation or shader creation failed: stay stock until the twin key changes
    Depth,      // stage 2: the engine depth view is not usable; refuse
    Refused,    // an engine target is not twinnable (format/shape); refuse
};
static_assert(static_cast<uint32_t>(MakeResult::Failed) == RSF_ANOMALY_SWAP_MAKE_FAILED &&
    static_cast<uint32_t>(MakeResult::Depth) == RSF_ANOMALY_SWAP_MAKE_DEPTH &&
    static_cast<uint32_t>(MakeResult::Refused) == RSF_ANOMALY_SWAP_MAKE_REFUSED);
}
