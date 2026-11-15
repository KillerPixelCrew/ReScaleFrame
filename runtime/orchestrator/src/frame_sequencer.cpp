// SPDX-License-Identifier: GPL-3.0-only
// Unwired scaffolding: no production caller yet. Kept for the planned ABI 2 frame-callback
// work (see docs/representation-plan.md). Do not delete as dead code.
#include <rescaleframe/frame_sequencer.h>
#include <mutex>
#include <new>
struct rsf_frame_sequencer {
    std::mutex guard;
    uint32_t capacity = 0;
    uint64_t last_begin = 0;
    rsf_sequence_state slots[64]{};
};
namespace {
uint32_t ordinal(rsf_latency_marker marker)
{
    return marker == RSF_LATENCY_INPUT_SAMPLE ? 0 : marker <= RSF_LATENCY_PRESENT_END ? marker + 1 : 7;
}
}
extern "C" rsf_backend_result rsf_frame_sequencer_create(uint32_t capacity, rsf_frame_sequencer** out)
{
    if (!out || !capacity || capacity > 64) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    *out = new (std::nothrow) rsf_frame_sequencer;
    if (!*out) return RSF_BACKEND_ERROR_INIT_FAILED;
    (*out)->capacity = capacity;
    return RSF_BACKEND_OK;
}
extern "C" rsf_backend_result rsf_frame_sequencer_begin(rsf_frame_sequencer* ledger, uint64_t id)
{
    if (!ledger || !id) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    std::lock_guard<std::mutex> lock(ledger->guard);
    auto& slot = ledger->slots[id % ledger->capacity];
    if (id <= ledger->last_begin || slot.frame_id) return RSF_BACKEND_ERROR_NOT_READY;
    slot = {}; slot.struct_size = sizeof(slot); slot.frame_id = id;
    ledger->last_begin = id;
    return RSF_BACKEND_OK;
}
extern "C" rsf_backend_result rsf_frame_sequencer_begin_rendered(rsf_frame_sequencer* ledger,
    uint64_t id, uint64_t source)
{
    if (!source) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    const auto result = rsf_frame_sequencer_begin(ledger, id);
    if (result != RSF_BACKEND_OK) return result;
    std::lock_guard<std::mutex> lock(ledger->guard);
    auto& slot = ledger->slots[id % ledger->capacity];
    slot.source_provenance = RSF_FRAME_SOURCE_RENDERED; slot.source_qpc = source;
    return RSF_BACKEND_OK;
}
extern "C" rsf_backend_result rsf_frame_sequencer_marker(rsf_frame_sequencer* ledger, uint64_t id,
    rsf_latency_marker marker, uint64_t timestamp)
{
    const uint32_t index = ordinal(marker);
    if (!ledger || !id || !timestamp || index > 6) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    std::lock_guard<std::mutex> lock(ledger->guard);
    auto& slot = ledger->slots[id % ledger->capacity];
    if (slot.frame_id != id || slot.failed) return RSF_BACKEND_ERROR_NOT_READY;
    const bool rendered = slot.source_provenance == RSF_FRAME_SOURCE_RENDERED;
    const uint32_t expected = rendered ? (((1u << index) - 1) & ~7u) : (1u << index) - 1;
    if ((rendered && (index < 3 || timestamp < slot.source_qpc)) || slot.marker_mask != expected ||
        (index && timestamp < slot.timestamps[index - 1])) {
        slot.failed = 1;
        return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    }
    slot.timestamps[index] = timestamp;
    slot.marker_mask |= 1u << index;
    return RSF_BACKEND_OK;
}
extern "C" rsf_backend_result rsf_frame_sequencer_get(rsf_frame_sequencer* ledger, uint64_t id,
    rsf_sequence_state* state)
{
    if (!ledger || !id || !state || state->struct_size < sizeof(*state)) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    std::lock_guard<std::mutex> lock(ledger->guard);
    const auto& slot = ledger->slots[id % ledger->capacity];
    if (slot.frame_id != id) return RSF_BACKEND_ERROR_NOT_READY;
    *state = slot;
    return RSF_BACKEND_OK;
}
extern "C" rsf_backend_result rsf_frame_sequencer_finish(rsf_frame_sequencer* ledger, uint64_t id)
{
    if (!ledger || !id) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    std::lock_guard<std::mutex> lock(ledger->guard);
    auto& slot = ledger->slots[id % ledger->capacity];
    if (slot.frame_id != id) return RSF_BACKEND_ERROR_NOT_READY;
    const bool complete = slot.marker_mask == (slot.source_provenance == RSF_FRAME_SOURCE_RENDERED ? 120u : 127u) && !slot.failed;
    slot = {};
    return complete ? RSF_BACKEND_OK : RSF_BACKEND_ERROR_NOT_READY;
}
extern "C" void rsf_frame_sequencer_destroy(rsf_frame_sequencer* ledger) { delete ledger; }
