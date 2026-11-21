// SPDX-License-Identifier: MIT
#pragma once
#include <cstddef>
#include <cstdint>

namespace rsf::skyrim {
enum class CpuProofStorage { process };

// The process uses Rust statics. The contract fixture owns a separate allocation.
class NativeJobOwnership {
public:
    struct Ticket { uint64_t generation{}, source{}, session{}; uintptr_t manager{}; };
    NativeJobOwnership() noexcept;
    constexpr explicit NativeJobOwnership(CpuProofStorage) noexcept {}
    ~NativeJobOwnership();
    NativeJobOwnership(const NativeJobOwnership&) = delete;
    NativeJobOwnership& operator=(const NativeJobOwnership&) = delete;
    Ticket publish(uintptr_t manager, uint64_t source, uint64_t session, bool native_idle = false) noexcept;
    Ticket snapshot() const noexcept;
    bool matches(const Ticket&) const noexcept;
    bool retire(const Ticket&) noexcept;
    uint64_t retired() const noexcept;
    void invalidate() noexcept;
    // Caller must have quiesced all publishing/observing hooks.
    void reset() noexcept;
private:
    void* store_{};
};
static_assert(sizeof(NativeJobOwnership::Ticket) == 32 && offsetof(NativeJobOwnership::Ticket, manager) == 24);

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
    using Storage = CpuProofStorage;
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
    InputOwnership() noexcept;
    constexpr explicit InputOwnership(CpuProofStorage) noexcept {}
    ~InputOwnership();
    InputOwnership(const InputOwnership&) = delete;
    InputOwnership& operator=(const InputOwnership&) = delete;
    Poll begin_poll() noexcept;
    void end_poll(const Poll&, bool produced) noexcept;
    bool owns_poll(const Poll&) const noexcept;
    void invalidate() noexcept;
    void begin_write() noexcept;
    void end_write() noexcept;
    Stamp snapshot() const noexcept;
    bool unchanged(const Stamp&) const noexcept;
private:
    void* store_{};
};
static_assert(sizeof(InputOwnership::Stamp) == 16 && sizeof(InputOwnership::Poll) == 24);
static_assert(offsetof(InputOwnership::Poll, exclusive) == 16);

// Stack record lent by the native PollDevices detour. Its TLS pointer is saved and
// restored by the detour, including for nested polls.
struct PollScope {
    InputOwnership::Poll ticket{};
    uintptr_t manager{};
    bool produced{};
    uint32_t kinds{};
};
static_assert(sizeof(PollScope) == 40 && offsetof(PollScope, kinds) == 36);

enum class PollSample : uint32_t { none, not_produced, ready };

enum class CpuConsumer : uint32_t { movement, player_update, camera, player_camera };
struct WorkerCounts { uint32_t movement{}, player_update{}, camera{}, mismatched{}; };
static_assert(sizeof(WorkerCounts) == 16);
}
