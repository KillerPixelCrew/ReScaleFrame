// SPDX-License-Identifier: MIT
// Prospective source reservation, delayed binding, explicit abort and predecessor
// actual-Present outcome through the generic CPU event interface.
#include <rescaleframe/native_cpu.h>
#include <windows.h>
#include <cstdio>
#include <cstring>
#include <vector>
#include <thread>
namespace {
int failures{};
#define CHECK(x) do { if (!(x)) { std::printf("FAIL %d: %s\n", __LINE__, #x); ++failures; } } while (false)
constexpr uint64_t session = 41;
std::vector<rsf_game_cpu_event> delivered;
bool withdraw_on_bind{};
bool actual_begin_on_bind{};
void sink(void*, const rsf_game_cpu_event* event) {
    // Source callbacks must see the committed ledger and may reenter it without deadlock.
    if (event->stage >= RSF_GAME_CPU_SOURCE_RESERVE) {
        rsf_native_cpu_reservation value{}; value.struct_size = sizeof(value);
        CHECK(rsf_native_cpu_reservation_read(event->session_id, event->source_frame_id, &value));
        CHECK(value.state != 0);
        if (event->stage == RSF_GAME_CPU_SOURCE_RESERVE) CHECK(value.state == RSF_NATIVE_CPU_RESERVATION_OPEN);
        if (event->stage == RSF_GAME_CPU_SOURCE_BIND) CHECK(value.state == RSF_NATIVE_CPU_RESERVATION_BOUND);
    }
    delivered.push_back(*event);
    if (event->stage == RSF_GAME_CPU_SOURCE_BIND && withdraw_on_bind) {
        withdraw_on_bind = false;
        rsf_game_cpu_source_event abort{};
        abort.header = *event; abort.header.struct_size = sizeof(abort);
        abort.header.stage = RSF_GAME_CPU_SOURCE_ABORT; ++abort.header.timestamp_qpc;
        abort.reason = RSF_GAME_CPU_ABORT_QUIESCE;
        CHECK(rsf_native_cpu_dispatch(&abort.header, sink, nullptr));
    }
    if (event->stage == RSF_GAME_CPU_SOURCE_BIND && actual_begin_on_bind) {
        actual_begin_on_bind = false;
        auto actual = *event; actual.struct_size = sizeof(actual);
        actual.event_version = RSF_GAME_CPU_EVENT_VERSION;
        actual.stage = RSF_GAME_CPU_SIMULATION_BEGIN; ++actual.timestamp_qpc;
        actual.flags = RSF_GAME_CPU_INPUT_KEYED;
        CHECK(rsf_native_cpu_dispatch(&actual, sink, nullptr));
    }
}
rsf_game_cpu_event fact(uint64_t source, uint32_t stage, uint64_t stamp, uint32_t flags = 0) {
    return {sizeof(rsf_game_cpu_event), stage, session, source, stamp, 1000, 1, 0, 0,
        RSF_GAME_CPU_EVENT_VERSION, RSF_GAME_CPU_INPUT_KEYED | flags};
}
rsf_game_cpu_source_event source_fact(uint64_t source, uint32_t stage, uint64_t stamp, uint64_t predecessor = 0) {
    rsf_game_cpu_source_event event{};
    event.header = {sizeof(rsf_game_cpu_source_event), stage, session, source, stamp, 1000, 0, 0, 0,
        RSF_GAME_CPU_EVENT_VERSION_SOURCE, 0};
    event.predecessor_source_id = predecessor;
    event.native_generation = source * 10;
    return event;
}
int send(const rsf_game_cpu_event& event) { return rsf_native_cpu_dispatch(&event, &sink, nullptr); }
int send(const rsf_game_cpu_source_event& event) { return rsf_native_cpu_dispatch(&event.header, &sink, nullptr); }
int reserve(uint64_t source, uint64_t stamp, uint64_t predecessor) {
    auto event = source_fact(source, RSF_GAME_CPU_SOURCE_RESERVE, stamp, predecessor);
    event.header.flags = RSF_GAME_CPU_SOURCE_OVERLAPS_PREDECESSOR;
    return send(event);
}
// The poll was entered one tick before it returned, after RESERVE returned.
int sampled(uint64_t source, uint64_t stamp, bool produced = true, uint32_t kinds = RSF_GAME_INPUT_MOUSE) {
    auto event = source_fact(source, RSF_GAME_CPU_SOURCE_SAMPLED, stamp);
    event.header.input_kind = kinds;
    event.header.flags = produced ? RSF_GAME_CPU_SOURCE_PRODUCED : 0;
    event.poll_begin_qpc = stamp - 1;
    return send(event);
}
int bind(uint64_t source, uint64_t stamp) { return send(source_fact(source, RSF_GAME_CPU_SOURCE_BIND, stamp)); }
int abort_source(uint64_t source, uint64_t stamp, uint32_t reason) {
    auto event = source_fact(source, RSF_GAME_CPU_SOURCE_ABORT, stamp);
    event.reason = reason;
    return send(event);
}
int presented(uint64_t source, int32_t result = 0) {
    return rsf_native_cpu_present_result(session, source, RSF_NATIVE_CPU_PRESENT_RETURNED, result);
}
rsf_native_cpu_reservation read(uint64_t source) {
    rsf_native_cpu_reservation value{}; value.struct_size = sizeof(value);
    if (!rsf_native_cpu_reservation_read(session, source, &value)) value.state = 0;
    return value;
}
rsf_native_cpu_reservation_counters counters() {
    rsf_native_cpu_reservation_counters value{}; value.struct_size = sizeof(value);
    (void)rsf_native_cpu_reservation_counters_get(&value);
    return value;
}
bool source_stage_delivered() {
    for (const auto& event : delivered) if (event.stage >= RSF_GAME_CPU_SOURCE_RESERVE) return true;
    return false;
}
// A packet placed directly before an inaccessible page proves the core never reads
// beyond the declared size of an old or truncated event.
void short_packets_are_not_overread() {
    SYSTEM_INFO system{}; GetSystemInfo(&system);
    auto* pages = static_cast<unsigned char*>(VirtualAlloc(nullptr, system.dwPageSize * 2, MEM_RESERVE | MEM_COMMIT, PAGE_READWRITE));
    DWORD old{};
    CHECK(pages && VirtualProtect(pages + system.dwPageSize, system.dwPageSize, PAGE_NOACCESS, &old));
    if (!pages) return;
    auto* edge = pages + system.dwPageSize - sizeof(rsf_game_cpu_event);
    const auto begin = fact(1, RSF_GAME_CPU_FRAME_BEGIN, 1);
    std::memcpy(edge, &begin, sizeof(begin));
    CHECK(rsf_native_cpu_dispatch(reinterpret_cast<const rsf_game_cpu_event*>(edge), &sink, nullptr));
    auto header = source_fact(90, RSF_GAME_CPU_SOURCE_RESERVE, 2, 1).header;
    header.struct_size = sizeof(rsf_game_cpu_event); // version 2 stage without its extension
    std::memcpy(edge, &header, sizeof(header));
    CHECK(!rsf_native_cpu_dispatch(reinterpret_cast<const rsf_game_cpu_event*>(edge), &sink, nullptr));
    VirtualFree(pages, 0, MEM_RELEASE);
    rsf_native_cpu_release();
}
}
int main() {
    rsf_native_cpu_release();
    short_packets_are_not_overread();
    rsf_native_cpu_rendered_source_set(1); // the rendered-source contract the Skyrim host uses

    // No session yet: a reservation cannot establish one.
    CHECK(!reserve(2, 5, 1));
    // Source S=1 renders while the late native poll reserves future source P=2.
    CHECK(send(fact(1, RSF_GAME_CPU_FRAME_BEGIN, 10)));
    CHECK(send(fact(1, RSF_GAME_CPU_SIMULATION_END, 20)));
    CHECK(reserve(2, 30, 1));
    CHECK(read(2).state == RSF_NATIVE_CPU_RESERVATION_OPEN && read(2).reserve_qpc == 30);
    CHECK(read(2).predecessor_present == RSF_NATIVE_CPU_DEPENDENCY_PENDING);
    CHECK(!bind(2, 31)); // poll still in flight and no consuming frame
    auto early = source_fact(2, RSF_GAME_CPU_SOURCE_SAMPLED, 32);
    early.header.flags = RSF_GAME_CPU_SOURCE_PRODUCED; early.poll_begin_qpc = 29;
    CHECK(!send(early)); // the poll cannot begin before the reservation opened
    early.poll_begin_qpc = 33;
    CHECK(!send(early)); // nor after it returned
    CHECK(sampled(2, 32));
    // Reservation opened at 30, before anything the runtime might do inside RESERVE; the
    // actual sample is the poll entry at 31, after RESERVE returned; 32 is the poll's return.
    CHECK(read(2).poll_begin_qpc == 31 && read(2).sampled_qpc == 32);
    // S's actual Present returns before its CPU FRAME_END; the outcome does not need it.
    CHECK(presented(1));
    CHECK(read(2).predecessor_present == RSF_NATIVE_CPU_DEPENDENCY_PRESENTED);
    CHECK(!presented(1)); // the first actual outcome stands
    CHECK(send(fact(1, RSF_GAME_CPU_FRAME_END, 40)));
    // The next tick adopts the reserved identity. Its actual input predates FRAME_BEGIN.
    delivered.clear();
    CHECK(send(fact(2, RSF_GAME_CPU_FRAME_BEGIN, 50)));
    CHECK(read(2).state == RSF_NATIVE_CPU_RESERVATION_ADOPTED && read(2).frame_begin_qpc == 50);
    CHECK(read(2).reserve_qpc < read(2).frame_begin_qpc);
    CHECK(!bind(2, 49)); // binding cannot precede the consuming update
    CHECK(bind(2, 55));
    CHECK(read(2).state == RSF_NATIVE_CPU_RESERVATION_BOUND && read(2).bind_qpc == 55 &&
        read(2).input_kind == RSF_GAME_INPUT_MOUSE && read(2).native_generation == 20);
    CHECK(!bind(2, 56)); // one binding per reservation
    rsf_native_cpu_frame frame{}; frame.struct_size = sizeof(frame);
    // A binding is no INPUT_SAMPLE, SIMULATION_BEGIN, marker or pacing time of the frame.
    CHECK(rsf_native_cpu_read(session, 2, &frame) && frame.stage_mask == 1u && !frame.marker_mask && !frame.pacing_qpc);
    CHECK(!source_stage_delivered()); // facts stay in the core; no backend sink sees them
    // The next late poll overlaps P's own render: the steady state, not an overlap.
    CHECK(reserve(3, 60, 2) && sampled(3, 61));
    CHECK(send(fact(2, RSF_GAME_CPU_SIMULATION_END, 62)));
    CHECK(send(fact(2, RSF_GAME_CPU_FRAME_END, 70)));
    CHECK(read(2).state == RSF_NATIVE_CPU_RESERVATION_CLOSED);
    // P's CPU closure says nothing about its Present.
    CHECK(read(3).predecessor_present == RSF_NATIVE_CPU_DEPENDENCY_PENDING);
    CHECK(send(fact(3, RSF_GAME_CPU_FRAME_BEGIN, 80)));
    CHECK(reserve(4, 85, 3) && sampled(4, 86));
    // A completion in another session never resolves this session's reused identity.
    CHECK(!rsf_native_cpu_present_result(session + 1, 3, RSF_NATIVE_CPU_PRESENT_RETURNED, 0));
    CHECK(!rsf_native_cpu_present_result(session, 3, 99, 0));
    CHECK(read(4).predecessor_present == RSF_NATIVE_CPU_DEPENDENCY_PENDING);
    // Source 3 never bound: the paused tick closes its reservation unbound, and the engine's
    // skipped render is its Present outcome.
    CHECK(send(fact(3, RSF_GAME_CPU_FRAME_END, 90, RSF_GAME_CPU_RENDER_SKIPPED)));
    CHECK(read(3).state == RSF_NATIVE_CPU_RESERVATION_UNBOUND);
    CHECK(read(4).predecessor_present == RSF_NATIVE_CPU_DEPENDENCY_CANCELLED);

    // Intervening poll: a newer reservation withdraws the sampled, unadopted one.
    CHECK(reserve(5, 95, 3) && sampled(5, 96));
    CHECK(read(4).state == RSF_NATIVE_CPU_RESERVATION_ABORTED && read(4).abort_reason == RSF_GAME_CPU_ABORT_INTERVENING_POLL);
    CHECK(reserve(6, 98, 3));
    CHECK(read(5).state == RSF_NATIVE_CPU_RESERVATION_ABORTED);
    // Concurrent polls: a reservation while another poll is in flight refuses both.
    CHECK(!reserve(7, 99, 3));
    CHECK(read(6).state == RSF_NATIVE_CPU_RESERVATION_ABORTED && read(6).abort_reason == RSF_GAME_CPU_ABORT_OVERLAP);
    CHECK(!read(7).state && !sampled(6, 100) && !bind(6, 101));
    // Reuse: an identity at or below one already reserved or begun is refused.
    CHECK(!reserve(6, 102, 3) && !reserve(3, 102, 2) && !reserve(7, 102, 3));
    // A frame cannot adopt a reservation whose poll is still in flight. The engine frame is kept,
    // without input ownership.
    CHECK(reserve(8, 103, 3));
    CHECK(send(fact(8, RSF_GAME_CPU_FRAME_BEGIN, 104)));
    CHECK(read(8).state == RSF_NATIVE_CPU_RESERVATION_ABORTED && read(8).abort_reason == RSF_GAME_CPU_ABORT_OVERLAP);
    CHECK(!bind(8, 105) && !sampled(8, 105));
    CHECK(send(fact(8, RSF_GAME_CPU_FRAME_END, 106)));
    // A frame with another identity supersedes an outstanding sampled reservation.
    CHECK(reserve(9, 107, 3) && sampled(9, 108));
    CHECK(send(fact(10, RSF_GAME_CPU_FRAME_BEGIN, 109)));
    CHECK(read(9).state == RSF_NATIVE_CPU_RESERVATION_ABORTED && read(9).abort_reason == RSF_GAME_CPU_ABORT_SUPERSEDED);
    // A poll without exclusively completed control state can be adopted but never bound.
    CHECK(reserve(11, 110, 10) && sampled(11, 111, false));
    CHECK(rsf_native_cpu_present_result(session, 10, RSF_NATIVE_CPU_PRESENT_RESULT_UNKNOWN, 0));
    CHECK(read(11).predecessor_present == RSF_NATIVE_CPU_DEPENDENCY_UNVERIFIED);
    CHECK(send(fact(10, RSF_GAME_CPU_FRAME_END, 112)));
    CHECK(send(fact(11, RSF_GAME_CPU_FRAME_BEGIN, 113)) && !bind(11, 114));
    CHECK(send(fact(11, RSF_GAME_CPU_FRAME_END, 115)));
    CHECK(read(11).state == RSF_NATIVE_CPU_RESERVATION_UNBOUND);
    // A failed actual Present is not a successful predecessor; an older unreported one is superseded.
    CHECK(reserve(12, 120, 11) && sampled(12, 121));
    CHECK(presented(11, E_FAIL));
    CHECK(read(12).predecessor_present == RSF_NATIVE_CPU_DEPENDENCY_FAILED);
    // Explicit abort.
    CHECK(abort_source(12, 122, RSF_GAME_CPU_ABORT_STATE_CHANGED));
    CHECK(read(12).state == RSF_NATIVE_CPU_RESERVATION_ABORTED && read(12).abort_reason == RSF_GAME_CPU_ABORT_STATE_CHANGED);
    CHECK(!bind(12, 123) && !abort_source(12, 124, 0));
    // A withdrawn identity still begins as an ordinary frame, but never gains input ownership.
    CHECK(send(fact(12, RSF_GAME_CPU_FRAME_BEGIN, 124)));
    CHECK(read(12).state == RSF_NATIVE_CPU_RESERVATION_ABORTED && !bind(12, 125));
    CHECK(send(fact(12, RSF_GAME_CPU_FRAME_END, 125)));
    // Source 2 never reported a Present; a later successful one supersedes it.
    CHECK(presented(12) && read(3).predecessor_present == RSF_NATIVE_CPU_DEPENDENCY_SUPERSEDED);
    // Stale session, frequency and malformed packets are refused.
    auto stale = source_fact(13, RSF_GAME_CPU_SOURCE_RESERVE, 125, 11); stale.header.session_id = session + 1;
    CHECK(!send(stale));
    auto frequency = source_fact(13, RSF_GAME_CPU_SOURCE_RESERVE, 125, 11); frequency.header.qpc_frequency = 999;
    CHECK(!send(frequency));
    auto future_predecessor = source_fact(14, RSF_GAME_CPU_SOURCE_RESERVE, 125, 13);
    CHECK(!send(future_predecessor)); // a predecessor must be a begun source
    auto version_two_base = fact(13, RSF_GAME_CPU_FRAME_BEGIN, 126);
    version_two_base.event_version = RSF_GAME_CPU_EVENT_VERSION_SOURCE;
    CHECK(!send(version_two_base));
    auto version_one_source = source_fact(13, RSF_GAME_CPU_SOURCE_RESERVE, 126, 11);
    version_one_source.header.event_version = RSF_GAME_CPU_EVENT_VERSION;
    CHECK(!send(version_one_source));
    auto unknown_flags = source_fact(13, RSF_GAME_CPU_SOURCE_RESERVE, 126, 11); unknown_flags.header.flags = 0x80000000u;
    CHECK(!send(unknown_flags));
    auto kinds_on_reserve = source_fact(13, RSF_GAME_CPU_SOURCE_RESERVE, 126, 11); kinds_on_reserve.header.input_kind = 1;
    CHECK(!send(kinds_on_reserve));
    auto sample_on_reserve = source_fact(13, RSF_GAME_CPU_SOURCE_RESERVE, 126, 11); sample_on_reserve.poll_begin_qpc = 126;
    CHECK(!send(sample_on_reserve)); // a sample time belongs to SAMPLED only
    sample_on_reserve.header.struct_size = RSF_GAME_CPU_SOURCE_EVENT_SIZE_1 + 1;
    CHECK(!send(sample_on_reserve)); // a partial timestamp must not become a fabricated QPC
    // A first-version packet, ending before poll_begin_qpc, is read only to its size: the
    // sample time stays unknown.
    CHECK(reserve(13, 127, 12));
    auto first_version = source_fact(13, RSF_GAME_CPU_SOURCE_SAMPLED, 128);
    first_version.header.flags = RSF_GAME_CPU_SOURCE_PRODUCED; first_version.poll_begin_qpc = 127;
    first_version.header.struct_size = RSF_GAME_CPU_SOURCE_EVENT_SIZE_1;
    CHECK(send(first_version) && read(13).state == RSF_NATIVE_CPU_RESERVATION_SAMPLED && !read(13).poll_begin_qpc);
    CHECK(abort_source(13, 129, RSF_GAME_CPU_ABORT_UNSPECIFIED));
    CHECK(!source_stage_delivered());
    const auto totals = counters();
    CHECK(totals.reserved == 10 && totals.sampled == 8 && totals.adopted == 3 && totals.bound == 1 && totals.closed == 1);
    CHECK(totals.unbound == 2 && totals.aborted == 7 && totals.rejected >= 18);

    // Quiesce: session release drops every reservation and Present outcome.
    CHECK(send(fact(14, RSF_GAME_CPU_FRAME_BEGIN, 130)));
    CHECK(reserve(15, 131, 14) && sampled(15, 132));
    CHECK(rsf_native_cpu_release_session(session));
    CHECK(!read(15).state && !counters().reserved && !presented(14));
    // A new session starts clean: no outcome of the old one resolves its identities.
    CHECK(send(fact(20, RSF_GAME_CPU_FRAME_BEGIN, 200)));
    CHECK(reserve(21, 201, 20) && sampled(21, 202));
    CHECK(read(21).predecessor_present == RSF_NATIVE_CPU_DEPENDENCY_PENDING);
    CHECK(abort_source(21, 203, RSF_GAME_CPU_ABORT_QUIESCE) && read(21).state == RSF_NATIVE_CPU_RESERVATION_ABORTED);
    // Existing version 1 clients keep their behavior beside reservations.
    CHECK(send(fact(20, RSF_GAME_CPU_SIMULATION_END, 210)) && send(fact(20, RSF_GAME_CPU_FRAME_END, 220)));
    rsf_native_cpu_rendered_source_set(0);
    rsf_native_cpu_release();
    // Input-timeline hosts deliver RESERVE synchronously, before the poll. A later
    // valid binding promotes the real poll entry without moving it to FRAME_BEGIN.
    delivered.clear();
    CHECK(send(fact(1, RSF_GAME_CPU_FRAME_BEGIN, 10)));
    CHECK(send(fact(1, RSF_GAME_CPU_INPUT_SAMPLE, 11)));
    CHECK(send(fact(1, RSF_GAME_CPU_SIMULATION_BEGIN, 12)));
    CHECK(send(fact(1, RSF_GAME_CPU_SIMULATION_END, 20)));
    CHECK(reserve(2, 30, 1));
    CHECK(delivered.back().stage == RSF_GAME_CPU_SOURCE_RESERVE);
    const auto after_reserve = delivered.size();
    CHECK(!reserve(2, 31, 1) && delivered.size() == after_reserve);
    CHECK(sampled(2, 35));
    CHECK(presented(1)); // no FRAME_END dependency for pre-input pacing
    CHECK(send(fact(1, RSF_GAME_CPU_FRAME_END, 40)));
    auto pending = fact(0, RSF_GAME_CPU_INPUT_EVENT, 45, RSF_GAME_CPU_INPUT_UNFRAMED);
    pending.input_kind = RSF_GAME_INPUT_MOUSE;
    CHECK(send(pending));
    delivered.clear();
    CHECK(send(fact(2, RSF_GAME_CPU_FRAME_BEGIN, 50)));
    CHECK(rsf_native_cpu_read(session, 2, &frame) && frame.stage_mask == 1u && !frame.timestamps_qpc[1]);
    CHECK(bind(2, 55));
    CHECK(rsf_native_cpu_read(session, 2, &frame) && frame.stage_mask == 7u && frame.timestamps_qpc[1] == 34 &&
        frame.timestamps_qpc[2] == 55 && (frame.flags & RSF_GAME_CPU_DERIVED_BOUNDARY));
    CHECK(!frame.pacing_qpc && !frame.input_ready_qpc && !frame.marker_mask && !frame.input_mask && !frame.pings);
    CHECK(frame.timestamps_qpc[1] < frame.timestamps_qpc[0]);
    CHECK(delivered.size() == 3 && delivered[0].stage == RSF_GAME_CPU_FRAME_BEGIN &&
        delivered[1].stage == RSF_GAME_CPU_SOURCE_BIND && delivered[2].stage == RSF_GAME_CPU_SIMULATION_BEGIN &&
        (delivered[2].flags & RSF_GAME_CPU_DERIVED_BOUNDARY) &&
        !(delivered[2].flags & RSF_GAME_CPU_INPUT_SUBSTITUTE)); // no second INPUT_SAMPLE or pace
    const auto after_bind = delivered.size();
    CHECK(!bind(2, 56) && delivered.size() == after_bind);
    CHECK(send(fact(2, RSF_GAME_CPU_SIMULATION_BEGIN, 56)));
    CHECK(send(fact(2, RSF_GAME_CPU_SIMULATION_END, 60)));
    CHECK(send(fact(2, RSF_GAME_CPU_FRAME_END, 70)));
    CHECK(rsf_native_cpu_read(session, 2, &frame) && frame.stage_mask == 31u && !frame.failed &&
        !(frame.flags & RSF_GAME_CPU_INPUT_SUBSTITUTE) && frame.timestamps_qpc[1] == 34 && frame.timestamps_qpc[2] == 56);
    CHECK(read(2).state == RSF_NATIVE_CPU_RESERVATION_CLOSED);
    rsf_native_cpu_counters measured{}; measured.struct_size = sizeof(measured);
    CHECK(rsf_native_cpu_counters_get(&measured) && !measured.substitutes && !measured.order_violations &&
        !measured.input_events[1] && !measured.pings);

    // Old packets without actual poll entry remain unknown, even after native bind.
    CHECK(reserve(3, 71, 2));
    auto unknown_sample = source_fact(3, RSF_GAME_CPU_SOURCE_SAMPLED, 74);
    unknown_sample.header.struct_size = RSF_GAME_CPU_SOURCE_EVENT_SIZE_1;
    unknown_sample.header.flags = RSF_GAME_CPU_SOURCE_PRODUCED;
    CHECK(send(unknown_sample));
    CHECK(send(fact(3, RSF_GAME_CPU_FRAME_BEGIN, 80)) && bind(3, 81));
    CHECK(rsf_native_cpu_read(session, 3, &frame) && frame.stage_mask == 1u && !frame.timestamps_qpc[1]);
    CHECK(send(fact(3, RSF_GAME_CPU_SIMULATION_BEGIN, 82)));
    CHECK(send(fact(3, RSF_GAME_CPU_SIMULATION_END, 90)));
    CHECK(rsf_native_cpu_read(session, 3, &frame) && (frame.flags & RSF_GAME_CPU_INPUT_SUBSTITUTE));
    CHECK(send(fact(3, RSF_GAME_CPU_FRAME_END, 100)));

    // Aborted consumption cannot promote its otherwise real poll timestamp.
    CHECK(reserve(4, 101, 3) && sampled(4, 103));
    CHECK(send(fact(4, RSF_GAME_CPU_FRAME_BEGIN, 110)));
    CHECK(abort_source(4, 111, RSF_GAME_CPU_ABORT_NOT_CONSUMED) && !bind(4, 112));
    CHECK(rsf_native_cpu_read(session, 4, &frame) && frame.stage_mask == 1u && !frame.timestamps_qpc[1]);
    const auto before_refused_simulation = delivered.size();
    CHECK(send(fact(4, RSF_GAME_CPU_SIMULATION_BEGIN, 113)));
    CHECK(send(fact(4, RSF_GAME_CPU_SIMULATION_END, 114)));
    CHECK(delivered.size() == before_refused_simulation); // facts retained, no latency callback
    CHECK(rsf_native_cpu_read(session, 4, &frame) && frame.stage_mask == 13u && !frame.timestamps_qpc[1] &&
        frame.timestamps_qpc[2] == 113 && frame.timestamps_qpc[3] == 114 && !frame.pacing_qpc && !frame.marker_mask);
    CHECK(send(fact(4, RSF_GAME_CPU_FRAME_END, 115)));
    CHECK(rsf_native_cpu_read(session, 4, &frame) && frame.ended && !frame.failed && frame.stage_mask == 29u);
    CHECK(rsf_native_cpu_marker(4,0) == -1);
    CHECK(rsf_native_cpu_counters_get(&measured) && !measured.order_violations);
    // A refused bind can also leave an adopted reservation, without an ABORT.
    CHECK(reserve(5, 120, 4) && sampled(5, 122, false));
    CHECK(send(fact(5, RSF_GAME_CPU_FRAME_BEGIN, 130)) && !bind(5, 131));
    const auto before_unbound_simulation = delivered.size();
    CHECK(send(fact(5, RSF_GAME_CPU_SIMULATION_BEGIN, 132)));
    CHECK(send(fact(5, RSF_GAME_CPU_SIMULATION_END, 140)));
    CHECK(delivered.size() == before_unbound_simulation);
    CHECK(send(fact(5, RSF_GAME_CPU_FRAME_END, 150)));
    CHECK(read(5).state == RSF_NATIVE_CPU_RESERVATION_UNBOUND);
    CHECK(rsf_native_cpu_marker(5,0) == -1);
    CHECK(rsf_native_cpu_read(session, 5, &frame) && frame.stage_mask == 29u && !frame.failed &&
        !frame.timestamps_qpc[1] && !frame.pacing_qpc && !frame.marker_mask);
    // A flags0 frame is canonical only after real input ownership is established.
    CHECK(reserve(6, 160, 5) && sampled(6, 162));
    auto unkeyed = fact(6, RSF_GAME_CPU_FRAME_BEGIN, 170); unkeyed.flags = 0;
    CHECK(send(unkeyed) && bind(6, 171));
    CHECK(rsf_native_cpu_read(session, 6, &frame) && frame.stage_mask == 7u &&
        (frame.flags & RSF_GAME_CPU_INPUT_KEYED) && frame.timestamps_qpc[1] == 161);
    auto derived_end = fact(6, RSF_GAME_CPU_SIMULATION_END, 180, RSF_GAME_CPU_DERIVED_BOUNDARY);
    CHECK(send(derived_end));
    const auto derived_delivered = delivered.size();
    CHECK(send(derived_end) && delivered.size() == derived_delivered);
    auto actual_begin = fact(6, RSF_GAME_CPU_SIMULATION_BEGIN, 172); actual_begin.flags = 0;
    auto bad_clock = actual_begin; bad_clock.qpc_frequency = 999;
    CHECK(!send(bad_clock));
    auto backwards = actual_begin; backwards.timestamp_qpc = 169;
    CHECK(!send(backwards));
    auto crosses_end = actual_begin; crosses_end.timestamp_qpc = 181;
    CHECK(!send(crosses_end));
    CHECK(rsf_native_cpu_marker(6,0)==-1); // derived bookkeeping cannot authorize a marker
    CHECK(send(actual_begin) && delivered.size() == derived_delivered+1);
    CHECK(!(delivered.back().flags & RSF_GAME_CPU_DERIVED_BOUNDARY));
    CHECK(rsf_native_cpu_marker(6,0)==1);
    CHECK(rsf_native_cpu_marker(6,1)==-1);
    CHECK(rsf_native_cpu_read(session, 6, &frame) && frame.timestamps_qpc[2] == 172 &&
        (frame.flags & RSF_GAME_CPU_DERIVED_BOUNDARY)); // end remains derived
    auto actual_end = fact(6, RSF_GAME_CPU_SIMULATION_END, 181); actual_end.flags = 0;
    CHECK(send(actual_end) && delivered.size() == derived_delivered+2);
    CHECK(!(delivered.back().flags & RSF_GAME_CPU_DERIVED_BOUNDARY));
    CHECK(rsf_native_cpu_marker(6,1)==1);
    CHECK(rsf_native_cpu_read(session, 6, &frame) && frame.timestamps_qpc[3] == 181 &&
        !(frame.flags & RSF_GAME_CPU_DERIVED_BOUNDARY));
    CHECK(send(fact(6, RSF_GAME_CPU_FRAME_END, 182)));
    // Withdrawal from the synchronous bind sink must prevent derived simulation.
    CHECK(reserve(7, 190, 6) && sampled(7, 192));
    CHECK(send(fact(7, RSF_GAME_CPU_FRAME_BEGIN, 200)));
    withdraw_on_bind = true;
    CHECK(bind(7, 201));
    CHECK(rsf_native_cpu_read(session, 7, &frame) && frame.stage_mask == 3u &&
        !(frame.flags & RSF_GAME_CPU_DERIVED_BOUNDARY));
    CHECK(send(fact(7, RSF_GAME_CPU_FRAME_END, 203, RSF_GAME_CPU_RENDER_SKIPPED)));
    // Missing end remains missing for an explicitly skipped source; no completion promotion.
    CHECK(reserve(8, 210, 7) && sampled(8, 212));
    CHECK(send(fact(8, RSF_GAME_CPU_FRAME_BEGIN, 220)) && bind(8, 221));
    CHECK(abort_source(8, 222, RSF_GAME_CPU_ABORT_STATE_CHANGED));
    CHECK(send(fact(8, RSF_GAME_CPU_FRAME_END, 223, RSF_GAME_CPU_RENDER_SKIPPED)));
    CHECK(rsf_native_cpu_read(session, 8, &frame) && frame.stage_mask == 23u && frame.ended && !frame.failed &&
        !frame.timestamps_qpc[3]);
    CHECK(reserve(9, 230, 8) && sampled(9, 232));
    CHECK(send(fact(9, RSF_GAME_CPU_FRAME_BEGIN, 240)) && bind(9, 241));
    CHECK(!send(fact(9, RSF_GAME_CPU_FRAME_END, 243))); // unflagged partial still refuses
    CHECK(reserve(10, 250, 9) && sampled(10, 252));
    CHECK(send(fact(10, RSF_GAME_CPU_FRAME_BEGIN, 260)));
    actual_begin_on_bind = true;
    CHECK(bind(10, 261));
    CHECK(rsf_native_cpu_read(session, 10, &frame) && frame.stage_mask == 7u &&
        frame.timestamps_qpc[2] == 262 && !(frame.flags & RSF_GAME_CPU_DERIVED_BOUNDARY));
    CHECK(send(fact(10, RSF_GAME_CPU_SIMULATION_END, 263)) && send(fact(10, RSF_GAME_CPU_FRAME_END, 264)));
    // Replacing end first must not erase the still-derived begin's provenance.
    CHECK(reserve(11, 270, 10) && sampled(11, 272));
    CHECK(send(fact(11, RSF_GAME_CPU_FRAME_BEGIN, 280)) && bind(11, 281));
    CHECK(send(fact(11, RSF_GAME_CPU_SIMULATION_END, 290, RSF_GAME_CPU_DERIVED_BOUNDARY)));
    CHECK(send(fact(11, RSF_GAME_CPU_SIMULATION_END, 291)));
    CHECK(rsf_native_cpu_read(session, 11, &frame) && frame.timestamps_qpc[2] == 281 &&
        frame.timestamps_qpc[3] == 291 && (frame.flags & RSF_GAME_CPU_DERIVED_BOUNDARY));
    CHECK(send(fact(11, RSF_GAME_CPU_SIMULATION_BEGIN, 282)));
    CHECK(rsf_native_cpu_read(session, 11, &frame) && frame.timestamps_qpc[2] == 282 &&
        !(frame.flags & RSF_GAME_CPU_DERIVED_BOUNDARY));
    CHECK(send(fact(11, RSF_GAME_CPU_FRAME_END, 292)));
    rsf_native_cpu_release();
    // Main must be able to cancel a reservation while its worker callback is
    // still waiting. No ledger lock may be held across that callback.
    rsf_native_cpu_rendered_source_set(0);
    CHECK(send(fact(1, RSF_GAME_CPU_FRAME_BEGIN, 10)));
    struct Cancellation {
        HANDLE entered=CreateEventW(nullptr,TRUE,FALSE,nullptr);
        HANDLE cancelled=CreateEventW(nullptr,TRUE,FALSE,nullptr);
        bool observed=false;
        int accepted=0;
    } cancellation;
    CHECK(cancellation.entered && cancellation.cancelled);
    if (cancellation.entered && cancellation.cancelled) {
        std::thread waiting([&] {
            auto event=source_fact(2,RSF_GAME_CPU_SOURCE_RESERVE,20,1);
            event.header.flags=RSF_GAME_CPU_SOURCE_OVERLAPS_PREDECESSOR;
            cancellation.accepted=rsf_native_cpu_dispatch(&event.header,
                [](void* context,const rsf_game_cpu_event*) {
                    auto& value=*static_cast<Cancellation*>(context);
                    SetEvent(value.entered);
                    if (WaitForSingleObject(value.cancelled,2000)==WAIT_OBJECT_0)
                        value.observed=read(2).state==RSF_NATIVE_CPU_RESERVATION_ABORTED;
                },&cancellation);
        });
        CHECK(WaitForSingleObject(cancellation.entered,2000)==WAIT_OBJECT_0);
        auto abort=source_fact(2,RSF_GAME_CPU_SOURCE_ABORT,21);
        abort.reason=RSF_GAME_CPU_ABORT_QUIESCE;
        CHECK(rsf_native_cpu_dispatch(&abort.header,[](void*,const rsf_game_cpu_event*) {},nullptr));
        SetEvent(cancellation.cancelled);
        waiting.join();
        CHECK(cancellation.accepted && cancellation.observed);
    }
    if (cancellation.entered) CloseHandle(cancellation.entered);
    if (cancellation.cancelled) CloseHandle(cancellation.cancelled);
    rsf_native_cpu_release();
    std::printf("%d failures\n", failures);
    return failures ? 1 : 0;
}
