// SPDX-License-Identifier: GPL-3.0-only
#pragma once
// SR Stage 1 world-window twin swap, mechanics (b) of docs/research/anomaly-sr-seam.md 1.4.
//
// Between the prologue of CRenderTarget::phase_scene_prepare (H1) and the S-B seam (first of phase_sunshafts /
// phase_blur, H2), the engine's generic0 and generic_temp CRT records and their CTextures point at RSF-owned
// FP16 twins. The exchange is a plain field swap: no reference count changes, no SRV is created per frame.
// At the seam the originals come back and the host runs the round trip into the engine's UNORM8 generic0.
// Every other exit (end of phase_combine without a seam, a stale window at the next prepare, shutdown, a failed
// write) also restores. This file has no D3D or Windows dependency so the state machine is tested on fake layouts.
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
// One engine target: where its CRT and CTexture live, and the values read from them.
struct EngineTarget {
    std::byte* crt{};
    std::byte* texture{};
    TargetFields fields;
};

enum class Slot : uint8_t { Generic0, GenericTemp, Count };
inline constexpr std::size_t slot_count = static_cast<std::size_t>(Slot::Count);
// Why an H1 call did not open a window.
enum class Skip : uint8_t {
    Off,      // swap not selected (capture-only stock run) or refused earlier
    Scope,    // second-viewport (scope) frame: stays stock at output size
    Msaa,     // o.dx10_msaa: refused, not patched
    Layout,   // engine fields did not match the verified layout; refuses further swaps
    Twins,    // twins could not be created; refuses further swaps
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
    uint32_t device_width{}, device_height{};
    bool scope{}, msaa{};
};

class SwapHost {
public:
    // Engine memory access. Production guards both against access violations.
    virtual bool read(const void* address, void* output, std::size_t size) noexcept = 0;
    virtual bool write(void* address, const void* input, std::size_t size) noexcept = 0;
    // Creates FP16 twins of the two engine targets (same size, render-target and shader-resource views, and a UAV
    // exactly when the engine target has one). Called only when the twin key changes; drop_twins comes first.
    virtual bool make_twins(const std::array<EngineTarget, slot_count>& engine,
        std::array<TargetFields, slot_count>& twins) noexcept = 0;
    virtual void drop_twins() noexcept = 0;
    // Runs after the restore at the seam: reads twin generic0, writes the stock curve into engine generic0.
    virtual bool round_trip(const TargetFields& twin_generic0, const TargetFields& engine_generic0) noexcept = 0;
protected:
    ~SwapHost() = default;
};

struct SwapCounters {
    uint64_t prepares{}, eligible{}, swapped{}, seams{}, seams_without_window{}, round_trips{}, round_trip_failures{};
    uint64_t combine_ends{}, rm_near_in_window{}, twin_generations{}, twin_drops{}, foreign_changes{}, write_faults{};
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
    // H1 prologue. True when the window opened: the engine targets now point at the twins.
    bool prepare(const FrameFacts&) noexcept;
    // S-B seam. True when it closed a window (restore, then round trip). False passes through (menu, SVP, stock).
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
    const std::array<TargetFields, slot_count>& twins() const noexcept { return twins_; }

private:
    struct TwinKey {
        std::byte* render_target{};
        std::array<std::byte*, slot_count> crt{};
        std::array<void*, slot_count> surface{};
        uint64_t device{}, generation{};
        uint32_t width{}, height{};
        bool operator==(const TwinKey&) const = default;
    };
    bool read_target(std::byte* render_target, Slot, const FrameFacts&, EngineTarget&) noexcept;
    bool twins_match(const std::array<EngineTarget, slot_count>&) const noexcept;
    bool store(const EngineTarget&, const TargetFields&) noexcept;
    bool load(const EngineTarget&, TargetFields&) noexcept;
    void restore(Restore) noexcept;
    void invalidate_texture_cache() noexcept;
    void refuse(const char* reason) noexcept { if (!refusal_) refusal_ = reason; }
    void drop() noexcept;

    SwapHost& host_;
    bool swap_enabled_{};
    bool open_{};
    bool have_twins_{};
    const char* refusal_{};
    std::byte* backend_{};
    TwinKey key_{};
    std::array<EngineTarget, slot_count> engine_{};
    std::array<TargetFields, slot_count> twins_{};
    SwapCounters counters_{};
};
}
