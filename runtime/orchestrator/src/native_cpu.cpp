// SPDX-License-Identifier: GPL-3.0-only
#include <rescaleframe/native_cpu.h>
#include <array>
#include <mutex>
#include <windows.h>
#include <cstdio>
#include <cwchar>
#include <iterator>
namespace {
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
void clear_facts() {
    frames = {}; pending_messages={}; next_message=0; totals = {sizeof(totals)}; ping_message=0; current_session = last_begin = 0; consumer = nullptr; consumer_user = nullptr;
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
extern "C" RSF_RUNTIME_API int rsf_native_cpu_dispatch(const rsf_game_cpu_event* incoming,
    rsf_game_cpu_event_fn supplied_sink, void* supplied_user)
{
    if (!incoming || incoming->struct_size < sizeof(*incoming) || incoming->event_version > RSF_GAME_CPU_EVENT_VERSION ||
        incoming->flags & ~(RSF_GAME_CPU_INPUT_KEYED | RSF_GAME_CPU_INPUT_SUBSTITUTE | RSF_GAME_CPU_WINDOW_MESSAGE | RSF_GAME_CPU_RENDER_SKIPPED | RSF_GAME_CPU_INPUT_UNFRAMED | RSF_GAME_CPU_DERIVED_BOUNDARY)) return 0;
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
        if (previous.stage_mask==15u && !previous.ended) {
            auto end=*event; end.stage=RSF_GAME_CPU_FRAME_END; end.source_frame_id=previous.source_frame_id;
            end.flags|=RSF_GAME_CPU_DERIVED_BOUNDARY; end.render_expected=0;
            if (!rsf_native_cpu_dispatch(&end,supplied_sink,supplied_user)) return 0;
        }
    }
    // A renderer can run without an engine input callback (loading/first frame). Normalize that absence centrally,
    // label it, and skip a late pacing sleep. Adapters report only the simulation end they actually observed.
    if (facts && event->stage == RSF_GAME_CPU_SIMULATION_END) {
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
        if (!current_session && event->stage == RSF_GAME_CPU_FRAME_BEGIN) current_session = event->session_id;
        if (current_session != event->session_id) return 0;
        auto& frame = frames[event->source_frame_id % frames.size()];
        if (event->stage == RSF_GAME_CPU_FRAME_BEGIN) {
            if (event->source_frame_id <= last_begin || (frame.source_frame_id && !frame.ended)) return 0;
            frame = {}; frame.struct_size = sizeof(frame); frame.session_id = event->session_id;
            frame.source_frame_id = event->source_frame_id; frame.qpc_frequency = event->qpc_frequency;
            last_begin = event->source_frame_id; frame.flags=event->flags; ++totals.frames;
        } else {
            if (frame.source_frame_id != event->source_frame_id || frame.session_id != event->session_id ||
                frame.ended) return 0;
            if (frame.failed) {
                if (event->stage == RSF_GAME_CPU_FRAME_END) frame.ended = 1;
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
        const bool early_simulation = facts && event->stage==RSF_GAME_CPU_SIMULATION_BEGIN && frame.stage_mask==1u;
        const bool input_after_begin = facts && event->stage==RSF_GAME_CPU_INPUT_SAMPLE && frame.stage_mask==5u;
        uint64_t previous = 0;
        for (const auto stamp : frame.timestamps_qpc) if (stamp > previous) previous = stamp;
        if (frame.qpc_frequency != event->qpc_frequency || event->timestamp_qpc < previous ||
            (!partial_end && !early_simulation && !input_after_begin && frame.stage_mask != expected)) {
            frame.failed = 1; ++totals.order_violations;
            if (event->stage == RSF_GAME_CPU_FRAME_END) frame.ended = 1;
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

extern "C" RSF_RUNTIME_API void rsf_native_cpu_paced(uint64_t source, uint64_t began, uint64_t ready) {
    if (!source || !began || ready<began) return;
    std::lock_guard lock(guard); auto& frame=frames[source%frames.size()];
    if (frame.source_frame_id != source || frame.session_id != current_session) return;
    frame.pacing_qpc=began; frame.input_ready_qpc=ready;
}
