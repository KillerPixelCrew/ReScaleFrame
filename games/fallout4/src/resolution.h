// SPDX-License-Identifier: GPL-3.0-only
#pragma once
#include <rescaleframe/game_renderer.h>
namespace rsf::fallout4 {
struct NativeDepthSnapshot;
enum class ResolutionStage : unsigned { pre_sr, output_ready, evaluated, effects_return, hudless, ui_return, ui_restored, source_retired };
// Bounded same-source receipts only; graphics state is queried on its native owner.
void report_resolution_state(ResolutionStage, const rsf_game_render_pass&, void* context,
    bool published, int32_t result, void (*report)(const char*)) noexcept;
bool verify_resolution() noexcept;
bool native_resolution_inputs(const rsf_game_render_pass&) noexcept;
bool configure_native_resolution(const rsf_game_host_services&, const rsf_game_render_pass&, void* context) noexcept;
// Called only after the complete original guarded native DR writer returns.
bool native_resolution_write() noexcept;
// Borrow native full-output state for the synchronous transaction. On refusal,
// restore it before native consumers; successful output drains at RenderUI end.
bool begin_output_resolution(const NativeDepthSnapshot&, const rsf_game_render_pass&) noexcept;
bool native_viewport_write(bool enabled) noexcept;
bool end_output_resolution() noexcept;
bool finish_resolution_source() noexcept;
bool resolution_busy() noexcept;
bool output_resolution_busy() noexcept;
bool resolution_refused() noexcept;
}
