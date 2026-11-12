// SPDX-License-Identifier: GPL-3.0-only
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
//
// Every other exit (end of phase_combine without a seam, a stale window at the next prepare, end of frame, shutdown, a
// failed write) also restores. This file has no D3D or Windows dependency so the state machine is tested on fake
// layouts (tests/anomaly_sr_swap.cpp).
#include "sr_plan.h"
#include <array>
#include <cstddef>
#include <cstdint>

namespace rsf::anomaly::sr {
inline constexpr std::size_t texture2d_desc_size = 44;   // D3D11_TEXTURE2D_DESC, cached in CTexture::desc
inline constexpr uint32_t d3dfmt_a8r8g8b8 = 21, d3dfmt_a16b16g16r16f = 113;

// The swapped values of one render target: its CRT record and the CTexture it is bound through.
struct TargetFields {
    void* surface{};
    void* rtv{};
    void* dsv{};
    void* uav{};
    uint32_t width{}, height{}, format{};   // CRT dwWidth, dwHeight, fmt (D3DFORMAT)
    void* texture_surface{};                // CTexture::pSurface
    void* desc_cache{};                     // CTexture::desc_cache
    std::array<std::byte, texture2d_desc_size> desc{};
    void* srv{};                            // CTexture::m_pSRView
    bool operator==(const TargetFields&) const = default;
};
// One engine target: where its CRT and CTexture live, and the values read from them. `present` is false for an
// optional target whose ref_rt is null.
struct EngineTarget {
    std::byte* crt{};
    std::byte* texture{};
    TargetFields fields;
    bool present{};
};

// Order of sr_world_targets (sr_profile.h). Stage 1 uses the first stage1_slot_count slots.
enum class Slot : uint8_t { Generic0, GenericTemp, Position, Albedo, Accumulator, Generic1, Generic2, HalfDepth, SsaoTemp, Count };
inline constexpr std::size_t slot_count = static_cast<std::size_t>(Slot::Count);
inline constexpr std::size_t stage1_slot_count = 2;
inline constexpr std::size_t slot_index(Slot slot) noexcept { return static_cast<std::size_t>(slot); }
// Slots whose twins are R16G16B16A16_FLOAT regardless of the engine format (the S-B colour and its copy partner).
inline constexpr bool slot_promoted(std::size_t slot) noexcept { return slot < stage1_slot_count; }

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
    Rollback,      // an exchange write failed part way
    FrameEnd,      // IRenderDeviceRender::End found the window open (defensive; combine_end should have run)
    Count
};
const char* skip_name(Skip) noexcept;
const char* restore_name(Restore) noexcept;

struct FrameFacts {
    std::byte* render_target{};    // CRenderTarget* passed to phase_scene_prepare
    std::byte* backend{};          // &RCache, for the texture-slot cache invalidation; null skips it
    uint64_t device{};             // HW.pDevice identity
    uint64_t generation{};         // reset generation (native observer)
    uint32_t device_width{}, device_height{};   // Device.dwWidth/dwHeight at H1: the output size
    bool scope{}, msaa{};
    // Stage 2. stage 1 ignores everything below.
    uint32_t stage{1};
    uint32_t render_width{}, render_height{};   // from make_plan(device size, SRScale)
    std::byte* device_size{};      // &Device.dwWidth (dwHeight follows at +4)
    std::byte* base_zb{};          // &HW.pBaseZB
    std::byte* base_rt{};          // &HW.pBaseRT
    std::byte* ssao_viewport{};    // &set_viewport's static D3D11_VIEWPORT; null leaves it alone
    std::byte* ssao_viewport_guard{};
};

// What the host must create on a twin-key change.
struct TwinSpec {
    bool present{};
    uint32_t width{}, height{};
    bool promote{};                // R16G16B16A16_FLOAT instead of the engine format
};
struct TwinRequest {
    uint32_t stage{1};
    std::array<EngineTarget, slot_count> engine{};
    std::array<TwinSpec, slot_count> spec{};
    void* engine_dsv{};            // stage 2: HW.pBaseZB, validated by the host
    void* engine_rtv{};            // stage 2: HW.pBaseRT (the back buffer view)
    uint32_t render_width{}, render_height{}, output_width{}, output_height{};
};
struct TwinSet {
    std::array<TargetFields, slot_count> targets{};
    void* depth_dsv{};             // stage 2 depth twin: D24_UNORM_S8_UINT view of an R24G8_TYPELESS texture
    void* depth_srv{};             // R24_UNORM_X8_TYPELESS view of the same texture
    uint32_t depth_width{}, depth_height{};
    void* base_rt{};               // stage 2: render-size scratch RTV standing in for HW.pBaseRT inside the window
};
enum class MakeResult : uint8_t {
    Created,
    Failed,     // allocation or shader creation failed: stay stock until the twin key changes
    Depth,      // stage 2: the engine depth view is not usable; refuse
    Refused,    // an engine target is not twinnable (format/shape); refuse
};
// The seam pass of Stage 2: render-size inputs, output-size engine targets.
struct UpscaleInputs {
    const TargetFields* generic0_twin{};    // FP16, engine low tonemap space
    const TargetFields* position_twin{};
    const TargetFields* generic1_twin{};
    void* depth_srv{};
    const TargetFields* generic0{};         // engine UNORM8 originals at output size
    const TargetFields* position{};
    const TargetFields* generic1{};
    void* engine_dsv{};
    uint32_t render_width{}, render_height{}, output_width{}, output_height{};
};

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
protected:
    ~SwapHost() = default;
};

struct SwapCounters {
    uint64_t prepares{}, eligible{}, swapped{}, seams{}, seams_without_window{}, round_trips{}, round_trip_failures{};
    uint64_t combine_ends{}, rm_near_in_window{}, twin_generations{}, twin_drops{}, foreign_changes{}, write_faults{};
    uint64_t allocation_failures{}, viewports{}, ssao_viewport_swaps{};
    std::array<uint64_t, static_cast<std::size_t>(Skip::Count)> skipped{};
    std::array<uint64_t, static_cast<std::size_t>(Restore::Count)> restores{};
};

class SwapController {
public:
    explicit SwapController(SwapHost& host) noexcept : host_(host) {}
    SwapController(const SwapController&) = delete;
    SwapController& operator=(const SwapController&) = delete;
    // false: every prepare is skipped as Off and the engine is never touched (capture-only stock runs).
    void set_swap_enabled(bool enabled) noexcept { swap_enabled_ = enabled; }
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
    // CRender::rmNear inside the window (first-person projection). Counted only; Stage 3 jitters here.
    void rm_near() noexcept;
    // Restore if open, then drop the twins. Safe at any time on the render thread.
    void shutdown() noexcept;
    bool window_open() const noexcept { return open_; }
    bool refused() const noexcept { return refusal_ != nullptr; }
    const char* refusal() const noexcept { return refusal_; }
    uint64_t twin_generation() const noexcept { return counters_.twin_generations; }
    const SwapCounters& counters() const noexcept { return counters_; }
    // The engine fields saved at the last exchange (valid while the window is open) and the current twins.
    const std::array<EngineTarget, slot_count>& engine() const noexcept { return engine_; }
    const std::array<TargetFields, slot_count>& twins() const noexcept { return twins_.targets; }
    const TwinSet& twin_set() const noexcept { return twins_; }
    // Stage and sizes of the last window that opened (diagnostics and capture records).
    uint32_t stage() const noexcept { return stage_; }
    uint32_t render_width() const noexcept { return render_width_; }
    uint32_t render_height() const noexcept { return render_height_; }
    uint32_t output_width() const noexcept { return output_width_; }
    uint32_t output_height() const noexcept { return output_height_; }

private:
    struct TwinKey {
        uint32_t stage{};
        std::byte* render_target{};
        std::array<std::byte*, slot_count> crt{};
        std::array<void*, slot_count> surface{};
        void* engine_dsv{};
        void* engine_rtv{};
        uint64_t device{}, generation{};
        uint32_t width{}, height{}, render_width{}, render_height{};
        bool operator==(const TwinKey&) const = default;
    };
    // Engine globals Stage 2 exchanges, with the values saved at the exchange.
    struct Globals {
        std::byte* device_size{};
        std::byte* base_zb{};
        std::byte* base_rt{};
        std::byte* viewport{};
        std::byte* viewport_guard{};
        uint32_t width{}, height{};          // Device.dwWidth/dwHeight (output)
        void* zb{};                           // HW.pBaseZB (engine DSV)
        void* rt{};                           // HW.pBaseRT (engine back buffer RTV)
        bool viewport_swapped{};
        bool viewport_guard_was_set{};       // the static was initialised when the window opened
        float viewport_width{}, viewport_height{};
    };
    std::size_t slots() const noexcept { return stage_ == 1 ? stage1_slot_count : slot_count; }
    bool read_targets(const FrameFacts&, std::array<EngineTarget, slot_count>&, TwinRequest&) noexcept;
    bool twins_match(const TwinRequest&) const noexcept;
    bool store(const EngineTarget&, const TargetFields&) noexcept;
    bool load(const EngineTarget&, TargetFields&) noexcept;
    bool open_globals() noexcept;
    void restore_globals(bool rollback) noexcept;
    bool viewport_initialised() noexcept;
    void restore(Restore) noexcept;
    void invalidate_caches() noexcept;
    void refuse(const char* reason) noexcept { if (!refusal_) refusal_ = reason; }
    void drop() noexcept;

    SwapHost& host_;
    bool swap_enabled_{};
    bool open_{};
    bool have_twins_{};
    bool failed_key_valid_{};
    const char* refusal_{};
    std::byte* backend_{};
    std::byte* render_target_{};
    uint32_t stage_{1};
    uint32_t render_width_{}, render_height_{}, output_width_{}, output_height_{};
    TwinKey key_{}, failed_key_{};
    std::array<EngineTarget, slot_count> engine_{};
    TwinSet twins_{};
    Globals globals_{};
    SwapCounters counters_{};
};
}
