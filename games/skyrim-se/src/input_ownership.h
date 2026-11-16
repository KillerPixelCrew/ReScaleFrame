// SPDX-License-Identifier: MIT
#pragma once
#include <atomic>
#include <cstdint>
#include <mutex>

namespace rsf::skyrim {
// Scalar observations only. A job's submitting source is not the source that
// will consume its late input poll. State 1 publishes; state 3 refuses until verified native idle or reset.
class NativeJobOwnership {
public:
    struct Ticket { uint64_t generation{}, source{}, session{}; uintptr_t manager{}; };
    Ticket publish(uintptr_t manager, uint64_t source, uint64_t session, bool native_idle = false) noexcept {
        if (!manager || !source || !session) return {};
        uint64_t empty{};
        if (!state_.compare_exchange_strong(empty, 1)) {
            // Recovery requires the caller to have verified requested=running=0.
            // Never replace a live or publishing generation, even with idle evidence.
            if (!native_idle || empty != 3 || !state_.compare_exchange_strong(empty, 1)) {
                invalidate(); return {};
            }
        }
        const auto generation = next_.fetch_add(1) * 4;
        manager_.store(manager); source_.store(source); session_.store(session);
        uint64_t publishing = 1;
        if (!state_.compare_exchange_strong(publishing, generation)) return {};
        return {generation, source, session, manager};
    }
    Ticket snapshot() const noexcept {
        const auto generation = state_.load();
        if (generation < 4) return {};
        Ticket ticket{generation, source_.load(), session_.load(), manager_.load()};
        return state_.load() == generation ? ticket : Ticket{};
    }
    bool matches(Ticket ticket) const noexcept {
        const auto now = snapshot();
        return ticket.generation && now.generation == ticket.generation && now.source == ticket.source &&
            now.session == ticket.session && now.manager == ticket.manager;
    }
    bool retire(Ticket ticket) noexcept {
        if (!matches(ticket)) return false;
        auto expected = ticket.generation;
        if (!state_.compare_exchange_strong(expected, 0)) return false;
        retired_.store(ticket.generation);
        return true;
    }
    // The last generation whose native join completed normally; zero after poisoning.
    uint64_t retired() const noexcept { return retired_.load(); }
    void invalidate() noexcept { state_.store(3); retired_.store(0); }
    // Caller must have quiesced all publishing/observing hooks.
    void reset() noexcept { state_.store(0); retired_.store(0); }
private:
    std::atomic<uint64_t> state_{}, next_{1}, source_{}, session_{}, retired_{};
    std::atomic<uintptr_t> manager_{};
};
// One prospective source per verified mode1 poll. The lock covers only these scalar
// transitions; it is never held across a native call or a host callback, and it never
// orders native producers or consumers. Unadopted (pending) and adopted (owned) slots are
// separate because the next late poll legitimately overlaps the owning tick's update.
class ProspectiveSources {
public:
    enum Phase : uint32_t { none, polling, sampled, adopted, bound };
    struct Reservation {
        uint64_t source{}, predecessor{}, session{}, job_generation{}, poll_generation{};
        Phase phase{none};
        bool produced{};
        uint32_t kinds{};
    };
    struct Reserve { Reservation reserved{}; uint64_t withdrawn{}; bool overlap{}; };
    // Before the native poll. A pending poll still in flight means two polls overlap: it is
    // withdrawn and no new reservation is made. A sampled, unadopted one is replaced.
    Reserve reserve(uint64_t source, uint64_t predecessor, uint64_t session, uint64_t job_generation,
        uint64_t poll_generation) {
        Reserve out{};
        if (!source || !session || !job_generation || !poll_generation) return out;
        std::lock_guard lock(guard_);
        if (pending_.phase != none) {
            out.withdrawn = pending_.source; out.overlap = pending_.phase == polling; pending_ = {};
            if (out.overlap) return out;
        }
        pending_ = {source, predecessor, session, job_generation, poll_generation, polling, false, 0};
        out.reserved = pending_;
        return out;
    }
    bool still_reserved(uint64_t source) const {
        std::lock_guard lock(guard_); return source && pending_.source == source && pending_.phase == polling;
    }
    bool sample(uint64_t source, bool produced, uint32_t kinds) {
        std::lock_guard lock(guard_);
        if (!source || pending_.source != source || pending_.phase != polling) return false;
        pending_.phase = sampled; pending_.produced = produced; pending_.kinds = kinds;
        return true;
    }
    // Any other native poll, a refused sample or a state change: returns the withdrawn source.
    uint64_t withdraw_pending(uint64_t only = 0) {
        std::lock_guard lock(guard_);
        if (pending_.phase == none || (only && pending_.source != only)) return 0;
        const auto source = pending_.source; pending_ = {}; return source;
    }
    // Tick entry: only a sampled, produced poll whose native job generation has joined.
    Reservation adopt(uint64_t session, uint64_t joined_job_generation) {
        std::lock_guard lock(guard_);
        if (owned_.phase != none || pending_.phase != sampled || !pending_.produced || pending_.session != session ||
            !joined_job_generation || pending_.job_generation != joined_job_generation) return {};
        owned_ = pending_; owned_.phase = adopted; pending_ = {};
        return owned_;
    }
    // Proven consuming branch: the completed control state must still be this poll's.
    bool bind(uint64_t source, uint64_t completed_poll_generation) {
        std::lock_guard lock(guard_);
        if (!source || owned_.source != source || owned_.phase != adopted ||
            completed_poll_generation != owned_.poll_generation) return false;
        owned_.phase = bound; return true;
    }
    Reservation owned() const { std::lock_guard lock(guard_); return owned_; }
    // Tick end, on the owning thread.
    Reservation release(uint64_t source) {
        std::lock_guard lock(guard_);
        if (!source || owned_.source != source) return {};
        const auto value = owned_; owned_ = {}; return value;
    }
    void reset() { std::lock_guard lock(guard_); pending_ = owned_ = {}; }
private:
    mutable std::mutex guard_;
    Reservation pending_{}, owned_{};
};
// Why a bound tick cannot own its adopted poll's consumption; none when it can.
// queued_move: player +0xBEC bit 0x40 or +0x692 at tick entry, the only gates under which
// 0x743090 (before the bind) calls anything, including camera update 0x7468b0 via 0x75ed50.
// consumed_before_bind: a hooked player/movement/camera consumer already ran this tick.
// free_camera: PlayerCamera +0x28 == +0xd0, the exact test by which 0x659ea0 updates the camera
// through 0x746a10 outside the 0x745200 envelope.
enum class BindRefusal : uint32_t { none, unreadable, queued_move, consumed_before_bind, free_camera };
constexpr BindRefusal bind_refusal(bool readable, bool queued_move, bool consumed_before_bind, bool free_camera) noexcept {
    return !readable ? BindRefusal::unreadable : queued_move ? BindRefusal::queued_move :
        consumed_before_bind ? BindRefusal::consumed_before_bind : free_camera ? BindRefusal::free_camera :
        BindRefusal::none;
}
// Observes native producers without locking or changing their execution order.
// A stamp is evidence of an unchanged completed poll, never a pacing token.
class InputOwnership {
public:
    struct Stamp { uint64_t revision{}, generation{}; };
    struct Poll { uint64_t generation{}, conflicts{}; bool exclusive{}; };
    Poll begin_poll() noexcept {
        begin_write();
        const auto conflicts = conflicts_.load();
        const bool exclusive = polls_.fetch_add(1) == 0;
        if (!exclusive) conflicts_.fetch_add(1);
        return {next_.fetch_add(1), conflicts, exclusive};
    }
    void end_poll(Poll poll, bool produced) noexcept {
        completed_.store(produced && poll.exclusive && poll.conflicts == conflicts_.load() ? poll.generation : 0);
        polls_.fetch_sub(1);
        end_write();
    }
    bool owns_poll(Poll poll) const noexcept {
        return poll.generation && poll.exclusive && polls_.load() == 1 && poll.conflicts == conflicts_.load();
    }
    void invalidate() noexcept { begin_write(); conflicts_.fetch_add(1); completed_.store(0); end_write(); }
    void begin_write() noexcept { writers_.fetch_add(1); revision_.fetch_add(1); }
    void end_write() noexcept { revision_.fetch_add(1); writers_.fetch_sub(1); }
    Stamp snapshot() const noexcept {
        Stamp stamp{revision_.load(), completed_.load()};
        return unchanged(stamp) ? stamp : Stamp{};
    }
    bool unchanged(Stamp stamp) const noexcept {
        return stamp.generation && !writers_.load() && revision_.load() == stamp.revision;
    }
private:
    std::atomic<uint64_t> next_{1}, revision_{}, completed_{};
    std::atomic<uint64_t> conflicts_{};
    std::atomic<uint32_t> writers_{}, polls_{};
};
}
