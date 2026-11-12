// SPDX-License-Identifier: GPL-3.0-only
#pragma once
// SR Stages 4 and 5 (docs/research/anomaly-sr-seam.md section 12): the adapter's use of the Runtime's shared D3D11 SR
// pipeline through rsf_get_d3d11_sr_api. The adapter borrows the carrier-loaded Runtime and never loads a second one.
// What stays here: when to start, which settings apply at which boundary, the reset policy, the pass contents, the
// counters. What the Runtime owns: Streamline/FidelityFX/XeSS, the motion decode and resolve, the backends.
//
// Threads. request() and overlay_report() run on any thread (the overlay's Present thread, settings_apply callers).
// Everything else runs on the engine render thread, which owns the immediate context the pipeline uses.
#include "sr_pass.h"
#include <rescaleframe/d3d11_sr_api.h>
#include <rescaleframe/overlay_api.h>
#include <windows.h>
#include <array>
#include <cstdint>
#include <string>

struct ID3D11Device;
struct ID3D11DeviceContext;

namespace rsf::anomaly::sr {
enum class ApiStatus : uint8_t { Bound, Missing, Refused };
// Validates a table from `query` exactly like overlay::bind_api: size, version, pipeline ABI and every entry. On
// failure `out` is untouched and `reason` names the refusal. Null `query` is Missing.
ApiStatus bind_sr_api(rsf_get_d3d11_sr_api_fn query, rsf_d3d11_sr_api& out, const char** reason) noexcept;

struct TemporalOptions {
    uint32_t stage{};                    // 4: inputs only (bilinear output), 5: temporal SR
    bool exposure_auto{};                // SRExposure=auto: no exposure texture, backend auto exposure
    bool first_person_zero{true};        // SRFirstPersonZero: unwritten first-person pixels get zero motion
    CutRule cut{};                       // SRCutDistance (m), SRCutAngle (degrees)
    std::string streamline, fidelityfx, xess;   // UTF-8 vendor runtime directories
    bool require_signature{};            // SRRequireSignature: refuse an unsigned sl.interposer.dll
    SrSettings initial{};                // SR, SRBackend, SRQuality, DLSSPreset from the INI before the overlay starts
};

// Why a Stage 5 frame did not reach a successful evaluation.
enum class Refusal : uint8_t {
    NotRunning,   // the pipeline is not started (start refused, export missing, stopped)
    Disabled,     // SR=0 in the shared settings (the world window stays closed then; counted for completeness)
    Prepare,      // the prepare pass could not run
    Pass,         // build_pass refused (see pass_errors)
    Pipeline,     // the Runtime refused or the backend failed (see results)
    Reinsert,     // evaluated, but the output pass failed: the bilinear fallback was drawn
    Count
};
const char* refusal_name(Refusal) noexcept;

struct TemporalCounters {
    std::array<uint64_t, 8> evaluated{};               // by RSF_SR_* backend id
    std::array<uint64_t, static_cast<std::size_t>(Refusal::Count)> refused{};
    std::array<uint64_t, 6> pass_errors{};             // by PassError
    std::array<uint64_t, 12> results{};                // by -rsf_dlss_pipeline_result (1..10), 11 = other
    std::array<uint64_t, static_cast<std::size_t>(ResetReason::Count)> resets{};
    uint64_t starts{}, start_failures{}, stops{}, selects{}, select_failures{}, qualities{}, quality_failures{}, presets{};
    uint64_t resizes{}, resize_failures{}, bilinear{}, motion_missing{}, prepared{};
};

struct TemporalStatus {
    bool bound{}, running{}, enabled{};
    rsf_dlss_pipeline_status pipeline{};
    SrSettings applied{}, requested{};
    int32_t start_result{}, select_result{}, quality_result{};
    bool last_evaluated{};
    uint32_t last_backend{}, last_render_width{}, last_render_height{}, last_output_width{}, last_output_height{};
    float jitter[2]{};
    bool motion_present{}, exposure_present{};
    std::string reason;
};

class TemporalDriver {
public:
    void configure(const TemporalOptions&, rsf_game_log_fn log, void* user) noexcept;
    const TemporalOptions& options() const noexcept { return options_; }
    // Install time: query the Runtime's table. Stage 4 does not need it; Stage 5 refuses SR without it.
    bool bind(HMODULE runtime) noexcept;
    // Test seam: a table supplied directly (validated the same way).
    bool bind_table(const rsf_d3d11_sr_api& table) noexcept;
    // Any thread: the shared settings the overlay or the INI asks for. Applied at the next boundary().
    void request(const SrSettings&) noexcept;
    // Render thread, after the frame's last SR use and before Present (IRenderDeviceRender::End): start the pipeline
    // on `device`, follow a device or output change, apply requested settings. This is the drained boundary: no SR
    // input of this frame is still to be submitted.
    void boundary(ID3D11Device* device, uint32_t output_width, uint32_t output_height) noexcept;
    // Render thread, H1: whether the world window should open for SR, and the render size the active backend plans
    // for this output (false when the pipeline is not running or plans for another output).
    bool enabled() const noexcept;
    bool planned_render(uint32_t output_width, uint32_t output_height, uint32_t& render_width, uint32_t& render_height) const noexcept;
    bool running() const noexcept { return running_; }
    // The output texture format the pipeline copies into PassInputs::color_output (DXGI_FORMAT), 0 when stopped.
    uint32_t output_format() const noexcept;
    // Render thread, at the seam: the reset decision, the pass and the evaluation. True when `in.color_output` holds
    // this frame's reconstruction. Every false return is counted with its reason.
    bool evaluate(ID3D11DeviceContext*, PassInputs& in, const ResetFacts& facts) noexcept;
    // Render thread: the pipeline writes the next evaluated frame's backend inputs and output under `prefix_utf8`
    // (_input, _output, _motion, _motion_decoded, _motion_submitted, _depth, _exposure, _frame.json).
    bool request_dump(const std::string& prefix_utf8) noexcept;
    void count(Refusal) noexcept;
    void count_bilinear() noexcept;
    void count_prepared(bool motion_missing) noexcept;
    // Quiesce or teardown: stop the pipeline (render thread or with rendering stopped).
    void stop() noexcept;
    // Any thread.
    bool overlay_report(rsf_overlay_sr_report&) const noexcept;
    TemporalStatus status() const noexcept;
    TemporalCounters counters() const noexcept;
    // Two log lines for the observer's periodic report (deltas since the last call, and the pipeline state).
    void report(void (*emit)(void*, const char*), void* user) noexcept;

private:
    void say(const char*) const noexcept;
    void start(ID3D11Device*, uint32_t output_width, uint32_t output_height) noexcept;
    void apply(const SrSettings& wanted) noexcept;
    void refresh() noexcept;

    TemporalOptions options_{};
    rsf_game_log_fn log_{};
    void* log_user_{};
    rsf_d3d11_sr_api api_{};
    bool bound_{};
    // Render thread.
    bool running_{};
    ID3D11Device* device_{};             // compared only; the pipeline holds its own reference
    ID3D11Device* failed_device_{};      // start refused on this device for these settings
    SrSettings failed_settings_{};
    SrSettings applied_{};
    SrSettings refused_request_{};      // a request apply() could not fully satisfy: not retried until it changes
    bool have_refused_{};
    bool settings_changed_{}, device_changed_{}, first_{true};
    uint64_t session_{};
    // Shared with any thread.
    mutable SRWLOCK lock_ = SRWLOCK_INIT;
    SrSettings requested_{};
    bool have_request_{};
    TemporalStatus status_{};
    TemporalCounters counters_{}, reported_{};
};
}
