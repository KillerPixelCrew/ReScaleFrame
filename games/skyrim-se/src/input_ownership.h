// SPDX-License-Identifier: MIT
#pragma once
#include <cstddef>
#include <cstdint>

namespace rsf::skyrim {
struct JobTicket { uint64_t generation{}, source{}, session{}; uintptr_t manager{}; };
static_assert(sizeof(JobTicket) == 32 && offsetof(JobTicket, manager) == 24);

enum class SourcePhase : uint32_t { none, polling, sampled, adopted, bound };
struct SourceReservation {
    uint64_t source{}, predecessor{}, session{}, job_generation{}, poll_generation{};
    SourcePhase phase{};
    bool produced{};
    uint32_t kinds{};
};
struct SourceReserve { SourceReservation reserved{}; uint64_t withdrawn{}; bool overlap{}; };
static_assert(sizeof(SourceReservation) == 56 && sizeof(SourceReserve) == 72);
static_assert(offsetof(SourceReservation, phase) == 40 && offsetof(SourceReservation, kinds) == 48);

// Why a bound tick cannot own its adopted poll's consumption; none when it can.
// queued_move: player +0xBEC bit 0x40 or +0x692 at tick entry, the only gates under which
// 0x743090 (before the bind) calls anything, including camera update 0x7468b0 via 0x75ed50.
// consumed_before_bind: a hooked player/movement/camera consumer already ran this tick.
// free_camera: PlayerCamera +0x28 == +0xd0, the exact test by which 0x659ea0 updates the camera
// through 0x746a10 outside the 0x745200 envelope.
enum class BindRefusal : uint32_t { none, unreadable, queued_move, consumed_before_bind, free_camera };
// A stamp is evidence of an unchanged completed poll.
struct InputStamp { uint64_t revision{}, generation{}; };
struct PollTicket { uint64_t generation{}, conflicts{}; bool exclusive{}; };
static_assert(sizeof(InputStamp) == 16 && sizeof(PollTicket) == 24);
static_assert(offsetof(PollTicket, exclusive) == 16);

// Stack record lent by the native PollDevices detour. Its TLS pointer is saved and
// restored by the detour, including for nested polls.
struct PollScope {
    PollTicket ticket{};
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
