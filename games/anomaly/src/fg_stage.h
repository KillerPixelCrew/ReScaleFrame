// SPDX-License-Identifier: GPL-3.0-only
#pragma once
// Frame generation for Anomaly through the Runtime's shared D3D11 presentation bridge (rsf_get_d3d11_present_api)
// and its D3D11 FG tagging path (rsf_get_d3d11_fg_api). The adapter owns the engine boundaries, the screen policy
// (fg_policy.h), the HUD-less boundary and the SR-off depth/motion pass (fg_inputs.h). The Runtime owns the vendor
// providers, presentation, pacing, latency markers, shared surfaces and their retirement.
//
// INI ([Anomaly], all optional except FG):
//   FG=1                  generate frames from startup. FG=1 or FGBridge=1 installs the bridge at cold start (before
//                         device creation; it cannot be added later). The overlay's FG switch then toggles generation.
//   FGBridge              default 1: install the bridge with FG initially off (the overlay can turn it on); 0 keeps
//                         presentation stock unless FG=1
//   FGBackend=7           1 DLSS-G, 3 FSR3, 4 FSR4, 5 XeSS, 7 Auto (by the rendering adapter's vendor)
//   FGGeneratedFrames=1   generated frames per rendered frame, clamped by the provider (1..3)
//   Latency=1             0 off, 1 on, 2 boost (Reflex for DLSS-G; the provider's own pacing otherwise)
//   FrameLimitUs=0        minimum interval between rendered frames, before generation; 0 unlimited
//   FGScopeHold=4         engine frames held back after a scope (secondary viewport) frame
//   FGDebug=0             Runtime FG eligibility/pacing log lines
//   FGDevelopment=0       load the vendors' development runtimes
//   FGCapture=1           dump the FG inputs (HUD-less colour, depth, motion) through the frame dump service for
//                         FGCaptureCount (default 3, max 16) fed frames from FGCaptureStart (default 600), every
//                         FGCaptureInterval (default 300) engine frames, into FGCaptureDir
//                         (default <ini directory>\RSF-FGCapture\<local time>)
//   SRStreamlineDir / SRFidelityFXDir / SRXeSSDir   vendor runtimes, shared with SR
// The shared frontend keys (FG, FGBackend, FGGeneratedFrames, Latency, FrameLimitUs) are also what the overlay saves.
//
// Threads: everything except configure/install/status runs on the engine thread, which renders and Presents.
#include "overlay_hooks.h"
#include "build_profile.h"
#include <rescaleframe/game_api.h>
#include <rescaleframe/d3d11_fg_api.h>
#include <rescaleframe/d3d11_present_api.h>
#include <windows.h>
#include <cstdint>
#include <string>

struct ID3D11Device;
struct ID3D11DeviceContext;
struct IDXGISwapChain;

namespace rsf::anomaly::fg {
struct Options {
    bool bridge{};                    // FG=1 or FGBridge (default 1)
    bool enabled{};                   // FG
    uint32_t backend{7};              // FGBackend (RSF_FG_BACKEND_AUTO)
    uint32_t generated{1};            // FGGeneratedFrames
    uint32_t latency{1};              // Latency (RSF_REFLEX_ON)
    uint32_t frame_limit_us{};        // FrameLimitUs
    uint32_t scope_hold{4};           // FGScopeHold
    bool debug{}, development{};      // FGDebug, FGDevelopment
    bool capture{};
    uint32_t capture_start{600}, capture_count{3}, capture_interval{300};
    std::wstring capture_dir;
    std::wstring streamline_dir, fidelityfx_dir, xess_dir;
    bool selected() const noexcept { return bridge; }
};
Options read_options(const wchar_t* ini) noexcept;
void configure(const Options&, rsf_game_log_fn log, void* user) noexcept;
const Options& options() noexcept;

// Cold start, before the engine creates its device: binds both Runtime tables and installs the bridge's swap-chain
// interception. False leaves presentation stock (logged). One call per process.
// `build` is the exact executable profile bootstrap selected (build_profile.h); the SR-off pass reads its HW.pBaseZB.
bool install(HMODULE runtime, const BuildProfile& build) noexcept;
bool installed() noexcept;

// Test seam: tables supplied directly instead of resolved from the Runtime module (validated the same way).
bool install_with(const rsf_d3d11_present_api& present, const rsf_d3d11_fg_api& api) noexcept;
// Test seam: where the SR-off pass finds the engine's depth view. Production reads HW.pBaseZB (sr_profile.h).
void set_depth_view_source(void* (*source)()) noexcept;

// The overlay's Present sink, called from the bridge's before_present with the engine-facing swap chain, after every
// tag of the frame and before the upload. Set once by the bootstrap after the overlay started.
void set_overlay_sink(const overlay::PresentSink& sink) noexcept;
// True when the bridge presents and `chain` is not its engine-facing chain (a provider's physical or proxy chain):
// the overlay must not draw on it.
bool foreign_chain(IDXGISwapChain* chain) noexcept;

// Native engine facts. The Runtime owns pacing, marker mapping and input deduplication.
void input_begin(uint32_t engine_frame) noexcept;
void input_received(uint32_t kinds, uint32_t message_id = 0) noexcept;
// Native boundaries (native_observer.cpp), engine thread.
void camera(const float* view, const float* projection) noexcept;              // SetCacheXform, after the original
void render_begin(uint32_t engine_frame, uint32_t width, uint32_t height) noexcept;   // after IRenderDeviceRender::Begin
void scope_frame() noexcept;                                                    // after RenderToTarget(rtSVP)
void ui_boundary() noexcept;                                                    // CHUDManager::RenderUI, before it
void end_before() noexcept;                                                     // before IRenderDeviceRender::End
void end_after() noexcept;                                                      // after it (Present returned or skipped)
void device_reset() noexcept;                                                   // CRender::reset_begin
// Per-window and total counters for the observer's periodic log. Nothing when not installed.
void report(void (*emit)(void*, const char*), void* user) noexcept;

struct Status {
    bool installed{}, owner{};
    uint32_t backend{}, choices{}, requested_mode{}, effective_mode{}, generated{}, reason{};
    uint64_t source_frames{}, level_frames{}, tagged{}, fed{}, presented{}, unpresented{}, runtime_tagged{},
        vendor_presented{}, captures{};
};
Status status() noexcept;
}
