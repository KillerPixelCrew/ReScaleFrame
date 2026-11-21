// SPDX-License-Identifier: MIT
#pragma once
#include <atomic>
#include <cstddef>
#include <cstdint>

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
// Native view of the Rust source store. The fixture owns an independent store.
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
    enum class Storage { process };
    ProspectiveSources() noexcept;
    constexpr explicit ProspectiveSources(Storage) noexcept {}
    ~ProspectiveSources();
    ProspectiveSources(const ProspectiveSources&) = delete;
    ProspectiveSources& operator=(const ProspectiveSources&) = delete;
    Reserve reserve(uint64_t source, uint64_t predecessor, uint64_t session, uint64_t job_generation,
        uint64_t poll_generation) noexcept;
    bool still_reserved(uint64_t source) const noexcept;
    bool sample(uint64_t source, bool produced, uint32_t kinds) noexcept;
    uint64_t withdraw_pending(uint64_t only = 0) noexcept;
    Reservation adopt(uint64_t session, uint64_t joined_job_generation) noexcept;
    bool bind(uint64_t source, uint64_t completed_poll_generation) noexcept;
    Reservation owned() const noexcept;
    Reservation release(uint64_t source) noexcept;
    void reset() noexcept;
private:
    void* store_{};
};
static_assert(sizeof(ProspectiveSources::Reservation) == 56 && sizeof(ProspectiveSources::Reserve) == 72);
static_assert(offsetof(ProspectiveSources::Reservation, phase) == 40 && offsetof(ProspectiveSources::Reservation, kinds) == 48);
// Why a bound tick cannot own its adopted poll's consumption; none when it can.
// queued_move: player +0xBEC bit 0x40 or +0x692 at tick entry, the only gates under which
// 0x743090 (before the bind) calls anything, including camera update 0x7468b0 via 0x75ed50.
// consumed_before_bind: a hooked player/movement/camera consumer already ran this tick.
// free_camera: PlayerCamera +0x28 == +0xd0, the exact test by which 0x659ea0 updates the camera
// through 0x746a10 outside the 0x745200 envelope.
enum class BindRefusal : uint32_t { none, unreadable, queued_move, consumed_before_bind, free_camera };
BindRefusal bind_refusal(bool readable, bool queued_move, bool consumed_before_bind, bool free_camera) noexcept;
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
