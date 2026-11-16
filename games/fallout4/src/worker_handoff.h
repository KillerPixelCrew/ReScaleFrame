// SPDX-License-Identifier: GPL-3.0-only
#pragma once
#include "cpu_events.h"

namespace rsf::fallout4::detail {
// Poll generation belongs to a bound sample, not an input-unbound render
// association. poll_only requires unchanged native owner/state and no lifecycle,
// raw-writer or overlapping-poll refusal; every other refusal invalidates both.
constexpr bool invalidates_render_source(uint64_t bound_source, bool poll_only) noexcept {
    return bound_source!=0 || !poll_only;
}
struct WorkerCycle {
    uint64_t session{}, source{}, idle{};
    uintptr_t main{}, worker{}, window{}, manager{}, queue{}, receiver{}, start_event{}, complete_event{};
    uint32_t frame{};
    bool operator==(const WorkerCycle&) const = default;
};
// Called with the current render owner locked after a worker saw mode0.
inline bool active_join_matches(const WorkerCycle& expected, const WorkerCycle& current,
    const WorkerCycle& predecessor, uint64_t worker_source, uint64_t successor_source,
    bool joining, bool swapped, bool closed, bool invalid) noexcept {
    return expected.source && worker_source && worker_source==successor_source &&
        current==expected && predecessor==expected && joining && swapped && closed && !invalid;
}
inline bool same_worker_owner(const WorkerCycle& a, const WorkerCycle& b) {
    return a.session==b.session && a.main==b.main && a.worker==b.worker && a.window==b.window &&
        a.manager==b.manager && a.queue==b.queue && a.receiver==b.receiver &&
        a.start_event==b.start_event && a.complete_event==b.complete_event;
}
// One copied future intention, not a frame history. The caller serializes only
// these scalar transitions; it never holds that lock while delivering events.
struct WorkerTicket {
    enum class Phase { empty, reserved, armed, sampled, begun, bound, consumed, complete, joined, claimed, adopted };
    Phase phase{};
    WorkerCycle predecessor{};
    CpuIdentity cpu{};
    uint64_t generation{}, raw{}, poll_begin{};
    uint32_t cursor{}, refusal{};
    bool frame_begun{}, callback_inflight{}, callback_returned{}, native_poll_started{};
    uint64_t predecessor_generation{}, predecessor_raw{};
    uint32_t predecessor_cursor{};
    bool active() const { return phase!=Phase::empty; }
    bool valid() const { return active() && !refusal; }
    void refuse(uint32_t reason) { if (active() && !refusal) refusal=reason; }
    bool reserve(const WorkerCycle& owner, const CpuIdentity& future, uint64_t gen, bool constructed) {
        if (active() || !constructed || !owner.source || future.source<=owner.source || future.session!=owner.session || !gen) return false;
        predecessor=owner; cpu=future; generation=gen; phase=Phase::reserved; return true;
    }
    bool begin_callback(uint64_t bound_generation, uint64_t bound_raw, uint32_t bound_cursor) {
        if (!valid() || phase!=Phase::reserved || callback_inflight || callback_returned || native_poll_started ||
            (bound_generation && (generation<=bound_generation || generation-bound_generation!=1))) return false;
        predecessor_generation=bound_generation; predecessor_raw=bound_raw; predecessor_cursor=bound_cursor;
        callback_inflight=true; return true;
    }
    void end_callback() noexcept {
        callback_returned=callback_inflight; callback_inflight=false;
    }
    // The predecessor's sample is already consumed. Raw arrivals and queue-tail
    // writes belong to the future poll, which has not started during this lease.
    bool protects_bound(const WorkerCycle& owner, uint64_t source, uint64_t bound_generation,
        uint64_t gen, uint64_t /*revision*/, uint32_t consumed, uint32_t /*queued*/, bool native_valid) const {
        return valid() && phase==Phase::reserved && callback_inflight && !native_poll_started &&
            owner==predecessor && source==predecessor.source && predecessor_generation &&
            bound_generation==predecessor_generation && gen==generation && generation>bound_generation &&
            generation-bound_generation==1 && consumed==predecessor_cursor && native_valid;
    }
    static bool poll_safe(uint64_t bound_source, bool closed, bool sealed) noexcept {
        return !bound_source || closed || sealed;
    }
    bool arm(const WorkerCycle& owner, uint64_t gen, uint64_t revision, bool native_valid, bool predecessor_safe) {
        if (!valid() || phase!=Phase::reserved || callback_inflight || !callback_returned ||
            owner!=predecessor || gen!=generation || !native_valid || !predecessor_safe) return false;
        raw=revision; phase=Phase::armed; return true;
    }
    void native_poll_entry() noexcept { callback_inflight=false; native_poll_started=true; }
    bool sample(uint64_t gen, uint64_t revision, uint32_t sequence, uint64_t began, bool native_valid) {
        if (!valid() || phase!=Phase::armed || !native_poll_started || gen!=generation || revision!=raw || !native_valid) return false;
        cursor=sequence; poll_begin=began; phase=Phase::sampled; return true;
    }
    bool begin(uint64_t now, uint64_t gen, uint64_t revision, uint32_t sequence, bool native_valid) {
        if (!valid() || phase!=Phase::sampled || gen!=generation || revision!=raw || sequence!=cursor || !native_valid) return false;
        cpu.began_qpc=now; frame_begun=true; phase=Phase::begun; return true;
    }
    bool bind(uint64_t gen, uint64_t revision, uint32_t sequence, bool native_valid) {
        if (!valid() || phase!=Phase::begun || gen!=generation || revision!=raw || sequence!=cursor || !native_valid) return false;
        phase=Phase::bound; return true;
    }
    bool consumed(uint64_t gen, uint32_t sequence, bool native_valid) {
        if (!valid() || phase!=Phase::bound || gen!=generation || sequence!=cursor || !native_valid) return false;
        phase=Phase::consumed; return true;
    }
    bool complete(bool native_valid) {
        if (!valid() || phase!=Phase::consumed || !native_valid) return false;
        phase=Phase::complete; return true;
    }
    bool join(const WorkerCycle& owner, bool completed_wait) {
        if (!valid() || phase!=Phase::complete || owner!=predecessor || !completed_wait) return false;
        phase=Phase::joined; return true;
    }
    // sequence is the receiver consumed cursor, never the live enqueue tail.
    bool successor(const WorkerCycle& owner, uint32_t sequence, uint64_t gen, bool world) const {
        return valid() && same_worker_owner(owner,predecessor) && owner.source==cpu.source &&
            owner.idle==predecessor.idle+1 && owner.frame==uint32_t(predecessor.frame+(world ? 1u : 0u)) &&
            sequence==cursor && gen==generation;
    }
    bool claim(const WorkerCycle& owner, uint32_t sequence, uint64_t gen) {
        if (phase!=Phase::joined || !successor(owner,sequence,gen,false)) return false;
        phase=Phase::claimed; return true;
    }
    bool adopt(const WorkerCycle& owner, uint32_t sequence, uint64_t gen) {
        if (phase!=Phase::claimed || !successor(owner,sequence,gen,true)) return false;
        phase=Phase::adopted; return true;
    }
};
}
