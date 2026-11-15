# Skyrim prospective input source: reservation and delayed binding, 15 November 2026

Continues [Skyrim native input ownership](skyrim-input-ownership-20261115.md), whose
"Prospective source at native poll" and "Mode1 generation ticket" sections hold the
native task graph this work relies on. Executable SHA256
`846efccf0c1374d71f892907f46549560f2fcb0a75cb87a3eed438baa0f1402f`, matching database
`.local/skyrim-research/SkyrimSE-1.7.104-research.i64` (IDA session `2b8897f9`).

## Question

The verified late poll (named callback `0x6e6030`, PollDevices `0xcf95f0`) runs on a
worker in manager generation G(n), submitted by rendered source S. Its controls are
consumed by a later tick. How can the runtime retain that actual poll as the input of
the consuming source P without moving or dropping any native poll, without attaching it
to S, and without inventing INPUT_SAMPLE, SIMULATION_BEGIN or Present markers? What
dependency on S's actual Present can later pacing use without the documented cycle
through S's FRAME_END?

## Why this approach

Existing CPU events cannot express a source that is sampled before it begins: version 1
INPUT_SAMPLE belongs to an open FRAME_BEGIN, and rendered-source mode refuses it. Reusing
those stages with guessed identities would be a false input timeline. Rule 3 requires an
append-only extension of the existing event interface, with the core deciding meaning.

## New native evidence (static)

- Main tick `0x658870` calls `0x743090` at `0x6588de` (return `0x6588e3`) on every tick
  that passes the Alt+Tab check, before the pause decision. It is not a gameplay-only branch
  and is therefore not used as the binding point.
- The pause/freeze decision computes r15b at `0x658940`..`0x658962` (UI pause count
  `+0x160` and Main `+0x16`). `0x658aed` skips the submit when r15b is zero; otherwise
  `0x658b0f` calls submit `0x60eaf0`, return `0x658b14`. Reaching that call proves the
  normal-simulation branch, and it precedes every worker callback of the new generation.
- The "Main update player" group (table `0x20b4d60`) callback `0x6e5040` runs actor process
  lists through `0x783130` on the job list and signals event4; `0x6e5010` waits bank0/3 and
  bank1/2. Player movement consumption therefore runs on workers inside the new generation,
  after the submit. Reaching `0x745200`/`0x776290` from `0x783130` is a static lead only.
- `rsf_observer_present_event.result` carries the actual Present HRESULT after the real call
  returned; this is the completion fact used below.

## Contract (generic, game ABI unchanged)

`rsf_game_cpu_event` and `RSF_GAME_CPU_EVENT_VERSION` (1) are unchanged, so every existing
client and equality check is untouched and no game ABI bump is needed. New stages travel
through the same `cpu_event` callback only as `rsf_game_cpu_source_event` (header first,
`header.struct_size` covering the extension, `event_version` 2):

| Stage | Meaning |
| --- | --- |
| 7 SOURCE_RESERVE | Before the unchanged native poll: future identity P, predecessor S, opaque poll generation, OVERLAPS_PREDECESSOR |
| 8 SOURCE_SAMPLED | After the poll returned: delivered input kinds, PRODUCED when it exclusively completed control state |
| 9 SOURCE_BIND | At the proven consuming branch of the open source P |
| 10 SOURCE_ABORT | Explicit withdrawal with a reason |

Version 2 on stages 0..6 and versions 0/1 on stages 7..10 are refused. A runtime predating
the extension refuses version 2. The core checks `struct_size` before reading any extension
byte; a test places packets against a no-access page to prove it.

Core (`native_cpu.cpp`) keeps reservations in its own ledger. They never reach a dispatch
sink, never set a frame stage, marker or pacing time, and are not visible to `native_fg` or
the Vulkan endpoints. Rules:

- RESERVE needs the established session and frequency; P must exceed every begun and every
  reserved identity (no reuse, including after abort); a nonzero predecessor must be a begun
  source. A reservation while another poll is still in flight withdraws it (OVERLAP) and is
  refused. A sampled, unadopted reservation is replaced (INTERVENING_POLL).
- FRAME_BEGIN with P adopts a sampled reservation, recording that actual input predates the
  frame. Any other identity supersedes an unadopted reservation. A frame whose reservation
  was withdrawn or is still polling begins as an ordinary frame without input ownership:
  the adapter cannot learn about a refusal through a void callback, and losing the engine's
  CPU frame would be worse than an unowned one.
- BIND requires ADOPTED, PRODUCED, a live unfailed frame P, and a timestamp at or after both
  frame begin and sample. FRAME_END closes BOUND as CLOSED and ADOPTED as UNBOUND (paused
  or frozen tick). Release/session end drops everything.
- `rsf_native_cpu_present_result(session, source, outcome, hresult)` records the actual
  application Present outcome: RETURNED with HRESULT (success or failure), RESULT_UNKNOWN
  (unverified, never success) or ABORTED. A FRAME_END with RENDER_SKIPPED records cancelled.
  CPU FRAME_END alone resolves nothing. Facts are keyed by session and source; a reused
  number from another session is refused, and session release clears them.
- `rsf_native_cpu_reservation_read` reports the reservation and, as `predecessor_present`,
  S's actual Present outcome at read time: pending, presented, failed, unverified,
  cancelled, or superseded by a later successful source. This is the dependency fact a
  future core pacing decision needs; no wait primitive or pacing caller is added until
  input ownership is proven (an earlier draft's bounded wait was removed as unused).
  `rsf_native_cpu_reservation_counters_get` reports totals.

## Skyrim adapter wiring

- PollDevices hook: inside the verified mode1 named-callback scope (`cpu_poll_provenance`),
  allocate P from the same counter as tick sources, report RESERVE with predecessor = the
  submitting source S, call the original unchanged (`manager`, XMM1 timestep), then report
  SAMPLED with kinds from the dispatch hook and PRODUCED only if this exact poll left the
  completed, unchanged control state; otherwise ABORT(NOT_PRODUCED). A scheduled late poll
  no longer emits INPUT_SAMPLE for whichever main tick happened to run it.
- Every other poll (mode2 loop, paused main poll, nested/unverified) keeps native behavior and
  withdraws a pending reservation with ABORT(INTERVENING_POLL).
- Tick entry adopts P only when it was sampled and produced in this session and its native job
  generation is the one whose join completed normally (`NativeJobOwnership::retired`).
  Otherwise the pending reservation is withdrawn (STATE_CHANGED) and a fresh identity is used.
- Submit hook (return `0x658b14`, idle modes, matching Main): BIND before the original wakes
  workers, only if the completed control-state generation is still P's poll generation;
  otherwise ABORT(STATE_CHANGED). The job ticket of this generation carries source P.
- Movement, player-update and camera hooks off the main thread count call envelopes only
  inside the bound source's published job generation, and count a different completed poll
  generation as a mismatch. These are logged for the first 16 adopted sources and are never
  reported as consumption.
- Quiesce/start reset the adapter state without events; the runtime closes reservations at
  session release.

## What this does not establish

- No INPUT_TIMELINE, pacing, SDK sleep, marker, DLSS or XeSS admission changed. The host
  still declares FG_INPUTS|RENDERED_SOURCE only.
- Binding asserts ownership of one completed poll at the gameplay branch. It does not
  prove that `0x743090` (before the bind) or every later worker consumer read only that
  poll, nor complete virtual movement/derived-state coverage (`+0x44` writers, look handler
  actor rotation, free/first-person camera).
- The coordinator wired the D3D11 bridge actual-Present completion to forward its
  real HRESULT with session/source copied at begin, independently of FRAME_END.
  The new CPU reservation test is registered in CMake. These integration edits
  await the final build/check and bounded game batch.

## Validation

Source and static review only in this task: no build, test run, game, GPU fixture or
deployment. New focused test `tests/native_cpu_reservation.cpp` covers overlapping S render
and P poll, delayed adoption and binding, refusals (no session, stale session/frequency,
reuse, overlap, intervening poll, unsampled adoption, unproduced, aborted, version prefix,
short packets at a guard page), abort and session release, and predecessor Present
outcome recorded before and independently of FRAME_END, including cross-session refusal,
failed, unverified, cancelled and superseded outcomes. `tests/skyrim_input.cpp` adds
`ProspectiveSources` and `NativeJobOwnership::retired` cases.

Residual acceptance: build and run both tests; a bounded game run whose log shows
reservations sampled, adopted and bound in steady gameplay, withdrawn on pause, menus and
mode2, with worker movement/player-update counts and zero mismatches before any
INPUT_TIMELINE work; a bounded probe of `0x743090` reads of PlayerControls; coverage of the
remaining derived-state writers.
