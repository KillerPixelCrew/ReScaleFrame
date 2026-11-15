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

## Consumer coverage and pre-input boundary, continuation (static, IDA session `77f0f198`)

Question: why did the bounded run (PID 41756) log bound sources with
worker_movement=1 but worker_player_update=0 and worker_camera=0, and where is the
earliest safe pre-input boundary?

- PlayerCharacter vtable is `0x19296c0` (PE RTTI: COL `0x1c06598`, meta pointer
  `0x19296b8`). `0x745200` is slot +0x568; the movement wrapper `0x746c30` is +0x8d0.
- Main tick calls `0x659ea0` at `0x658b17`, immediately after the gameplay submit at
  `0x658b0f`. Its normal branch calls player vfunc +0x568, i.e. `0x745200`, at
  `0x659f4d` on the main thread. `0x745200` reads PlayerControls at `0x745844` and calls
  `0x7468b0` (`0x745c80`/`0x74615e`), which signals bank/event2 through `0xec1d30(2)` and
  then runs PlayerCamera update `0x8f8680` -> TESCamera::Update `0x558e70` (PlayerCamera
  vtable `0x196d980` slot 2) -> current camera state +0x18. Player update and camera look
  therefore consume P on the main thread inside the bound tick, after the bind. That is why
  the worker counters showed 0. Only movement through `0x746c30` ran on a worker.
- When the current camera state is PlayerCamera +0xd0 (free camera), `0x659ea0` calls
  `0x746a10` -> `0x558e70` directly, without `0x745200`. When paused, it calls `0x743c10`.
- Camera states reached this way: ThirdPersonState/Dragon/Horse `0x8fd590` (hooked),
  first person `0x8f47a0` (vtable `0x196d908`), and `0x8f66f0` (vtable `0x196dae8`). The
  last two also call derived-state writer `0x8fb2e0` and are not hooked. The `0x745200`
  envelope covers them in the normal branch, but not on the free-camera branch.
- `0x558e70` calls state +0x18 with `(state, &next)`. In `0x8fd590`, every path writes RDX
  before reading it (`0x8fd5b3`, or calls at `0x8fd5d5`/`0x8fd60f`), so the one-pointer hook
  is ABI-safe.
- `0x743090` (before the bind, every tick) has no direct PlayerControls reference. Its
  transitive callees were not exhaustively proven.
- Ordering: the next late poll P' runs in the Main render end group. That group is
  submitted only after group 5 completes, which waits for event7 from main's World render.
  World render runs in RenderEntry after `0x659ea0` returns. So the main-thread
  player/camera consumption of P strictly precedes P'. Worker movement (Main update player
  group) also precedes P' by manager group order.

Change: the existing prospective log line now also reports main-thread player-update,
camera and movement counts, whether the main-thread stamp generation equals the bound
poll generation, and ambiguity. No new hook, ledger or event.

Earliest safe pre-input boundary: the scheduled poll hook on the worker, inside
`0x6e6030` scope, before the original PollDevices (the existing SOURCE_RESERVE point).
Main never waits on the Main render end group before its join `0x65932f`. S's Present
(`0x100a012`) is not ordered after this callback. Only the manager's group completion and
then Main's join wait on it. A sleep there therefore delays P's input and tick n+1 without
blocking S's Present. Any later point (FRAME_BEGIN, the bind) is after P's sample and
would be a late sleep. Pacing there needs a narrow core change, not implemented: accept a
bound reservation's reserve QPC as the INPUT_SAMPLE of an INPUT_KEYED frame, and acquire
the vendor frame token for P at RESERVE (today `frame_acquire` runs at FRAME_BEGIN in
`native_fg.cpp`), with the sleep inside RESERVE and abort on withdrawal. (Corrected below:
the reserve timestamp is not the sample time, and SIMULATION_START does not belong at RESERVE.)

Remaining proof: a bounded run printing main_player_update>0, main_generation_match=1,
main_ambiguous=0 for bound sources; free-camera and first-person camera hooks or explicit
refusal; transitive PlayerControls reads under `0x743090`; then the core contract above.

## Coverage completion and sample-time contract (static, IDA session `77f0f198`)

`0x743090` (before the bind): the only branches that call anything are `test [player+0xBEC],
0x40` at `0x7430ed` and `cmp [player+0x692], 0` at `0x74314e` (jump to epilogue `0x743b0c`).
With both clear it makes no call and reads no PlayerControls. When set it handles a queued
move, which reaches `0x75ed50` -> `0x7468b0` (PlayerCamera update) before the bind. The tick
hook reads both gates of player `0x3230778`. Only `0xec1c40` and GetAsyncKeyState run
between that read and the native test on the same thread. A set gate refuses the bind.

Camera states (PE RTTI): `0x8f47a0` is FirstPersonState (COL type `0x214b8e0`), `0x8f66f0` is
FurnitureCameraState (`0x214bba0`), `0x8fd590` is Third/Dragon/Horse. In the normal branch
all run inside the hooked `0x745200` envelope through `0x7468b0`, so `main_player_update`
covers first person without a separate hook. The free-camera branch of `0x659ea0` is taken
when PlayerCamera (`[0x31a5478]`) `+0x28 == +0xd0`, and it updates through `0x746a10`
outside that envelope. The submit hook evaluates exactly this test and refuses the bind.
It also refuses when any hooked player, movement or camera consumer already ran on the main
thread before the submit, or when the gates are unreadable. Refusal emits ABORT(NOT_CONSUMED)
and the log prints `refusal=` (1 unreadable, 2 queued move, 3 consumer before bind,
4 free camera). Pure decision `bind_refusal` is tested in `tests/skyrim_input.cpp`.

Sample-time correction. RESERVE is emitted inside the poll hook before the original
PollDevices. Any future runtime sleep would run inside that synchronous callback, so the
RESERVE timestamp marks the reservation opening, before such work. SAMPLED is emitted after
PollDevices returns. Neither is the physical sample. `rsf_game_cpu_source_event` now appends
`poll_begin_qpc` (`RSF_GAME_CPU_SOURCE_EVENT_SIZE_1` marks the first version). The adapter
reads QPC after RESERVE returned and after the existing non-scheduled INPUT_SAMPLE branch,
immediately before the original PollDevices call, and reports it in SAMPLED. The poll
diagnostic log was moved ahead of RESERVE so nothing but that read separates the callback
from the poll. The core requires reserve <= poll_begin <= sampled. It stores the value in
`rsf_native_cpu_reservation.poll_begin_qpc` and reads a first-version packet only to its
size, leaving the sample time unknown.

Simulation begin is not at RESERVE: P's native simulation starts in its own tick after
FRAME_BEGIN. The first consumer-capable call is `0x743090` at `0x6588de` on queued-move ticks,
and otherwise the normal-branch work after the submit. Marker semantics remain core policy.
The native ordering facts for a future core are: sleep boundary = inside RESERVE (worker,
before the poll); input sample = `poll_begin_qpc`; simulation begin = the consuming tick,
never before its FRAME_BEGIN.

Residual: transitive PlayerControls reads by the other pre-submit main-thread callees
(`0x65c730`, `0x625b90`, `0x6e4a90`, `0x645d60`, `0x642750`, scene-graph vfunc `+0x200` at
`0x658ac1`) are not proven. The hooked-consumer refusal covers player update, movement and
third-person camera only. Other callers of `0x7468b0`/`0x558e70` (`0x750450`, `0x7600f0`)
were not placed in the tick.

## Pre-submit callee reachability and timeline specification (static, IDA session `4708573f`)

Each main-thread call between tick entry and the gameplay submit was decompiled:

| Call | Identity (evidence) | PlayerControls/derived reads |
| --- | --- | --- |
| `0x65c730` at `0x6588e3` | walks the high-actor array `0x3230788`, counts actors via vfunc +0x380 (player: `0x747af0`, a 3D/state query) | none |
| `0x204dd0` | drains a deferred reference queue through `0x204ba0` | none found; not an input path |
| `0x625b90` at `0x65890a` | save/load request queue processor (BSTCommonStaticMessageQueue<bgs::saveload::Request>) | only when a load is processed: `0x61c2f0` (LoadGlobalData) -> `0x61d5f0` -> `0x75ed50` -> camera update `0x7468b0` |
| `0x6e4a90` | per-thread bit in `0x3244170` | none |
| `0x645d60` | calendar/game-hour advance | none |
| `0x642750` | timed list expiry | none |
| `0x3230740` vfunc +0x200 at `0x658ac1` | Main-constructed (`0x652600`) world SceneGraph; SceneGraph vtable `0x1914b00` slot +0x200 = `0xff4bc0`, frame-time camera frustum adjustment | none |
| `[0x331f570]` vfunc +0x80 at `0x658a3e` | refcounted handle/task manager referenced from ~140 actor/sound/UI sites; target unresolved statically | none known |
| `0x743090` at `0x6588de` | queued-move processor; gated by player +0xBEC&0x40 / +0x692 (refused when set) | only on the gated path |

Other camera callers: `0x7600f0` is called only from `0x745200` (inside the envelope).
`0x750450`, PlayerCharacter vfunc +0x88, takes a load-form buffer and runs at load, not in the
ordinary tick.

Envelope correction (implemented): `0x75ed50` (queued move and load) and `0x746a10` (free
camera) reach PlayerCamera TESCamera::Update `0x558e70` outside `0x745200`. In first person
the only hooked state update, third-person `0x8fd590`, never runs. `0x558e70` is now
hooked with guard `40 57 48 83 ec 30 48 c7 44 24 20 fe ff ff ff 48 89 5c 24`. It takes `this`
in RCX and returns void; RDX is set at `0x558eaf` before the state call. It counts only
when `this` equals the PlayerCamera singleton `[0x31a5478]` on the owning main tick, using
the same unchanged-stamp check (`main_player_camera` in the log). Any such update before the
submit refuses the bind as consumed_before_bind. The free-camera mirror and queued-move
gate refusals are kept: the free path also skips player update, and the gate covers
non-camera queued-move work.

Prior checkpoint: timeline proposal for an ordinary covered gameplay tick (bound,
refusal none, main_generation_match=1, mismatched=0). At this checkpoint step 3 was
emitted only after successful BIND, and INPUT_TIMELINE remained disabled pending
runtime acceptance. The implementation and acceptance updates below supersede
those restrictions; this proposal is retained as historical context.

1. Input sample: SAMPLED `poll_begin_qpc` of P (worker, late group of generation n, after
   RESERVE returned). The pacing sleep belongs inside RESERVE.
2. FRAME_BEGIN(P): tick entry (existing).
3. SIMULATION_BEGIN(P): at the bind, immediately before the original submit `0x60eaf0`
   wakes generation n+1. Everything earlier in the tick (calendar, scene-graph frustum,
   queues; `0x743090` inert when its gates are clear) consumes no controls. Actor/player
   simulation (workers' process lists, main `0x659ea0`/`0x745200`, camera) starts here.
   At that checkpoint, after a successful SOURCE_BIND in the guarded `jobs_submit` hook, the
   adapter emits the existing keyed v1 SIMULATION_BEGIN, before the original submit wakes
   workers. It does so only for bound covered frames; refused or unbound frames emit none.
   Core acceptance: in rendered mode SIMULATION_BEGIN on mask 1 is accepted as
   `early_simulation`, SIMULATION_END at RenderEntry yields mask 13, and FRAME_END accepts
   mask 13. Outside rendered mode, the substitute INPUT_SAMPLE on mask 5 is accepted as
   `input_after_begin`. No hook, ledger, ABI or policy change.
4. SIMULATION_END(P): existing, at RenderEntry `0x6565b0`, after `0x659ea0` returned. World
   render's event7 is required before the late poll group can start, so all main-thread
   consumption of P precedes P'.
5. Present: the actual application Present of P's frame (core already records the
   session/source HRESULT).

The proposal required shared-core work before adapter capability activation: a frame whose reservation is BOUND
satisfies INPUT_SAMPLE with `poll_begin_qpc` (predating FRAME_BEGIN) instead of requiring a
v1 INPUT_SAMPLE; the vendor frame token for P is acquired at RESERVE; the single sleep runs
inside the RESERVE callback; an unbound, aborted or refused reservation falls back to the
existing substitute path. Only then would Skyrim declare INPUT_TIMELINE. That requires a
bounded run showing the counters above for steady gameplay, including first person
(`main_player_camera>0`).

### Final pre-submit target resolved

Offline inspection of the matching 1.7.104 IDA database (session `47c19d47`)
resolved `[0x331f570]+0x80` as IOManager's pure field getter `0xfc4fa0`:
it returns the uint32 at `this+0x30` without a call. TES initialization
`0x19ef60` allocates 296 bytes, constructs the singleton through `0xfc6710`
with IOManager vtable `0x1a68fd8`, and stores it at `0x19f41f`. The only other
store is the null assignment at `0x19fea3` in TES destruction `0x19f8d0`.
Vtable slot `0x1a69058` is the getter. The tick branch at `0x658a46` reaches
`0x63bf40/0x63bf70`, which toggle `[0x331c370]+0x20` through
`0xed8180/0xed8250`; no PlayerControls reference or known consumer was found.

This closes the unresolved static pre-submit callee in the table above. It
does not replace runtime consumption evidence. Ordinary covered ticks still
need observed bound source ownership, main player/camera updates, matching
generation and zero mismatches; queued-move, load-camera and free-camera
exceptions retain their existing refusal gates. No new hook or game run was
used to resolve this target.

### First-person ownership acceptance and timeline connection

After the user released the capture session, the coordinator ran the verified
`54235df` DLLs for one bounded QASmoke session, PID 45008. All 16 retained
prospective records (sources 2 through 17) report bound=1, refusal=0,
main_player_update=1, main_player_camera=1, main_generation_match=1,
main_ambiguous=0 and mismatched=0. main_camera=0 distinguishes the first-person
envelope from the third-person state hook. Worker movement was also observed.
The game exited normally with code 0; original DLL/settings hashes were
restored. Receipts and parsed assertions are under ignored
`input-acceptance-20261115-231108/`.

Together with the static pre-submit graph and guarded refusal paths, these
facts supplied the prerequisite for connecting the adapter's INPUT_TIMELINE
capability. The first connection emitted SIMULATION_BEGIN after successful BIND
and declared FG_INPUTS|INPUT_TIMELINE instead of RENDERED_SOURCE. The shared
runtime owns the actual pre-poll sleep, tokens and marker mapping. The PID 45008
run used the earlier observation-only capability and does not demonstrate a
sleep, DLSS/XeSS FG or lower latency.

### Bounded timeline connection acceptance before the latest fixes

The subsequent `input-acceptance-20261115-231558/` run used the timeline-connected
entry DLL with runtime SHA256
`A4C742CB3C17C2FE0999EE84EA6E16770CF057643D67F6DBC671D1B22A588D31`,
entry SHA256 `90834FD03943CFA9DD32C125E3CE70B8EE43EBDD6E164CE884B6A2FFD7C626AD`,
and PID 42264. Its retained loader log records DLSS FG effective mode=1,
generated=1, active=1, status=0, Reflex mode=1 applied and available driver
reports. It also records transitions through inactive FG states, so this is
bounded evidence of active DLSS FG and Reflex, not continuous admission for
every frame class or a measured reduction in latency. The observation completed,
the game exited with code 0 and all three DLL/settings restorations were verified.

Both this run and the preceding first-person ownership run
(`input-acceptance-20261115-231108/`, PID 45008) predate the latest CPU lifecycle
and refused-reservation fixes. They do not validate those fixes, XeSS FG, or the
free-camera/load/withdrawal cases excluded by ownership checks.

Current source emits the actual keyed SIMULATION_BEGIN on every verified normal
submit, including refused or unbound input, before the original submit wakes
workers. This is an engine fact independent of successful SOURCE_BIND.
For a refused prospective reservation the core preserves real simulation facts
without fabricating INPUT_SAMPLE or sending those simulation facts to the
latency sink; see [prospective source refusal](prospective-source-refusal-20261115.md).
Frames without a prospective record retain the explicitly labelled substitute
path described there. Neither path establishes measured FG admission for
excluded frames. These source corrections await the coordinator's current gate
and any separately authorized bounded runtime validation.
