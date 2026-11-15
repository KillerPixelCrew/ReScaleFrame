# Skyrim native input ownership, 15 November 2026

Question: can the six-device poll move before the first control-consuming update,
with only redundant polls of the same owned generation suppressed?

Static analysis used the existing matching Skyrim 1.7.104 database,
`.local/skyrim-research/SkyrimSE-1.7.104-research.i64`, executable SHA256
`846efccf0c1374d71f892907f46549560f2fcb0a75cb87a3eed438baa0f1402f`.
The former IDA session was no longer adopted; reopening the existing worker
returned session `07c59a31`. No game, GPU fixture, deployment or build ran.

## Concrete ABI repair

Disassembly, unlike the incomplete decompiler prototype, proves PollDevices
RVA `0xcf95f0` takes `(manager, float timestep)` in RCX/XMM1. Instruction
`0xcf9608` saves XMM1 in XMM6; `0xcf961e` forwards it to each device vfunc+0x10,
and `0xcf9648` forwards it in XMM0 to `0xcee700`. Wrapper `0x659df0` loads
XMM1 from global `0x327560c` at `0x659e00` and tail-jumps at `0x659e13`.
The independent manager call at `0x6e4e28` also loads XMM1 at `0x6e4e20`.
The adapter's one-argument hook allowed logging/event callbacks to clobber this
volatile register. Its declaration and trampoline call now preserve the float
argument explicitly. Native return remains pointer-width RAX unchanged.
This repair applies whenever hooks are enabled, including inactive forwarding.

## Why relocation is not yet safe

- Outer tick `0x658870` first calls `0xec1c40` at `0x6588ae`.
  That routine loops native event arrays and calls ResetEvent at `0xec1c87`.
  It does not wait for any input worker. The first successful-branch update
  is PlayerCharacter `0x743090` at `0x6588de`, before gameplay job submission.
- Gameplay submit `0x60eaf0`, called at `0x658b0f`, sets manager+0x68/+0x6c
  and signals manager+0x58. Manager `0x60f1e0` mode 1 submits each list,
  waits its event at `0x60f2ec`, then invokes its completion callback.
  Main joins through `0x60eb80` at `0x65932f`, after render entry `0x659319`.
  Therefore render entry alone does not prove all native jobs have completed.
- A third direct poll caller exists beyond gameplay and paused main polling:
  manager mode 2 invokes `0x6e4c20` at `0x60f27e`. Its loop increments native
  counter `0x33363f8`, draws UI and Presents at `0x6e4dd4`, then polls at
  `0x6e4e28` while manager mode remains 2 (`0x6e4c59`). These are independent
  iterations, not redundant polls of one outer tick. A latest-source atomic
  cannot distinguish them. Suppressing them would lose native input delivery.
- Poll itself dispatches at `0xcf966b`, clears the borrowed queue at
  `0xcf9677`, reads/clears manager+0xf0 and may dispatch a second notification
  through `0xcfa8f0` at `0xcf969f`. A skipped call must preserve its observed
  return and these side effects, not simply return zero.

The mode-2 transition/start/stop synchronization and event-sink prerequisites
before PlayerCharacter update remain unproven. In particular the ordinary
join at tick end cannot by itself exclude an already running mode-2 loop at
the proposed early poll. Adding a mutex only inside adapter poll hooks would
serialize polls but would not serialize all native consumers or mode changes.
Relocation was therefore withheld. No worker input was assigned to the latest
source; NO_FG and unknown screen classification remain unchanged. No new
INPUT_SAMPLE or SimulationBegin boundary was invented.

## Validation and required continuation

`git diff --check` is the focused textual check. The coordinator must build
`rsf_skyrim_input` and `rsf_game_skyrim_se` in Release, run
`ctest --test-dir build/windows-x64 -C Release -R skyrim_input_contract --output-on-failure`,
and run `eng/verify.ps1` after concurrent native edits settle. Existing input
tests validate borrowed event-header parsing, not native timestep delivery or
ownership. The ABI repair is static-evidenced, not game-tested.

Next establish all manager mode transitions and their waits, the immutable
poll-generation owner through submit/completion, and dependencies of all
registered sinks before moving dispatch earlier. A bounded future game probe
must reject overlap, duplicate delivery, stale controls and mode-transition
misassociation. Only then can a real pre-input boundary and gameplay admission
be reported; runtime retains all pacing, vendor and FG policy.

## Continuation: concrete producer/consumer tracking

Further tracing resolves the mode-2 stop, correcting the earlier open question:
`0x60e9c0(char enable)` enables mode 2 only if manager+0x6c is zero, sets
manager+0x68/+0x6c to 2 and signals +0x58. Disable releases an owned UI lock,
sets +0x68 to zero, and waits for manager+0x60 at `0x60ea82`/`0x60eaa0`,
pumping `0x65c3e0` between 1ms waits, before clearing +0x6c. Direct calls
are `0x1a665a` (enable) and `0x1a6941` (disable), both in `0x1a6540`.
The latter also has stored callback references, so these direct calls alone
do not establish all invocations. `0x6e4c20` has one direct caller, manager
`0x60f27e`. The normal submit cannot replace mode 2 while +0x6c is nonzero.
The join mechanism is therefore real; the unresolved point is the safe early
dispatch prerequisites and timing, not the absence of any mode-2 stop.

Timing is concrete too: `0x659d10` calls `0xce36b0` on timer `0x32755f0`.
That function derives seconds from milliseconds (multiply by 0.001), clamps
and optionally smooths the interval, stores the unscaled result at timer+0x1c
(`0xce3944`, global `0x327560c`), and scales +0x18 by the time multiplier.
Normal tick calls timing at `0x65932a`, after rendering, whereas paused tick
calls at `0x658d8f`, before its stock poll. Moving normal polling to `0x743090`
without also moving timing would deliberately retain the prior interval;
moving timing itself would alter every engine timestep consumer and has not
been justified by the sink prerequisite analysis. No such retiming is installed.

The fallback now implements an actual producer-to-consumer observation:

- Every unchanged native PollDevices invocation gets its own monotonic poll
  generation, scoped in TLS through native dispatch and queue destruction.
  No generation is assigned a source merely because an update is concurrent.
- Guarded PlayerControls `0x7ad900` is hooked with RCX owner, RDX borrowed
  list and R8 event-source forwarded unchanged. Only the singleton `0x31a5698`
  processing the exact list passed by dispatch in the same native poll marks
  that generation produced. Early native returns still reset controls before
  returning, so these are completed control-state generations, not proof that
  a nonzero movement event was accepted. No vector values are read or retained.
- Guarded shared camera update `0x8fd590` preserves its one-pointer/RAX
  signature. Its non-disabled path (+0xe0 == 0) calls HandleLookInput with
  singleton+0x2c at `0x8fd5eb`. Before/after revision stamps reject any poll or
  producer overlapping camera execution. A native poll stays active until
  its post-dispatch clear/notification returns. Nested and concurrent polls,
  and writes outside their dispatch scope, invalidate ownership conservatively.
- A consumer records a source only from its own existing TLS outer tick.
  `cpu_input_consumption` returns that source, native poll generation and
  mapped camera update count only while the producer stamp is still unchanged.
  Multiple consumed generations, disabled consumers, quiesce/restart, and an
  intervening producer all refuse. The helper never locks native code or
  suppresses any callback, and it never publishes a native pointer.

New guards (first 19 bytes, same exact executable): PlayerControls `0x7ad900`
`48 8b c4 56 57 41 54 41 56 41 57 48 81 ec 80 00 00 00 48`;
camera update `0x8fd590`
`40 57 48 83 ec 70 48 c7 44 24 20 fe ff ff ff 48 89 9c 24`.
The existing byte preflight and hook lifecycle apply to both. No SKSE is used.

`tests/skyrim_input.cpp` now covers empty/in-flight/completed generations,
intervening writes, overlap in both completion orders, unframed invalidation,
and session invalidation through the same atomic helper used by the hooks.
These cases are added but not executed, per the coordinator's no-build rule.
The renderer contract is deliberately observational: a successful query proves
one mapped camera update used an unchanged completed control-state generation,
not a pre-input pacing boundary, full simulation ownership, the final camera's
view identity, or FG eligibility. NO_FG is retained. First-person/free-camera
coverage and cross-thread camera source identity remain refused. Build/test
commands above still belong to the coordinator after integration.

## Native screen classification and rendered-source admission

The subsequent task separates rendered-source classification from input timing.
Static analysis used the same exact database, readopted as IDA session
`a3b93525`. CommonLib MIT checkout revision
`add17c090cfa312d370bfcdd55d2eb989d086fec` supplied layout leads only; native
instructions and constructor references below establish the used offsets.
No reference code was copied.

Native facts and their use:

- Main tick reads UI singleton `0x219e5c0`+0x160 as a DWORD pause count at
  `0x658940`, mirrors nonzero into `0x32303b3`, and permits normal simulation
  only when that count and Main+0x16 are zero (`0x65895f`, `0x658aa7`).
  These establish paused and freeze-time facts; freeze alone does not prove
  which menu or loading state is active.
- Player singleton is `0x3230778`; native RenderEntry explicitly requires
  player+0x60 nonnull before its menu background World render at `0x65681a`.
  The matched parentCell layout is used only as a nonnull active-cell guard,
  with an actual primary World call required separately for gameplay.
- InterfaceStrings constructor `0x1177a90` stores singleton `0x21a05c8` at
  `0x1177aa6`; it interns Main Menu at +0xa8 (`0x1177c1e`) and Loading Menu
  at +0xe8 (`0x1177cbe`). This independently confirms exact-build menu names
  and offsets. No string matching or guessed menu ordinal is used at runtime.
- Native `0x1169530(UI*, const BSFixedString*)` looks up a retained menu,
  tests its flags+0x1c bit0x40 (on stack), then releases its temporary
  reference. Native `0x1169590(UI*)` holds the native stack lock and tests
  flags+0x1c bit0x20 (freeze-frame background). RenderEntry uses the latter
  at `0x6567c7` to choose cached menu background versus fresh World rendering.
  The adapter calls these native queries on the existing render thread rather
  than walking the mutable menu map/stack without their synchronization.
- Manager singleton `0x3220a90` requested+0x68 and running+0x6c must both be
  at most1 for gameplay. A mode-2 shutdown with requested0/running2 still
  refuses gameplay until the native join finishes. Mode2 alone does not
  imply Loading Menu, so it produces UNKNOWN unless a menu is established.

Both query entrypoints are preflighted with 19 expected bytes before installing
CPU hooks, in addition to the existing exact-build hook guards:
`0x1169530`: `40 53 48 83 ec 40 48 c7 44 24 20 fe ff ff ff 33 c0 48 89`;
`0x1169590`: `48 89 5c 24 10 48 89 6c 24 18 48 89 74 24 20 57 41 56 41`.
The queries borrow initialized native fixed strings. A before/after scalar
snapshot checks singleton identities, fixed-string identities, cell, pause,
freeze and both manager modes; unreadable or changed snapshots remain UNKNOWN.
This detects observed transitions, not an atomic transaction across all engine
state. All native pointers remain local and are never exported.

Classification priority is UNKNOWN for unowned/unreadable facts, then LOADING
for the actual Loading Menu, MENU for Main Menu, pause or native frozen menu
background, then GAMEPLAY only for a verified primary World with a nonnull
parent cell, no freeze and neither manager mode2. Freeze without menu evidence,
no active cell, and pre-World frames remain UNKNOWN. Normal HUD/dialogue that
neither pauses nor freezes the background does not itself relabel gameplay.

Primary World still requires the exact return address `0x656e32`, output0,
the owned CPU/render source and actual camera/swapchain identity. Its entry
establishes screen classification before native World rendering and therefore
before SR identity is captured. `cpu_render_identity` carries that screen to
SUBMISSION, SR and renderer handoff; FRAME begin is conservatively UNKNOWN
until World observation, and FRAME end carries the established classification.
Multiple primary World entries in one source degrade it to UNKNOWN.

The coordinator subsequently authorized conditional NO_FG removal for the
generic rendered-source contract, superseding the earlier preserve-only scope.
`cpu_rendered_source_eligible()` requires an active owned render scope, exactly
one primary World entry and GAMEPLAY. `cpu_render_identity` sets NO_FG by
default and clears it only for that predicate; CPU FRAME/SUBMISSION callbacks
recompute flags at completion so they agree with later source facts. The
renderer worker consumes the same predicate for FG_INPUTS/FINAL_SCENE/WINDOW.
The generic host declares FG_INPUTS|RENDERED_SOURCE, without INPUT_TIMELINE.
Runtime limits this contract to rendered-source FSR, pacing None and latency
Off, and refuses DLSS/XeSS. No provider decision is implemented in this adapter.
This permits a bounded FSR experiment; it is not evidence of working FG,
pre-input pacing, low latency, moving-image quality or broad acceptance.

Focused classification cases were added to `tests/skyrim_input.cpp`: active
World, pre-World refusal, unreadable/foreign state, pause/loading priority,
freeze-only refusal, Main Menu over an existing cell, frozen menu background,
missing cell, both mode2 transition states and duplicate-source refusal.
Only scoped `git diff --check` ran for this change. The coordinator's earlier
Release build/contracts apply to the preceding input/camera work, not these
new classification/query changes. No builds, games, GPU fixtures, deployments
or commits ran in this task. Coordinator validation remains Release targets
`rsf_skyrim_input`/`rsf_game_skyrim_se`, `skyrim_input_contract`, renderer
contracts and the native verification gate after integration.

## Prospective source at native poll: dependency graph and design

The next question was whether a source allocated before the real native poll
could later bind to its consuming update, preserving native timing and mode2,
and whether waiting for the preceding rendered source would deadlock.
The answer differs for actual Present and CPU frame completion. This section
is a static design with explicit admission conditions, not an installed input
timeline. Existing runtime capabilities and CPU markers remain unchanged.
Retained disassembly/decompilation, callback arrays and names are in ignored
`.local/skyrim-research/native-input-task-graph-20261115.json` (same executable
SHA256 above, IDA session `a3b93525`). No process was launched or attached.

### The late poll belongs to a future consuming update

Constructor `0x60d740` builds seven ordered groups using `0x60ef30`, appending
each to manager array `0x3220a78`/count `0x3220a88`:

| Group | Native callback table | Completion callback |
| --- | --- | --- |
| Main update cells | 0x20b4d00 | 0x6e4fa0: signal1, wait bank1/event1 |
| Main update player | 0x20b4d60 | 0x6e50e0: wait bank1/event3 |
| Main prepare render | 0x20b4d90 | 0x6e54b0: wait bank1/event4 |
| Main render scene | 0x20b4ed0 | 0x6e5980: wait bank1/event5 or6 |
| Main render 1st person | 0x20b5000 | 0x6e61e0: wait bank1/event7 |
| Main render end | 0x20b50e0 | 0x6e6200: wait bank1/event8 then9 |
| Main post render | 0x20b5150 | no JobListEnd entry |

The manager at `0x60f2db` submits a group, waits its completion event at
`0x60f2ec`, and only then calls the table's JobListEnd callback at `0x60f2fd`.
It cannot advance to the next group while a callback of the current group
remains in flight. Its final manager-complete event is signalled at `0x60f377`.

Within a group, table order is NOT completion order. `0x60ef30` converts
sentinels `0x60d700`/710/720 into signal/sync list entries via `0xebb120`:
700 signals a batch,710 synchronizes,720 does both. Worker `0xebc170`
claims a callback by incrementing the shared cursor at `0xebc235`, invokes
it at `0xebc245`, and decrements its batch count only AFTER it returns at
`0xebc27a`. Another worker can claim and run a later callback concurrently.
Sync entries wait for the preceding batch count to reach zero at `0xebc308`.

Main render end's exact entries are:

1. `0x6e5db0` Non render-safe AI, then a signal/sync sentinel at0x20b50f0.
2. `0x6e6310` Animated cell object update, then sentinel at0x20b5110.
3. `0x6e6050` Post process and `0x6e6030` Poll controls, with NO sentinel
   between them (table0x20b5120/130).
4. On group completion, `0x6e6200` waits main events8 and9.

Thus Post process does not complete before Poll controls just because it is
listed first. Post process waits bank0/event20 at `0x6e606b`, performs
`0x65a3c0`, updates/joins a native list, then signals22 at `0x6e61cb`.
Non render-safe AI signals20 at `0x6e5f97`; the preceding barrier already
separates it from the final batch. The existing wait22 wrapper `0x6e6230`
has no analyzed incoming reference and is absent from every constructed main
group. Do not describe its wait as an active render dependency. The name
Post process here denotes native CPU work and is distinct from image-space
postprocessing `0x153ac00`/SR.

Signal helper `0xec1d30(index)` uses TLS+0x880 to select its bank;
wait helper `0xec1dd0(bank,index)` selects an explicit bank and waits infinitely.
Constructor sets the main thread bank to1 through `0xec1e10(1)` at0x60d79f.
Bank0 is the worker side expected by the native waits; a future bounded probe
must record the actual worker TLS bank rather than assume it for every thread.

### Exact Present versus CPU completion

Main World signals event6 at0x657724 and event7 at0x65711a/0x657849/0x65789b
depending on the first-person path. Group4 completion waits event7 before
Main render end, and hence before the named worker poll, is submitted.
The World tail still performs image-space postprocessing at0x6578d1 after its
late event7 signal. The worker poll can therefore overlap SR/postprocessing,
UI and Present of that already prepared native scene. Main RenderEntry waits
bank0/event2 at0x6568f3; its producer is UI callback0x6e4e80 in the earlier
Main render scene group, not Poll controls.

Main signals event8 at0x656b94 before RendererEnd. Actual swap-chain Present
is the vfunc+0x40 call at0x100a012 inside0x1009f70, NOT RendererEnd entry:
that routine may first execute its own Sleep(0) limiter. The presentation
counter increments at0x100a02b only after the call. Main then signals event9
at0x656bf0, returns RenderEntry, updates timing at0x65932a, and joins the
whole manager at0x65932f through0x60eb80. Tick FRAME_END is still later.

Consequences for the core scheduler:

- A prospective poll P must not be attached to the concurrently rendered
  source S solely because S owns the manager submission. S's movement/camera
  preparation precedes this late poll. P is a candidate for a later tick.
- Waiting at P for S's CPU FRAME_END or manager completion is a definite cycle:
  P -> S FRAME_END -> manager join -> P's group completion -> P.
- Waiting for S's actual application Present completion has no such edge in
  the traced native group graph: event9 follows Present and the manager waits
  for9 only after the Poll callback completes. Main's pre-UI wait2 is produced
  by an earlier, already-completed manager group. Present completion must be
  signalled directly from the actual Present callback, independently of CPU
  frame closure, delayed job binding and pending input-source resolution.
- This is a graph result, not a measured guarantee for vendor waits, injected
  hooks, lock ownership or a saturated worker pool. A core callback must hold
  no runtime/session/Present lock while it may wait. No polling of FRAME_END,
  final classification, or a future P render is a valid replacement for S's
  actual Present completion. Skipped/native error paths need explicit aborts
  and bounded cancellation, never an infinite wait for an absent Present.

### A safe contract and the remaining proof

The concrete design retains native execution order and separates provisional
sampling from rendered-source binding:

1. At exact mode1 submission0x60eaf0, before its SetEvent at0x60eb60, capture
   an immutable manager-generation ticket tied to the current TLS rendered
   source S. Publish it BEFORE wakeup. Verify idle pending mode0 and the exact
   singleton; never read the latest tick atomic in a worker. Retire this ticket
   only after0x60eb80's actual wait returns, preserving its borrowed lifetime.
2. Hook named callback0x6e6030 to establish TLS provenance through its tail
   call0x659df0 to PollDevices. At CF95F0 entry only a verified mode1 ticket
   may allocate provisional CPU source P. Report an append-only generic
   source-reserved/pre-input fact, including the explicit predecessor S and
   native overlap fact. Let the orchestrator decide if/when to pace. The
   adapter neither sleeps nor chooses a vendor or marker semantics.
3. P's real input boundary is immediately before the unchanged original
   PollDevices(manager,float timestep). Preserve XMM1 and all native queue,
   second-notification and return behavior. Dispatch, PlayerControls completion
   and the completed poll epoch belong to P, even before any rendering exists.
   Reserve alone must NOT be reported as a simulation or Present marker.
4. At the next successful main-update branch, before first PlayerCharacter
   call743090 (return6588e3), bind P only if its singleton/control epoch remains
   unchanged, the owning manager generation has joined, and no paused/mode2 or
   intervening poll invalidated it. Then trace that epoch across movement and
   camera consumers and their native group completions up to view preparation.
   A failed branch/transition closes P as partial/aborted; it is never rebound
   opportunistically to another view. Mode2/paused polls remain unretimed and
   either reserve separately labelled partial sources or produce no admission.
5. The public C ABI needs source reservation, delayed binding, abort and explicit
   predecessor-Present dependency semantics. Implement these append-only in the
   shared interface, not as Skyrim-only callbacks. Core must accept facts before
   binding and close abandoned reservations on quiesce. Existing ABI events
   cannot express these lifetimes honestly by reusing FRAME_BEGIN/INPUT_SAMPLE
   with guessed source IDs. INPUT_TIMELINE remains absent until this contract
   and actual consumption coverage are validated.

Movement coverage is a real additional requirement. Native0x776290 reads
PlayerControls+0x24/+0x28 and+0x2c/+0x30, scales movement with actor functions
at0x7763d4 and look at0x7765c3, and writes derived control state+0x44 at0x776617.
Its caller0x746c30 at0x746c72 has a floating-point second argument in XMM1,
despite the decompiler's incomplete integer prototype. Player update0x745200
also reads movement+0x24/+0x28 around0x745844-865 through its native vtable.
The current camera-only stamp does not cover these paths, their virtual
callers, or all writers of derived control state. An unchanged camera stamp
must not be promoted into complete simulation/input ownership.

Exact future hook contracts:0x6e6030 accepts the scheduler's callback-data RCX
(unused by its body), returns pointer-width RAX and tail-forwards Main;
0x659df0 loads native float XMM1 at0x659e00; CF95F0 takes RCX manager/XMM1
timestep.0x60eaf0 takes no used native arguments and returns low32 native
result;0x60eb80 takes no used arguments and returns DWORD. Their candidate
19-byte guards are respectively:

- 6e6030: `48 8b 0d f9 a4 b4 02 e9 b4 3d f7 ff cc cc cc cc cc cc cc`
- 60eaf0: `48 83 ec 28 48 83 3d 94 1f c1 02 00 74 69 48 89 5c 24 30`
- 60eb80: `40 53 48 83 ec 20 48 8b 1d 03 1f c1 02 48 85 db 74 26 83`

These are candidate hooks, not installed changes. No new production hook or
CPU event was added in this continuation because the unresolved movement and
ABI lifetime requirements would otherwise turn a useful prospective-source
design into a false input timeline. The delivered screen/ownership code is
preserved. Validation here is static disassembly, cross-reference and native
control-flow review plus scoped documentation whitespace checking only; no
build, test execution, game, GPU fixture, deployment or commit was performed.

## Mode1 generation ticket and mapped movement observations, 2026-11-15

This continuation asks whether the adapter can identify the native job owning
an actual poll without assigning that poll to a guessed consuming frame. The
matching IDA database was reopened as session `e50577c3`; the executable SHA256
remains `846efccf0c1374d71f892907f46549560f2fcb0a75cb87a3eed438baa0f1402f`.
Evidence is static disassembly, decompilation and callback-table references,
retained locally in `.local/skyrim-research/native-job-ticket-movement-20261115.json`.
No target execution occurred. The preceding paragraph describing hooks as
candidates records the earlier stopping point; the verified subset below is
now implemented, without the proposed public reservation interface.

### Implemented native lifetime

`NativeJobOwnership` publishes immutable scalar manager address, session,
submitting source and generation before calling original submit `0x60eaf0`.
Only the call returning to `0x658b14`, inside the observed Main tick, with
matching Main singleton and manager requested/running modes both zero can
publish. Original submit still drains its thirteen lists, writes manager
`+0x68/+0x6c = 1`, and performs SetEvent on `+0x58` at `0x60eb60`.
The ticket therefore exists before a newly awakened worker can observe it.
A failed native wake or unexpected post-call mode/pointer poisons provenance.

The named `0x6e6030` callback accepts the scheduler data in RCX and tail-forwards
Main to `0x659df0`; the hook preserves its pointer-width return. Only the actual
scheduler indirect call at `0xebc245`, return address `0xebc248`, establishes
callback TLS. Preparation verifies the native callback pointer at `0x20b5130`
is exactly `0x6e6030`. The scheduler claims the list index at `0xebc226` before
calling; table adjacency is not used as a join. TLS contains only a copied
ticket, never a borrowed Frame or native event pointer.

`cpu_poll_provenance` is an internal, callback-lifetime query. It returns
session, job generation, submitting source and current poll generation only
inside that named callback's exclusive PollDevices scope, with matching
manager and actual input-manager singleton. Native running mode must be 1;
requested mode may be 0 or 1 because Main clears requested mode before waiting
for the still-running callback. Mode2 and direct paused polls get no such
provenance. Nested or overlapping polls, intervening invalidation, conflicting
submissions and stale tickets refuse it. The submitting source is the native
render that overlaps this late poll, not the source consuming the controls.
No reservation, delayed source binding or CPU event is emitted by this query.

Retirement is after original `0x60eb80` returns to `0x659334`, with the same
manager/Main, an observed pre-call pending mode1, native result WAIT_OBJECT_0,
and post-call requested/running modes both zero. Original clears requested
mode at `0x60eba4`, waits event `+0x60` at `0x60ebab`, then clears pending mode
at `0x60ebb1`. The immutable ticket remains available while Main is waiting.
An unexpected join poisons it rather than recycling it. Reset is restricted
to a quiescent adapter start; generations remain monotonic across resets.

The three entry guards are the 19-byte sequences above. Added movement entry
guards are:

- `0x776290`: `48 8b c4 41 56 48 81 ec e0 00 00 00 48 c7 44 24 30 fe ff`
- `0x745200`: `48 8b c4 55 56 57 41 54 41 55 41 56 41 57 48 8d a8 98 fe`

All hooks forward the original arguments and result. Submit/join use native
low32 results; no native arguments are consumed by these two bodies. Movement
`0x776290` is void `(actor RCX, float XMM1, argument R8, float* R9)`.
Its only direct caller `0x746c30` calls at `0x746c72`, then calls base
`0x6b1580`; the wrapper occurs in native vtable `0x1929f90`.
Player update `0x745200` takes `(actor RCX, float XMM1)` and preserves native
pointer-width RAX, with a virtual slot at `0x1929c28`. Prologues save XMM1 at
`0x7762eb` and `0x745252` respectively. Neither hook replaces the timestep.
Both observe an unchanged completed poll stamp around the call only for the
verified PlayerCharacter singleton and existing current-thread tick scope.
Counts distinguish these call envelopes from the existing camera observation;
zero or an early-return call is not evidence of complete movement execution.

### Native producers and unresolved coverage

PlayerControls event sink `0x7ad900` resets raw movement `+0x24/+0x28` and look
`+0x2c/+0x30`, dispatches through `0x7b0130`, filters through `0x7aeba0` at
`0x7add44`, applies constraints and copies sixteen bytes into previous state
`+0x34` at `0x7ade56`. Handler dispatch supplies data = PlayerControls+0x24.
Constructor `0x7b0920` installs MovementHandler `0x19351c8` at owner+0x170,
LookHandler `0x1935150` at +0x178, and AutoMoveHandler `0x19353d8` at +0x190.
Their native dispatch slots +0x20/+0x28/+0x30 are thumbstick/mouse/button:

- Movement thumb `0x7b38c0` writes data+0/+4; button `0x7b37c0` writes axes
  and clears auto-move data+0x24, which is controls+0x48.
- Look thumb `0x7b37b0` and mouse `0x7b3270` write data+8/+0xc. Mouse converts
  integer deltas and negates Y. Auto-move button `0x7b3130` toggles data+0x24.
- Look motion `0x7b32a0` also invokes actor rotation through `0x693050` and
  `0x674210`, retains handler+0xa8/+0xac and counter+0xb4, and sets data+0x26.
  Dispatch therefore has synchronous actor side effects, not just vector writes.
- Filter `0x7aeba0` retains controls+0x1dc timing state using native timestep
  `0x327560c`. Movement `0x776290` writes derived controls+0x44 at `0x776617`;
  camera helper `0x8fb2e0`, called by `0x8fd590`, also writes +0x44.

The sink's existing write envelope covers its synchronous handlers for raw
poll observation. It does not prove every derived-state writer across native
virtual updates or all actor consumers. Movement/player-update observations
are restricted to the current main-thread tick; a worker invocation gets no
invented source association. Free camera and remaining virtual caller/group
coverage are unresolved. Neither counts nor camera stamps are used to enable
INPUT_TIMELINE, remove NO_FG, bind a prospective source, or assert simulation
coverage. Existing conditional rendered-source admission is unchanged.

### Remaining generic contract and validation

Core still needs append-only reservation/bind/abort and explicit predecessor
actual-Present dependency semantics before the callback query can become a
pre-input producer. A reservation would occur before unchanged native polling;
its binding must wait for the owning manager join and prove no intervening
poll/control mutation across all relevant simulation and camera consumers.
It must abort on mode transitions, skipped updates, quiesce and ambiguity.
Waiting for the submitting frame's FRAME_END still creates the documented
join cycle; callback lifetime provenance does not make that wait safe.

Focused helper cases now cover immutable ticket identity, forged/stale joins,
monotonic reuse, overlap poisoning, quiescent session restart, invalidation and
exclusive poll provenance. They have not been executed in this task. Scoped
whitespace checks passed; source review and static native evidence are the
only validation here. No build, game, GPU fixture, deployment or commit ran.
Coordinator checks after integration: build Release targets `rsf_game_skyrim_se`
and `rsf_skyrim_input`, run `ctest -C Release -R skyrim_input_contract` in the
configured native build tree, then the normal Release verification gate.
Native runtime acceptance remains outstanding, including hook installation and
observing matched submit/callback/join generations through mode transitions.

The generic reservation/bind/abort contract and predecessor Present dependency are
now implemented; see [prospective source](skyrim-prospective-source-20261115.md).
