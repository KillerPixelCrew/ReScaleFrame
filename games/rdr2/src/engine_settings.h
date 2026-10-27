// SPDX-License-Identifier: GPL-3.0-only
#pragma once
#include <rescaleframe/game_renderer.h>
// Install after exact-image decryption, before native settings load. Diagnostics are
// announced later, outside the ASI loader lock.
bool rsf_rdr2_own_engine_settings(const wchar_t* configuration) noexcept;
void rsf_rdr2_report_engine_settings(rsf_game_log_fn, void*) noexcept;
// Called only by the ScriptHook game fiber, never the presentation/job workers.
void rsf_rdr2_engine_quality_tick(uint32_t quality) noexcept;
