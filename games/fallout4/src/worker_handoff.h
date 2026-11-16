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
    bool frame_begun{};
    bool active() const { return phase!=Phase::empty; }
    bool valid() const { return active() && !refusal; }
    void refuse(uint32_t reason) { if (active() && !refusal) refusal=reason; }
    bool reserve(const WorkerCycle& owner, const CpuIdentity& future, uint64_t gen, bool present) {
        if (active() || !present || !owner.source || future.source<=owner.source || future.session!=owner.session || !gen) return false;
        predecessor=owner; cpu=future; generation=gen; phase=Phase::reserved; return true;
    }
    bool arm(const WorkerCycle& owner, uint64_t gen, uint64_t revision, bool native_valid) {
        // Present may have returned while RESERVE waited. Its original entry
        // proof is retained in predecessor, not demanded again here.
        if (!valid() || phase!=Phase::reserved || owner!=predecessor || gen!=generation || !native_valid) return false;
        raw=revision; phase=Phase::armed; return true;
    }
    bool sample(uint64_t gen, uint64_t revision, uint32_t sequence, uint64_t began, bool native_valid) {
        if (!valid() || phase!=Phase::armed || gen!=generation || revision!=raw || !native_valid) return false;
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
