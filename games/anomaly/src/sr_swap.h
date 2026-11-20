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
// failed write, a refused sun cascade) also restores. The state machine is owned by sr_swap.rs. SwapController forwards
// each hook to it and lends a SwapHost's guarded engine copies and D3D/engine calls as its platform, so the contract is
// tested on fake layouts (tests/anomaly_sr_swap.cpp) through the production owner.
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

// The native leaf: guarded engine copies and the D3D11 and engine calls the owner sequences.
class SwapHost {
public:
    // Engine memory access. Production guards both against access violations.
    virtual bool read(const void* address, void* output, std::size_t size) noexcept = 0;
    virtual bool write(void* address, const void* input, std::size_t size) noexcept = 0;
    // Creates the twins in `request.spec` (and the stage 2 depth twin), each with render-target and shader-resource
    // views and a UAV exactly when the engine target has one. Called only when the twin key changes; drop_twins first.
    virtual MakeResult make_twins(const TwinRequest& request, TwinSet& twins) noexcept = 0;
    virtual void drop_twins() noexcept = 0;
    // Stage 1, after the restore at the seam: reads twin generic0, writes the stock curve into engine generic0.
    virtual bool round_trip(const TargetFields& twin_generic0, const TargetFields& engine_generic0) noexcept = 0;
    // Stage 2, after the restore at the seam.
    virtual bool upscale(const UpscaleInputs&) noexcept = 0;
    // Stage 2: the D3D11 viewport {0, 0, width, height, 0, 1}, what CRender::rmNormal sets for a bound target.
    virtual void set_viewport(uint32_t width, uint32_t height) noexcept = 0;
    // Stage 3: R_xforms::set_P(&RCache.xforms, matrix), re-uploading the projection the engine binds from `matrix`
    // (always &Device.mProject). False when the call faulted.
    virtual bool upload_projection(const void* matrix) noexcept = 0;
    // Stage 3: dx10SamplerStateCache::SetMipLODBias(&SSManager, value). Rebuilds every sampler when the value changes.
    virtual bool set_mip_bias(float value) noexcept = 0;
    virtual bool correct_position_constant(void*, float, float) noexcept { return false; }
protected:
    ~SwapHost() = default;
};

// The hooks' handle on the Rust owner. Render thread only, under the SR stage guard. `host` outlives the controller.
class SwapController {
public:
    explicit SwapController(SwapHost& host) noexcept;
    ~SwapController();
    SwapController(const SwapController&) = delete;
    SwapController& operator=(const SwapController&) = delete;
    // false: every prepare is skipped as Off and the engine is never touched (capture-only stock runs).
    void set_swap_enabled(bool enabled) noexcept;
    // Restores a window left open by an earlier frame (counted as StalePrepare). The H1 hook calls this before it
    // reads Device.dwWidth for the next FrameFacts; prepare() does it too.
    void close_stale() noexcept;
    // H1 prologue. True when the window opened: the engine targets now point at the twins.
    bool prepare(const FrameFacts&) noexcept;
    // H1 epilogue (after the original phase_scene_prepare). Stage 2: viewport to render size.
    void prepared() noexcept;
    // S-B seam. True when it closed a window (restore, then the seam pass). False passes through (menu, SVP, stock).
    bool seam() noexcept;
    // End of phase_combine, the last call of CRender::Render. Restores and refuses when the seam did not fire.
    void combine_end() noexcept;
    // Before IRenderDeviceRender::End. Restores and refuses when a window survived the whole frame.
    void frame_end() noexcept;
    // CRender::rmNear, after the original (the shared geometry slot). Counted inside the window; with Stage 3 jitter
    // it jitters a freshly built first-person projection once.
    void rm_near() noexcept;
    // Around CRender::render_sun_cascades: fit shadows to the saved main camera, then restore the exact jittered pair
    // and upload it through set_P. The token is true only when begin suspended jitter; pass it once to end. A refusal
    // closes the window and returns the engine to output size.
    bool sun_cascades_begin() noexcept;
    void sun_cascades_end(bool suspended) noexcept;
    // After the engine binder uploads stock values, correct its constant for the jittered G-buffer camera.
    void position_constant_setup(void* constant) noexcept;
    // Stage 3 state of the open (or last) window.
    bool jitter_applied() const noexcept { return view_->jitter_applied; }
    bool mip_bias_applied() const noexcept { return view_->mip_bias_applied; }
    float window_mip_bias() const noexcept { return view_->window_mip_bias; }
    float saved_mip_bias() const noexcept { return view_->saved_mip_bias; }
    // Restore if open, then drop the twins. Safe at any time on the render thread.
    void shutdown() noexcept;
    bool window_open() const noexcept { return view_->open; }
    bool refused() const noexcept { return view_->refusal != nullptr; }
    const char* refusal() const noexcept { return view_->refusal; }
    uint64_t twin_generation() const noexcept { return view_->counters.twin_generations; }
    // Owned by the Rust state and updated by every call; a held reference sees later counts.
    const SwapCounters& counters() const noexcept { return view_->counters; }
    // The engine fields saved at the last exchange (valid while the window is open) and the current twins.
    using Targets = EngineTarget[slot_count];
    using Twins = TargetFields[slot_count];
    const Targets& engine() const noexcept { return view_->engine; }
    const Twins& twins() const noexcept { return view_->twins.targets; }
    const TwinSet& twin_set() const noexcept { return view_->twins; }
    // Stage and sizes of the last window that opened (diagnostics and capture records).
    uint32_t stage() const noexcept { return view_->stage; }
    uint32_t render_width() const noexcept { return view_->render_width; }
    uint32_t render_height() const noexcept { return view_->render_height; }
    uint32_t output_width() const noexcept { return view_->output_width; }
    uint32_t output_height() const noexcept { return view_->output_height; }

private:
    rsf_anomaly_swap* state_{};
    const rsf_anomaly_swap_view* view_{};
};
}
