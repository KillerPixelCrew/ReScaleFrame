// SPDX-License-Identifier: GPL-3.0-only
#include <rescaleframe/native_cpu.h>
#include <array>
#include <atomic>
#include <mutex>
#include <windows.h>
#include <cstdio>
#include <cstring>
#include <cwchar>
#include <iterator>
namespace {
std::atomic<bool> rendered_sources{false};
std::mutex guard;
std::array<rsf_native_cpu_frame, 128> frames{};
uint64_t current_session = 0, last_begin = 0;
rsf_game_cpu_event_fn consumer = nullptr;
void* consumer_user = nullptr;
rsf_native_cpu_counters totals{sizeof(totals)};
uint32_t ping_message{};
std::array<rsf_game_cpu_event,16> pending_messages{};
uint32_t next_message{};
std::mutex lease_guard;
HANDLE pacing_lease{};
uint32_t pacing_client{};
// Prospective sources (CPU event version 2). A reservation is a retained fact about an actual
// input poll whose consuming source is not yet known. Valid binding promotes only the real
// poll timestamp; the runtime sink owns pacing and markers. Live entries are never evicted.
std::array<rsf_native_cpu_reservation, 16> reservations{};
uint64_t highest_reserved = 0, session_frequency = 0;
rsf_native_cpu_reservation_counters reservation_totals{sizeof(reservation_totals)};
std::atomic<uint64_t> reservation_rejected{0};
// Actual application Present outcomes of the current session, independent of FRAME_END and
// binding. Guarded by `guard`; the first outcome per source stands.
struct PresentOutcome { uint64_t source{}; int32_t state{}; };
std::array<PresentOutcome, 64> present_outcomes{};
uint32_t next_present_outcome = 0;
uint64_t presented_through = 0;
bool record_present(uint64_t session, uint64_t source, int32_t state) {
    if (!session || session != current_session || !source) return false;
    for (const auto& entry : present_outcomes) if (entry.source == source) return false;
    present_outcomes[next_present_outcome++ % present_outcomes.size()] = {source, state};
    if (state == RSF_NATIVE_CPU_DEPENDENCY_PRESENTED && source > presented_through) presented_through = source;
    return true;
}
int32_t present_state(uint64_t source) {
    if (!source) return RSF_NATIVE_CPU_DEPENDENCY_NONE;
    for (const auto& entry : present_outcomes) if (entry.source == source) return entry.state;
    return presented_through > source ? RSF_NATIVE_CPU_DEPENDENCY_SUPERSEDED : RSF_NATIVE_CPU_DEPENDENCY_PENDING;
}
bool reservation_live(uint32_t state) {
    return state == RSF_NATIVE_CPU_RESERVATION_OPEN || state == RSF_NATIVE_CPU_RESERVATION_SAMPLED;
}
bool reservation_owned(uint32_t state) {
    return state == RSF_NATIVE_CPU_RESERVATION_ADOPTED || state == RSF_NATIVE_CPU_RESERVATION_BOUND;
}
rsf_native_cpu_reservation* find_reservation(uint64_t session, uint64_t source) {
    if (!session || !source) return nullptr;
    for (auto& entry : reservations)
        if (entry.state && entry.session_id == session && entry.source_frame_id == source) return &entry;
    return nullptr;
}
void abort_reservation(rsf_native_cpu_reservation& entry, uint32_t reason) {
    entry.state = RSF_NATIVE_CPU_RESERVATION_ABORTED; entry.abort_reason = reason; ++reservation_totals.aborted;
}
// FRAME_BEGIN under `guard`. Adopts a sampled reservation of this identity and withdraws the
// others. The frame itself is never refused here: the reporting callback cannot tell an adapter
// that a reservation was refused, so a frame with a withdrawn or still-polling identity begins
// as an ordinary frame without input ownership rather than losing the engine's CPU frame.
void reservation_begin(const rsf_game_cpu_event& event) {
    for (auto& entry : reservations) {
        if (!entry.state || entry.session_id != event.session_id) continue;
        if (entry.source_frame_id == event.source_frame_id) {
            if (entry.state == RSF_NATIVE_CPU_RESERVATION_SAMPLED && entry.qpc_frequency == event.qpc_frequency &&
                event.timestamp_qpc >= entry.sampled_qpc) {
                entry.state = RSF_NATIVE_CPU_RESERVATION_ADOPTED; entry.frame_begin_qpc = event.timestamp_qpc;
                ++reservation_totals.adopted;
            } else if (reservation_live(entry.state)) {
                abort_reservation(entry, RSF_GAME_CPU_ABORT_OVERLAP); // poll overlaps its consumer
            }
        } else if (reservation_live(entry.state)) {
            abort_reservation(entry, RSF_GAME_CPU_ABORT_SUPERSEDED);
        }
    }
}
// Any CPU closure of a frame, under `guard`.
void reservation_end(uint64_t session, uint64_t source) {
    auto* entry = find_reservation(session, source);
    if (!entry) return;
    if (entry->state == RSF_NATIVE_CPU_RESERVATION_ADOPTED) { entry->state = RSF_NATIVE_CPU_RESERVATION_UNBOUND; ++reservation_totals.unbound; }
    else if (entry->state == RSF_NATIVE_CPU_RESERVATION_BOUND) { entry->state = RSF_NATIVE_CPU_RESERVATION_CLOSED; ++reservation_totals.closed; }
}
int refuse_source() { reservation_rejected.fetch_add(1, std::memory_order_relaxed); return 0; }
void clear_facts() {
    frames = {}; pending_messages={}; next_message=0; totals = {sizeof(totals)}; ping_message=0; current_session = last_begin = 0; consumer = nullptr; consumer_user = nullptr;
    reservations = {}; highest_reserved = session_frequency = 0; reservation_totals = {sizeof(reservation_totals)};
    reservation_rejected.store(0, std::memory_order_relaxed);
    present_outcomes = {}; next_present_outcome = 0; presented_through = 0;
}
int source_event(const rsf_game_cpu_event* incoming) {
    // Validate the declared size before reading any extension byte. Appended fields beyond a
    // shorter packet stay zero.
    if (incoming->event_version != RSF_GAME_CPU_EVENT_VERSION_SOURCE ||
        incoming->struct_size < RSF_GAME_CPU_SOURCE_EVENT_SIZE_1 ||
        (incoming->struct_size > RSF_GAME_CPU_SOURCE_EVENT_SIZE_1 && incoming->struct_size < sizeof(rsf_game_cpu_source_event)))
        return refuse_source();
    rsf_game_cpu_source_event event{};
    std::memcpy(&event, incoming, incoming->struct_size < sizeof(event) ? incoming->struct_size : sizeof(event));
    const auto& fact = event.header;
    const uint32_t allowed = fact.stage == RSF_GAME_CPU_SOURCE_RESERVE ? RSF_GAME_CPU_SOURCE_OVERLAPS_PREDECESSOR :
        fact.stage == RSF_GAME_CPU_SOURCE_SAMPLED ? RSF_GAME_CPU_SOURCE_PRODUCED : 0u;
    if (!fact.session_id || !fact.source_frame_id || !fact.timestamp_qpc || !fact.qpc_frequency ||
        (fact.flags & ~allowed) || fact.message_id || fact.render_expected || (fact.input_kind & ~7u) ||
        (fact.input_kind && fact.stage != RSF_GAME_CPU_SOURCE_SAMPLED) || event.reserved ||
        (event.reason && fact.stage != RSF_GAME_CPU_SOURCE_ABORT) ||
        (event.predecessor_source_id && fact.stage != RSF_GAME_CPU_SOURCE_RESERVE) ||
        (event.poll_begin_qpc && (fact.stage != RSF_GAME_CPU_SOURCE_SAMPLED || event.poll_begin_qpc > fact.timestamp_qpc)))
        return refuse_source();
    bool refused = false;
    {
        std::lock_guard lock(guard);
        if (!current_session || current_session != fact.session_id || fact.qpc_frequency != session_frequency)
            return refuse_source();
        auto* entry = find_reservation(fact.session_id, fact.source_frame_id);
        if (fact.stage == RSF_GAME_CPU_SOURCE_RESERVE) {
            if (fact.source_frame_id <= last_begin || fact.source_frame_id <= highest_reserved) return refuse_source();
            if (const auto predecessor = event.predecessor_source_id) {
                const auto& owner = frames[predecessor % frames.size()];
                if (predecessor > last_begin || owner.source_frame_id != predecessor || owner.session_id != fact.session_id)
                    return refuse_source();
            }
            highest_reserved = fact.source_frame_id;
            bool overlap = false;
            for (auto& other : reservations) {
                if (other.session_id != fact.session_id || !reservation_live(other.state)) continue;
                // A sampled, unadopted poll is replaced by this newer poll's control state.
                // A poll still in flight means two native polls overlap: neither owns its state.
                overlap |= other.state == RSF_NATIVE_CPU_RESERVATION_OPEN;
                abort_reservation(other, other.state == RSF_NATIVE_CPU_RESERVATION_OPEN ?
                    RSF_GAME_CPU_ABORT_OVERLAP : RSF_GAME_CPU_ABORT_INTERVENING_POLL);
            }
            rsf_native_cpu_reservation* slot = nullptr;
            if (!overlap) for (auto& candidate : reservations) {
                if (reservation_live(candidate.state) || reservation_owned(candidate.state)) continue;
                if (!slot || !candidate.state || (slot->state && candidate.source_frame_id < slot->source_frame_id)) slot = &candidate;
                if (!slot->state) break;
            }
            if (slot) {
                *slot = {}; slot->struct_size = sizeof(*slot); slot->state = RSF_NATIVE_CPU_RESERVATION_OPEN;
                slot->session_id = fact.session_id; slot->source_frame_id = fact.source_frame_id;
                slot->predecessor_source_id = event.predecessor_source_id; slot->native_generation = event.native_generation;
                slot->qpc_frequency = fact.qpc_frequency; slot->reserve_qpc = fact.timestamp_qpc; slot->flags = fact.flags;
                ++reservation_totals.reserved;
            } else refused = true; // overlapping polls, or every entry still owned by a live source
        } else if (!entry || entry->qpc_frequency != fact.qpc_frequency) {
            return refuse_source();
        } else if (fact.stage == RSF_GAME_CPU_SOURCE_SAMPLED) {
            if (entry->state != RSF_NATIVE_CPU_RESERVATION_OPEN || fact.timestamp_qpc < entry->reserve_qpc ||
                event.native_generation != entry->native_generation ||
                (event.poll_begin_qpc && event.poll_begin_qpc < entry->reserve_qpc)) return refuse_source();
            entry->state = RSF_NATIVE_CPU_RESERVATION_SAMPLED; entry->sampled_qpc = fact.timestamp_qpc;
            entry->poll_begin_qpc = event.poll_begin_qpc;
            entry->input_kind = fact.input_kind; entry->flags |= fact.flags; ++reservation_totals.sampled;
        } else if (fact.stage == RSF_GAME_CPU_SOURCE_BIND) {
            auto& frame = frames[fact.source_frame_id % frames.size()];
            if (entry->state != RSF_NATIVE_CPU_RESERVATION_ADOPTED || !(entry->flags & RSF_GAME_CPU_SOURCE_PRODUCED) ||
                frame.source_frame_id != fact.source_frame_id || frame.session_id != fact.session_id ||
                frame.ended || frame.failed || fact.timestamp_qpc < entry->frame_begin_qpc ||
                fact.timestamp_qpc < entry->sampled_qpc) return refuse_source();
            if (!rendered_sources.load() && entry->poll_begin_qpc && frame.stage_mask != 1u) return refuse_source();
            entry->state = RSF_NATIVE_CPU_RESERVATION_BOUND; entry->bind_qpc = fact.timestamp_qpc;
            if (!rendered_sources.load() && entry->poll_begin_qpc) {
                // This real poll predates FRAME_BEGIN. Promote only after native consumption
                // is bound; no v1 input event or simulation marker is invented at RESERVE.
                frame.timestamps_qpc[RSF_GAME_CPU_INPUT_SAMPLE] = entry->poll_begin_qpc;
                frame.stage_mask |= 1u << RSF_GAME_CPU_INPUT_SAMPLE;
            }
            ++reservation_totals.bound;
        } else {
            if (!reservation_live(entry->state) && !reservation_owned(entry->state)) return refuse_source();
            abort_reservation(*entry, event.reason);
        }
    }
    return refused ? refuse_source() : 1;
}
}
extern "C" RSF_RUNTIME_API int rsf_native_latency_claim(uint32_t client) {
    if (client!=1 && client!=2) return 0;
    std::lock_guard lock(lease_guard);
    if (pacing_lease) return pacing_client==client;
    wchar_t name[96]{};std::swprintf(name,std::size(name),L"Local\\ReScaleFrame.LatencyOwner.%lu",GetCurrentProcessId());
    SetLastError(ERROR_SUCCESS);
    const auto event=CreateEventW(nullptr,TRUE,FALSE,name);
    const auto result=GetLastError();
    if (!event) return 0;
    if (result==ERROR_ALREADY_EXISTS) {CloseHandle(event);return 0;}
    pacing_lease=event;pacing_client=client;return 1;
}
extern "C" RSF_RUNTIME_API int rsf_native_latency_has_claim(uint32_t client) {
    std::lock_guard lock(lease_guard);return pacing_lease && pacing_client==client;
}
extern "C" RSF_RUNTIME_API void rsf_native_latency_release_claim(uint32_t client) {
    std::lock_guard lock(lease_guard);
    if (pacing_lease && pacing_client==client) {CloseHandle(pacing_lease);pacing_lease=nullptr;pacing_client=0;}
}
extern "C" RSF_RUNTIME_API void rsf_native_cpu_rendered_source_set(uint32_t enabled) {
    rendered_sources.store(enabled != 0);
}
extern "C" RSF_RUNTIME_API int rsf_native_cpu_dispatch(const rsf_game_cpu_event* incoming,
    rsf_game_cpu_event_fn supplied_sink, void* supplied_user)
{
    // Source facts are stored before the synchronous runtime callback. Rendered-only
    // hosts retain their observation-only behavior; input hosts may pace at RESERVE.
    if (incoming && incoming->struct_size >= sizeof(*incoming) &&
        incoming->stage >= RSF_GAME_CPU_SOURCE_RESERVE && incoming->stage <= RSF_GAME_CPU_SOURCE_ABORT) {
        const auto accepted = source_event(incoming);
        rsf_game_cpu_event_fn source_sink = nullptr; void* source_user = nullptr;
        if (accepted && !rendered_sources.load()) {
            std::lock_guard lock(guard);
            source_sink = supplied_sink ? supplied_sink : consumer;
            source_user = supplied_sink ? supplied_user : consumer_user;
        }
        if (source_sink) source_sink(source_user, incoming);
        return accepted;
    }
    if (!incoming || incoming->struct_size < sizeof(*incoming) || incoming->event_version > RSF_GAME_CPU_EVENT_VERSION ||
        incoming->flags & ~(RSF_GAME_CPU_INPUT_KEYED | RSF_GAME_CPU_INPUT_SUBSTITUTE | RSF_GAME_CPU_WINDOW_MESSAGE | RSF_GAME_CPU_RENDER_SKIPPED | RSF_GAME_CPU_INPUT_UNFRAMED | RSF_GAME_CPU_DERIVED_BOUNDARY)) return 0;
    const bool rendered = rendered_sources.load();
    if (rendered && (incoming->stage == RSF_GAME_CPU_INPUT_SAMPLE || incoming->stage == RSF_GAME_CPU_PACING)) return 0;
    auto copy = *incoming;
    const auto* event = &copy;
    const bool window_message = event->event_version == RSF_GAME_CPU_EVENT_VERSION && (event->flags & RSF_GAME_CPU_WINDOW_MESSAGE);
    const bool unframed=event->event_version==RSF_GAME_CPU_EVENT_VERSION && (event->flags & RSF_GAME_CPU_INPUT_UNFRAMED);
    if ((window_message || unframed) && event->stage==RSF_GAME_CPU_INPUT_EVENT && !event->source_frame_id) {
        if (!event->session_id || !event->timestamp_qpc || !event->qpc_frequency) return 0;
        if (event->input_kind & ~7u) return 0;
        if (!event->input_kind && (event->message_id<0xc000 || event->message_id>0xffff)) return 1;
        if (event->input_kind && !unframed) return 0;
        std::lock_guard lock(guard);
        if (current_session && current_session != event->session_id) return 0;
        pending_messages[next_message++ % pending_messages.size()] = *event;
        return 1;
    }
    const bool facts = event->event_version == RSF_GAME_CPU_EVENT_VERSION && (event->flags & RSF_GAME_CPU_INPUT_KEYED);
    if (facts && event->stage==RSF_GAME_CPU_FRAME_BEGIN) {
        rsf_native_cpu_frame previous{};
        {
            std::lock_guard lock(guard);
            if (current_session==event->session_id && last_begin && last_begin<event->source_frame_id)
                previous=frames[last_begin%frames.size()];
        }
        if ((previous.stage_mask==15u || (rendered && (previous.stage_mask & 9u)==9u)) && !previous.ended) {
            auto end=*event; end.stage=RSF_GAME_CPU_FRAME_END; end.source_frame_id=previous.source_frame_id;
            end.flags|=RSF_GAME_CPU_DERIVED_BOUNDARY; end.render_expected=0;
            if (!rsf_native_cpu_dispatch(&end,supplied_sink,supplied_user)) return 0;
        }
    }
    // A renderer can run without an engine input callback (loading/first frame). Normalize that absence centrally,
    // label it, and skip a late pacing sleep. Adapters report only the simulation end they actually observed.
    if (!rendered && facts && event->stage == RSF_GAME_CPU_SIMULATION_END) {
        rsf_native_cpu_frame current{}; current.struct_size = sizeof(current);
        if (!rsf_native_cpu_read(event->session_id, event->source_frame_id, &current)) return 0;
        auto substitute = *event; substitute.flags |= RSF_GAME_CPU_INPUT_SUBSTITUTE;
        if ((current.stage_mask & 1u) && !(current.stage_mask & 2u)) {
            substitute.stage = RSF_GAME_CPU_INPUT_SAMPLE;
            if (!rsf_native_cpu_dispatch(&substitute, supplied_sink, supplied_user)) return 0;
        }
        if ((current.stage_mask & 1u) && !(current.stage_mask & 4u)) {
            substitute.stage = RSF_GAME_CPU_SIMULATION_BEGIN;
            if (!rsf_native_cpu_dispatch(&substitute, supplied_sink, supplied_user)) return 0;
        }
    }
    if (!event || event->struct_size < sizeof(*event) || event->stage > RSF_GAME_CPU_PACING ||
        !event->session_id || !event->source_frame_id || !event->timestamp_qpc || !event->qpc_frequency) return 0;
    rsf_game_cpu_event_fn sink = nullptr; void* user = nullptr;
    {
        std::lock_guard<std::mutex> lock(guard);
        if (!current_session && event->stage == RSF_GAME_CPU_FRAME_BEGIN) {
            current_session = event->session_id; session_frequency = event->qpc_frequency;
        }
        if (current_session != event->session_id) return 0;
        auto& frame = frames[event->source_frame_id % frames.size()];
        if (event->stage == RSF_GAME_CPU_FRAME_BEGIN) {
            if (event->source_frame_id <= last_begin || (frame.source_frame_id && !frame.ended)) return 0;
            reservation_begin(*event);
            frame = {}; frame.struct_size = sizeof(frame); frame.session_id = event->session_id;
            frame.source_frame_id = event->source_frame_id; frame.qpc_frequency = event->qpc_frequency;
            last_begin = event->source_frame_id; frame.flags=event->flags; ++totals.frames;
        } else {
            if (frame.source_frame_id != event->source_frame_id || frame.session_id != event->session_id ||
                frame.ended) return 0;
            if (frame.failed) {
                if (event->stage == RSF_GAME_CPU_FRAME_END) { frame.ended = 1; reservation_end(event->session_id, event->source_frame_id); }
                return 0;
            }
        }
        if (facts && event->stage <= RSF_GAME_CPU_FRAME_END && event->stage != RSF_GAME_CPU_FRAME_BEGIN &&
            (frame.stage_mask & (1u << event->stage))) {
            if (event->stage==RSF_GAME_CPU_SIMULATION_END && !(event->flags & RSF_GAME_CPU_DERIVED_BOUNDARY) &&
                (frame.flags & RSF_GAME_CPU_DERIVED_BOUNDARY)) {
                frame.timestamps_qpc[event->stage]=event->timestamp_qpc;
                frame.flags &= ~RSF_GAME_CPU_DERIVED_BOUNDARY;
            }
            ++totals.duplicates; return 1;
        }
        if (event->stage == RSF_GAME_CPU_PACING) {
            if (facts) return 1; // version 1 pacing is selected at INPUT_SAMPLE, not at a guessed frame-open boundary
            if (frame.stage_mask != 1u || frame.pacing_qpc || frame.qpc_frequency != event->qpc_frequency ||
                event->timestamp_qpc < frame.timestamps_qpc[0]) return 0;
            frame.pacing_qpc = event->timestamp_qpc;
            sink = supplied_sink ? supplied_sink : consumer; user = supplied_sink ? supplied_user : consumer_user;
        } else if (event->stage == RSF_GAME_CPU_INPUT_EVENT) {
            // Pending window/unframed observations are not proof of consumption by this
            // prospective source. Do not attribute input or PCL counts to its future token.
            if (!rendered && (unframed || window_message) && find_reservation(event->session_id, event->source_frame_id)) return 1;
            if (event->input_kind & ~7u || (!event->input_kind && event->message_id < 0xc000u && !window_message) ||
                event->qpc_frequency != frame.qpc_frequency || (!unframed && event->timestamp_qpc < frame.timestamps_qpc[0]) ||
                (unframed && frame.timestamps_qpc[0]>event->timestamp_qpc &&
                 frame.timestamps_qpc[0]-event->timestamp_qpc>event->qpc_frequency/4)) return 0;
            if (window_message && copy.message_id != ping_message && !copy.input_kind) return 1;
            if (unframed && copy.input_kind) ++totals.unframed_inputs;
            if (facts) {
                const auto previous=copy.input_kind; copy.input_kind &= ~frame.input_mask;
                if (previous != copy.input_kind) ++totals.duplicates;
            }
            if (copy.message_id && copy.message_id == ping_message) {
                if (frame.pings) { copy.message_id=0; ++totals.duplicates; }
                else { frame.pings=1; ++totals.pings; }
            }
            if (!copy.input_kind && !copy.message_id) return 1;
            for (uint32_t i = 0; i < 3; ++i) if (event->input_kind & (1u << i)) {
                frame.input_mask |= 1u << i;
                ++frame.input_events[i]; ++totals.input_events[i]; frame.input_qpc[i] = event->timestamp_qpc;
            }
            sink = supplied_sink ? supplied_sink : consumer; user = supplied_sink ? supplied_user : consumer_user;
        } else {
        const uint32_t expected = (1u << event->stage) - 1u;
        // Idle/loading frames may close before input or simulation; they remain explicit partial
        // CPU records. They cannot be promoted to a complete render or latency frame.
        const bool partial_end = event->stage == RSF_GAME_CPU_FRAME_END &&
            (frame.stage_mask == 1u || frame.stage_mask == 3u);
        const bool rendered_end = rendered &&
            ((event->stage == RSF_GAME_CPU_SIMULATION_END && (frame.stage_mask == 1u || frame.stage_mask == 5u)) ||
             (event->stage == RSF_GAME_CPU_FRAME_END && (frame.stage_mask == 9u || frame.stage_mask == 13u)));
        const bool early_simulation = facts && event->stage==RSF_GAME_CPU_SIMULATION_BEGIN && frame.stage_mask==1u;
        const bool input_after_begin = facts && event->stage==RSF_GAME_CPU_INPUT_SAMPLE && frame.stage_mask==5u;
        uint64_t previous = 0;
        for (const auto stamp : frame.timestamps_qpc) if (stamp > previous) previous = stamp;
        if (frame.qpc_frequency != event->qpc_frequency || event->timestamp_qpc < previous ||
            (!rendered_end && !partial_end && !early_simulation && !input_after_begin && frame.stage_mask != expected)) {
            frame.failed = 1; ++totals.order_violations;
            if (event->stage == RSF_GAME_CPU_FRAME_END) { frame.ended = 1; reservation_end(event->session_id, event->source_frame_id); }
            return 0;
        }
        if (event->stage == RSF_GAME_CPU_INPUT_SAMPLE && (event->flags & RSF_GAME_CPU_INPUT_SUBSTITUTE)) {
            frame.flags |= RSF_GAME_CPU_INPUT_SUBSTITUTE; ++totals.substitutes;
        }
        frame.timestamps_qpc[event->stage] = event->timestamp_qpc;
        if (event->flags & RSF_GAME_CPU_DERIVED_BOUNDARY) frame.flags|=RSF_GAME_CPU_DERIVED_BOUNDARY;
        frame.stage_mask |= 1u << event->stage;
        frame.ended = event->stage == RSF_GAME_CPU_FRAME_END;
        if (frame.ended && (event->flags & RSF_GAME_CPU_DERIVED_BOUNDARY)) ++totals.derived_frame_ends;
        if (frame.ended) {
            reservation_end(event->session_id, event->source_frame_id);
            // The engine's own statement that this source will not present is a Present outcome.
            // CPU closure alone says nothing about Present.
            if ((event->flags & RSF_GAME_CPU_RENDER_SKIPPED) && !(event->flags & RSF_GAME_CPU_DERIVED_BOUNDARY))
                (void)record_present(event->session_id, event->source_frame_id, RSF_NATIVE_CPU_DEPENDENCY_CANCELLED);
        }
        sink = supplied_sink ? supplied_sink : consumer; user = supplied_sink ? supplied_user : consumer_user;
        }
    }
    if (sink) sink(user, event);
    if (event->stage == RSF_GAME_CPU_FRAME_BEGIN && facts) {
        std::array<rsf_game_cpu_event,16> ready{}; size_t count{};
        {
            std::lock_guard lock(guard);
            for (auto& pending : pending_messages) {
                if (!pending.message_id && !pending.input_kind) continue;
                if (pending.session_id == event->session_id && (pending.message_id == ping_message || (pending.input_kind && (pending.flags & RSF_GAME_CPU_INPUT_UNFRAMED))) &&
                    pending.qpc_frequency == event->qpc_frequency && event->timestamp_qpc >= pending.timestamp_qpc) {
                    if (event->timestamp_qpc-pending.timestamp_qpc <= event->qpc_frequency/4) {
                        ready[count]=pending; ready[count].source_frame_id=event->source_frame_id;
                        if (!(ready[count].flags & RSF_GAME_CPU_INPUT_UNFRAMED)) ready[count].timestamp_qpc=event->timestamp_qpc;
                        ++count;
                    } else ++totals.expired_messages;
                }
                pending={};
            }
        }
        for (size_t i=0;i<count;++i) (void)rsf_native_cpu_dispatch(&ready[i],supplied_sink,supplied_user);
    }
    return 1;
}
extern "C" RSF_RUNTIME_API int rsf_native_cpu_event(const rsf_game_cpu_event* event) {
    return rsf_native_cpu_dispatch(event, nullptr, nullptr);
}
extern "C" RSF_RUNTIME_API int rsf_native_cpu_read(uint64_t session, uint64_t source, rsf_native_cpu_frame* frame)
{
    if (!frame || frame->struct_size < sizeof(*frame) || !source) return 0;
    std::lock_guard<std::mutex> lock(guard);
    const auto& record = frames[source % frames.size()];
    if (record.session_id != session || record.source_frame_id != source) return 0;
    *frame = record; return 1;
}
extern "C" RSF_RUNTIME_API void rsf_native_cpu_set_sink(rsf_game_cpu_event_fn sink, void* user)
{ std::lock_guard<std::mutex> lock(guard); consumer = sink; consumer_user = user; }
extern "C" RSF_RUNTIME_API void rsf_native_cpu_release(void)
{
    std::lock_guard<std::mutex> lock(guard);
    clear_facts();
}

extern "C" RSF_RUNTIME_API int rsf_native_cpu_release_session(uint64_t session) {
    std::lock_guard lock(guard);
    if (!session || current_session!=session) return 0;
    clear_facts(); return 1;
}

extern "C" RSF_RUNTIME_API int rsf_native_cpu_counters_get(rsf_native_cpu_counters* out) {
    if (!out || out->struct_size < sizeof(*out)) return 0;
    std::lock_guard lock(guard); *out=totals; return 1;
}
extern "C" RSF_RUNTIME_API void rsf_native_cpu_ping_message(uint32_t message) {
    std::lock_guard lock(guard); ping_message = message >= 0xc000 && message <= 0xffff ? message : 0;
}
extern "C" RSF_RUNTIME_API int rsf_native_cpu_marker(uint64_t source, uint32_t marker) {
    if (!source || marker > 5) return -1;
    std::lock_guard lock(guard);
    auto& frame=frames[source%frames.size()];
    if (frame.source_frame_id != source || frame.session_id != current_session) return -1;
    if (frame.marker_mask & (1u<<marker)) { ++totals.duplicates; return 0; }
    // Legacy threaded hosts report overlapping simulation/render and EndFrame after Present.
    // Preserve those dependencies until they migrate; input-keyed facts use canonical order.
    static constexpr uint32_t legacy_required[]{0u,1u,1u,4u,4u,16u};
    const auto required = frame.flags & RSF_GAME_CPU_INPUT_KEYED ? (1u<<marker)-1 : legacy_required[marker];
    if ((frame.marker_mask & required) != required) { ++totals.order_violations; return -1; }
    frame.marker_mask |= 1u<<marker; return 1;
}

extern "C" RSF_RUNTIME_API int rsf_native_cpu_reservation_read(uint64_t session, uint64_t source, rsf_native_cpu_reservation* out) {
    if (!out || out->struct_size < sizeof(*out)) return 0;
    std::lock_guard lock(guard);
    const auto* entry = session == current_session ? find_reservation(session, source) : nullptr;
    if (!entry) return 0;
    *out = *entry;
    out->predecessor_present = present_state(out->predecessor_source_id);
    return 1;
}
extern "C" RSF_RUNTIME_API int rsf_native_cpu_reservation_counters_get(rsf_native_cpu_reservation_counters* out) {
    if (!out || out->struct_size < sizeof(*out)) return 0;
    std::lock_guard lock(guard);
    *out = reservation_totals; out->rejected = reservation_rejected.load(std::memory_order_relaxed);
    return 1;
}
extern "C" RSF_RUNTIME_API int rsf_native_cpu_present_result(uint64_t session, uint64_t source, uint32_t outcome, int32_t native_result) {
    int32_t state{};
    if (outcome == RSF_NATIVE_CPU_PRESENT_RETURNED)
        state = native_result >= 0 ? RSF_NATIVE_CPU_DEPENDENCY_PRESENTED : RSF_NATIVE_CPU_DEPENDENCY_FAILED;
    else if (outcome == RSF_NATIVE_CPU_PRESENT_RESULT_UNKNOWN) state = RSF_NATIVE_CPU_DEPENDENCY_UNVERIFIED;
    else if (outcome == RSF_NATIVE_CPU_PRESENT_ABORTED) state = RSF_NATIVE_CPU_DEPENDENCY_CANCELLED;
    else return 0;
    std::lock_guard lock(guard);
    return record_present(session, source, state) ? 1 : 0;
}
extern "C" RSF_RUNTIME_API void rsf_native_cpu_paced(uint64_t source, uint64_t began, uint64_t ready) {
    if (!source || !began || ready<began) return;
    std::lock_guard lock(guard); auto& frame=frames[source%frames.size()];
    if (frame.source_frame_id != source || frame.session_id != current_session) return;
    frame.pacing_qpc=began; frame.input_ready_qpc=ready;
}
