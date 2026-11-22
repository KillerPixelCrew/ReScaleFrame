// SPDX-License-Identifier: MIT
#pragma once
#include <rescaleframe/game_api.h>
#include <cstdint>
namespace rsf::skyrim {
bool cpu_prepare();
bool cpu_bind(const rsf_game_host_services&);
bool cpu_start();
void cpu_quiesce();
// Disable and drain while retaining prepared services for retry.
bool cpu_disable();
// Release services only after every adapter stop refusal check has passed.
bool cpu_stop();
bool cpu_busy();
uint64_t cpu_source();
bool cpu_render_identity(rsf_game_render_pass&);
// Bounded producer receipts at actual FG_INPUTS/WINDOW emission, with no policy changes.
void cpu_render_diagnostic(const char* stage, const rsf_game_render_pass&);
// Exact native World source and gameplay classification only. This permits the
// host's render eligibility check; prospective input ownership is tracked separately.
bool cpu_rendered_source_eligible() noexcept;
// Read only the native event headers while the dispatch callback owns the list.
uint32_t cpu_input_kinds(const void* list) noexcept;
struct CpuInputConsumption {
    uint64_t source_frame_id{}, poll_generation{};
    uint32_t camera_updates{};
    uint32_t movement_calls{}, player_update_calls{};
};
// Mapped camera consumption plus movement/player-update call envelopes only.
// A call may take a native early return. These counts do not establish complete
// simulation coverage, pre-input pacing or FG admission.
bool cpu_input_consumption(CpuInputConsumption&) noexcept;
struct CpuPollProvenance {
    uint64_t session_id{}, native_job_generation{}, submitting_source_frame_id{}, poll_generation{};
};
// Available only inside the real poll reached through the named mode1 callback.
// The submitting source is the overlapping render, not a consuming-frame claim.
bool cpu_poll_provenance(CpuPollProvenance&) noexcept;
}
