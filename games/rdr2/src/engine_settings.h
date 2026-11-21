// SPDX-License-Identifier: MIT
#pragma once
#include <rescaleframe/game_renderer.h>
// Owned by the Rust crate rsf-rdr2 (games/rdr2/src/engine_settings.rs). rsf_rdr2_asi_attach
// installs it after exact-image decryption, before native settings load. Diagnostics are
// announced later, outside the ASI loader lock.
extern "C" {
void rsf_rdr2_report_engine_settings(rsf_game_log_fn, void*) noexcept;
// Called only by the ScriptHook game fiber, never the presentation/job workers.
void rsf_rdr2_engine_quality_tick(uint32_t quality) noexcept;
}
