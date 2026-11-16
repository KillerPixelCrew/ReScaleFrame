# Fallout 4 native CPU ownership, 16 November 2026

This investigation asks where the installed engine polls input, consumes it,
updates simulation and renders a source that reaches actual Present. Native
instructions were checked against the source leads in
[the render integration investigation](fallout4-render-integration-20261116.md)
because the reference's latency markers alone cannot establish these facts.
This is static shipped-binary analysis and source review, not a runtime test.

## Identity and method

The installed `Fallout4.exe` is 1.11.240.0, SHA256
`fdcef37ac1230af6d0b0050eb2142b139ef3a867b37b9211fb6edfcc646072f8`.
All addresses below are RVAs against this image, whose preferred base is
`0x140000000`. ASLR requires adding the actual module base at runtime.
Source revisions are Community Shaders
`e0e711594291c1a8e447ac758099492514810fbb`, CommonLibF4
`7f8deca16dc7dd245c9c52151ec9ef27edd9ad60` and commonlib-shared
`5408d22adf937cc00e0ccd4bdeef66021172884d`.

Capstone 5.0.7 and pefile inspected the installed PE directly. The old
1.11.221 address database supplied search neighborhoods only. No old RVA
was used as an activation address. The `.pdata` entries split some functions
into fragments, so a fragment entry is not automatically a function entry.
Shared Ghidra/REA/IDA sessions were not touched. No game, build, test,
deployment or commit was performed by this worker.

Local receipts under `.local/fallout4-research/` are `cpu-c314f0.txt`,
`cpu-c336b0-full.txt`, `cpu-c339d0-full.txt`, `cpu-c33000.txt`,
`cpu-1672540-full.txt`, `cpu-166e110-full.txt`, `cpu-10372a0-full.txt`,
`cpu-input-xrefs.txt` and the corresponding `cpu-*.py` scripts. These are
untracked analysis receipts, not runtime dependencies. The supplied F4SE
0.7.9 source archive's `GameInput.cpp/h` gives an independent singleton/layout
lead; its archive fingerprint is recorded in the linked investigation.

## Native loop and render ownership

| Boundary | Installed instructions | Interpretation |
| --- | --- | --- |
| Main::Run | entry `c314f0`; `c3151e: e8 8d 21 00 00` calls `c336b0` | Matches AE source Run+2e -> WindowsMessageLoop |
| Conditional idle | `c315bb: e8 10 24 00 00` calls `c339d0` | OnIdle is conditional, not every message-loop iteration |
| Main::Swap | `c346ac: e8 9f eb ff ff` calls `c33250`, return `c346b1` | Matches OnIdle+cdc; synchronous render source scope |
| World/UI path | `c33434: e8 07 4e 00 00` calls `c38240` after `c3342a` calls `c38e30` | Swap contains world/UI work, so its entry is not render-submit after simulation |
| Render_UI | `c383c2`, `c39d6a`, `c39e0e` call `21fa200` | Normal path plus both DrawGamePause paths, as independently established by coordinator |
| Late Swap work | `c335db` calls `17eeed0` | Callee ownership and actual Present are not yet proved by this worker |

Main::Run reads the native main singleton at `32dd130`. Its native HWND is
read from Main+30 before foreground-window comparison. The loop can take a
background branch with an existing native sleep and skip OnIdle. It checks
quit/reset flags after that branch and loops back to `c31510`. These sleeps
are existing game behavior, not an RSF pacing implementation.

OnIdle writes Main+1d0 from a native menu-related count at entry and tests
Main+2a repeatedly. This supports the CommonLib `inMenuMode` and `freezeTime`
layout leads but does not identify all UI/menu states or prove that every
simulation subsystem is frozen. OnIdle continues work after Swap returns,
through its return at `c3494b`. Therefore a Swap source is a rendered source,
not the whole OnIdle update. Do not label that scope simulation begin/end.

## Windows messages are distinct from polling

The `c336b0` routine calls imported `PeekMessageA` at `c338e4` and `c33918`,
with PM_REMOVE=1 and null HWND/filter arguments. The loop calls
`TranslateMessage` at `c338f4` and `DispatchMessageA` at `c338fe` before the
next PeekMessage. This proves native dequeuing/dispatch ordering. It does not
prove that a dispatched WM message caused a keyboard, mouse or controller
sample for the next world update. A second small pump at `c33950` likewise
does not establish physical sampling.

The reference TemporalPipeline emits simulation/input markers after its
WindowsMessageLoop wrapper. That remains reference marker policy. RSF must
not emit INPUT_SAMPLE or SIMULATION_BEGIN there. A future native message
reporter can report the actual dequeued ID, without key/button contents,
through INPUT_EVENT/WINDOW_MESSAGE after proving the MSG lifetime and hook
ABI. No such hook is installed here.

## Actual native device polling

OnIdle loads the float delta in XMM1 at `c342a6`, loads the input manager
singleton from `3e75cb8` at `c342ae` (`48 8b 0d 03 1a 24 03`) and calls
`1672540` at `c342b5` (`e8 86 e2 a3 00`), returning to `c342ba`.
The dispatcher ABI is `void(manager*, float)`, x64 RCX/XMM1.

Its instructions establish the dispatcher independently of the source marker:

| RVA | Bytes | Native fact |
| --- | --- | --- |
| `1672540` | `48 83 ec 38` | Stack entry |
| `1672544` | `80 79 35 00` | Tests manager pollingEnabled+35 |
| `1672550` | `74 40` | Disabled branch skips every device poll |
| `1672557` | `48 8d 59 08` | First device pointer at manager+8 |
| `1672560` | `bf 05 00 00 00` | Five entries |
| `1672565..167256b` | `48 8b 0b 48 85 c9 74 09` | Null devices skipped |
| `167256d..1672573` | `48 8b 01 0f 28 ce ff 50 08` | Device vtable slot1 receives float delta in XMM1 |
| `1672576..167257e` | `48 83 c3 08 48 83 ef 01 75 e5` | Advances one pointer until all five visited |
| `1672583` | `e8 88 bb ff ff` | Calls `166e110` after device poll loop |
| `167259b` | `c3` | Dispatcher returns |

Pinned CommonLib `BSInputDeviceManager.h` names the five entries keyboard,
mouse, gamepad, debug gamepad and virtual keyboard; `BSInputDevice.h` names
slot1 `Poll(float)`. The native +8 array and +35 gate agree exactly.
This is proof of the dispatcher calling device poll methods. It is not yet
proof of which physical API each implementation calls, which devices deliver
events on this invocation, or where the resulting queue/control state is
consumed by gameplay and menus.

The trailing `166e110` is a locked float/time-list processor, including an
existing contention Sleep. Its proximity does not establish an input event
consumer. Following OnIdle call `10372a0` is a short tail wrapper to `1673ae0`;
its arguments and consumer behavior remain unresolved. These are retained
leads, not a claimed source binding or completed simulation boundary.

## Initial rendered boundary contract

`games/fallout4/src/cpu_events.cpp/h` supplies one guarded rendered-source
scope. It detours native Swap `c33250` with `void(Main*)`, admitting only the
verified OnIdle return address `c346b1`. Activation checks the call bytes
above and entry bytes
`48 89 5c 24 08 57 48 81 ec 40 01 00 00`.
The caller must first verify the exact executable fingerprint, initialize
MinHook, supply size-checked host services and preserve module/callback
lifetime. A byte mismatch refuses activation; the lifecycle owner must log
the refusal. Source code is provided, not built or activated in this task.

At synchronous admitted entry the reporter creates a monotonic source ID and
reports FRAME_BEGIN; after unchanged Swap returns it reports FRAME_END.
QPC timestamps are measured at those callbacks with their frequency, never
backdated to a presumed input or simulation time. The current identity is
thread-local and only usable during that call. A verified primary renderer
calls `begin_cpu_render()` once and copies `current_cpu_identity()` into its
leased resources. The render association ID is not an ExecuteCommandLists
event. FRAME_END reports one primary renderer only after that producer has
registered, otherwise RENDER_SKIPPED. The adapter must not use host.submit
to describe a DX11 world draw or Swap entry.

The runtime owns missing-stage derivation and rendered provenance admission.
At this initial stage no INPUT_KEYED, INPUT_SAMPLE, prospective reservation,
simulation stage, vendor marker, pacing sleep or input capability was emitted.
The later prospective implementation below supersedes the reservation absence.
Root owns wiring
the functions into lifecycle/render hooks and validating that actual outer
Present and the WINDOW lease carry the same source. The scope does not
authorize caching textures, camera state or immediate-context work after its
lease ends. Pause paths outside the selected caller have no source until
their separate ownership is proved; never borrow the last world source.

## Remaining proof and activation limits

For full latency, trace concrete keyboard/mouse/controller Poll implementations
to the physical APIs and their event/state producers, then the queue drain
and gameplay/menu consumers. Prove whether an intervening poll or worker can
replace sampled state, including loading, VATS, inactive, menu and pause paths.
The later implementation selects RESERVE/SAMPLED/BIND/ABORT from the existing
versioned event contract, without claiming complete simulation coverage.
RESERVE runs before the native poll, and SAMPLED
must carry QPC taken after RESERVE returns immediately before the unchanged
poll. A skipped pollingEnabled branch cannot claim produced input.

Simulation begin/end and actual render submission remain unresolved. Trace
Swap's late callee `17eeed0` and the actual outer chain Present separately;
FRAME_END is CPU scope closure, not GPU completion or successful Present.
F4SE callback/trampoline and ENB pre/post-Present work must remain in native
order. Public capabilities and readiness remain refused until producer
coverage and the coordinator's required checks exist. Source review does not
establish measured latency, SR/FG quality or F4SE/ENB runtime acceptance.

## Follow-up: acquisition, queue consumption and redraw ordering

The follow-up dispatch continued static analysis after the initial source
freeze. The earlier statement that the queue consumer was unresolved is
superseded for the dispatcher and the concrete PlayerControls receiver below.
It remains unresolved for the simulation source that owns the effects of
those controls. Additional local receipts are `cpu2-*.txt`,
`cpu2-physical-api-xrefs.json`, `cpu2-vtables.json` and `cpu2-xrefs.json`.

### Keyboard and mouse acquisition precedes buffered Poll

Native window procedure `c3a5f0` has the x64 WNDPROC ABI
`LRESULT(HWND,UINT,WPARAM,LPARAM)`, keeping the message in ESI and LPARAM in
RDI. Its `WM_INPUT` (0xff) branch leads to `c3a8ba`. It queries
`GetRawInputData` at `c3a8fe`, then obtains RAWINPUT at `c3a945`, both with
RID_INPUT=0x10000003 and header size0x18. For dwType0 it calls mouse handler
`16754d0` at `c3a96b`, bytes `e8 60 ab a3 00`, return `c3a970`; for dwType1
it calls keyboard handler `1675140` at `c3a98d`, bytes `e8 ae a7 a3 00`,
return `c3a992`. Both handlers use `void(device*,const RAWINPUT*)` in
RCX/RDX and return without a value consumed by the caller.

Keyboard handler `1675140` starts
`48 89 5c 24 10 48 89 6c 24 18`. After raw-key mapping it writes device+170
at `16751f1`. Mouse handler `16754d0` starts
`0f b7 42 1c a8 01 74 08`; it updates button bits+71 and wheel+72, then
accumulates relative mouse motion into+74/+78 at `167557a`/`1675580`.
These are actual native receipt/buffer-write boundaries, not hypothetical
hardware timestamps. The previously identified five-device Poll dispatcher
later consumes those buffers. In particular, keyboard Poll compares+70/+170,
while mouse Poll emits movement from+74/+78 and clears the accumulated deltas.

The distinction matters: a marker before buffered Poll measures the engine's
poll boundary, not the time the physical keyboard or mouse was sampled by the
OS. Do not describe the former as a direct hardware API sample. Events can
also arrive through other native paths, which are not yet all covered.

The CPU reporter now additionally detours these two RAWINPUT handlers, checks
their exact entry/call bytes and selects only the native return addresses
above. It reports INPUT_EVENT with source0, INPUT_UNFRAMED and the received
keyboard/mouse kind, without retaining keys, buttons or payloads. This does
not claim which engine source consumes the event. The unchanged native
handler still runs with original arguments and LastError. Prepare/start/stop
manage these hooks with the existing Swap hook, including partial setup
cleanup. No runtime execution is claimed for this addition.

### Controller samples inside its native Poll

PE RTTI complete-object-locator and vtable inspection establishes these
offset0 vtables and entries, rather than assuming source IDs resolve:

| Native class | Vtable RVA | Poll/receiver entry |
| --- | --- | --- |
| BSPCKeyboardDevice | `267e800` | slot1 `1674fe0` |
| BSPCMouseDevice | `267e838` | slot1 `1675350` |
| BSPCGamepadDeviceHandler | `267efc0` | slot1 `16771b0` |
| BSPCGamepadDevice | `267f0b8` | slot1 `16777f0` |
| BSPCGamepadDeviceDelegate | `267f040` | slot1 `1677620`, a no-op return |
| PlayerControls | `2571a30` | slot0 `ddbe50` |
| MenuControls | `25b1ca8` | slot0 `1074af0` |

Handler `16771b0` selects/delegates through device+b0, checks connection and
calls the selected device's slot1. Concrete gamepad Poll `16777f0` saves its
previous state, passes user index ECX and state destination device+e0 in RDX
to `1677bb7` at `1677847`, and tests EAX at `167784c`. A nonzero result
skips event generation. Thunk `1677bb7`, bytes `ff 25 53 93 dc 00`, jumps
through IAT `2440f10`, imported from `XINPUT1_3.dll` ordinal2. The installed
System32 DLL's export table names ordinal2 `XInputGetState`, establishing the
physical API locally. This poll differs from keyboard/mouse buffered Poll.
No controller hook or controller-reception counter is implemented here:
successful state retrieval alone does not prove a new delivered user event.

### Queue-to-player consumption is synchronous

OnIdle after Poll calls `10372a0`, a short tail wrapper to `1673ae0` for
another receiver. More directly, at `c342cb` it loads PlayerControls from
`31f0b80` into RDX, at `c342d2` loads input queue `3273ae0` into RCX, then
at `c342d9` calls `1673ae0`, bytes `e8 02 f8 a3 00`.

Dispatcher `1673ae0` uses queue lock+1478, queued head+1480 and generation
+1490. It locates the first node newer than receiver+8, copies the current
generation to receiver+8, then at `1673b72` (`ff 10`) synchronously calls
receiver vtable slot0 with the selected event head in RDX. The receiver ABI
is `void(receiver*,const InputEvent*)`, matching pinned
`BSInputEventReceiver::PerformInputProcessing`. A receiver generation of -1
instead adopts current generation without delivering the queue.

Concrete PlayerControls `ddbe50` resets control floats+44/+48/+4c/+50, checks
menu/player eligibility, traverses event nodes through+18 and calls `ddc350`
at `ddbff6`, followed by its registered input handlers. Concrete MenuControls
`1074af0` traverses the same typed event structure and dispatches handlers
by the event type. These functions can intentionally refuse or skip input.
The dispatch call is proof of consumption entry, not proof that every queued
input changed the world or that every consumer has completed.

Additional receiver dispatches occur at `c346fb..c34728`, after native Swap.
Therefore the first PlayerControls dispatch is not a blanket end of all
input handling. Root must preserve unknown source ownership on these paths.

### Simulation boundary limitation and the narrower redraw flag

Source-named OnIdle_UpdatePlayer lead resolves near current `c34fa0`; the
native `c33d3d` call is conditional on menu/freeze state and precedes Poll.
It must not be used alone as an unconditional current-input simulation
boundary. `c3b810`, called before Swap, is actor-distance/task-list
collection, not established as the entire simulation update. Likewise late
Swap callee `17eeed0` performs world camera/object work, including vtable+1d0
and `16c8570`; it is not direct DXGI Present. Those hypotheses were rejected
by instruction review rather than promoted to markers.

The actual proved ordering is PlayerControls queue dispatch `c342d9`, later
scene preparation calls `c345b6`/`c345f7`, collection `c34683` and list/join
work `c346a0`, Swap `c346ac`, world/UI `c33434`, DrawGamePause check `c3828e`,
then WorldBegin `c382a0`. DrawGamePause may bypass the world path. No complete
loading/menu/gameplay classifier follows merely from these branches.

`RSF_GAME_RENDER_AFTER_SIMULATION` has a narrower documented meaning:
the constructor followed the engine update-to-redraw boundary, without
promising vendor markers or final Present ownership. Root can review the
sequence above against that exact contract independently of actual CPU
SIMULATION_BEGIN/END marker coverage. This worker does not emit those stages
or set the flag merely because Swap ran. A prospective poll source must not
be bound to this or the next rendered source until the control-state effect
and intervening-poll ownership are established.

The poll's float delta is read from global `339f434` by instruction
`c342a6: f3 0f 10 0d 86 b1 76 02`. An adjacent distinct float `339f430`
is read in UpdatePlayer at `c34fd7`. These are native storage facts, not
proof that either represents scaled game delta or unscaled real delta.
Do not substitute a QPC interval for engine delta in CameraABI.

The exact early UpdatePlayer gate is `c33d14` load Main+1d0,
`c33d1d: 75 23` skip when inMenuMode is nonzero,
`c33d1f: 40 38 77 2a` compare Main+2a with preserved zero SIL, and
`c33d23: 74 1d` skip when freezeTime is zero. Only the remaining frozen
branch tests predicate `102b5c0` with EDX3 before calling `c34fa0` at
`c33d3d: e8 5e 12 00 00`. That callee itself skips its ordinary player
update when menu/freeze is set. Thus this is a special frozen-path update,
not proof that normal gameplay's new poll happens after its player simulation.

A full executable direct-call scan additionally finds Poll caller `bd764e`
and wrapper `c34eb0`, besides OnIdle's `c342b5`. Wrapper `c34eb0` polls at
`c34ec3: e8 78 d6 a3 00` and dispatches PlayerControls at `c34ee7`.
It is called from `bd6684` and `bd6940`; each is followed immediately by
UpdatePlayer calls `bd6690` and `bd694c`. Those callers contain a `Sky`
string, which is not sufficient to name them as loading paths.

The third poll is inside an actual worker loop: `.pdata` fragment `bd7478`
waits on object+58 with WaitForSingleObjectEx, resets event+60, branches on
work mode+68, later calls Poll at `bd764e: e8 ed ae a9 00`, and loops back
from `bd7658` to `bd7505` while mode remains2. That body also dispatches the
input queue at `bd756e`, `bd7581` and `bd7594` before the next poll. At
`bd775c` it sets event+60 and checks exit byte+6c. These are worker/event and
poll-order facts, not a proven gameplay/loading classifier or proof that the
worker cannot overlap normal gameplay. Local receipts are
`cpu2-poll-callers.json`, `cpu2-bd65a0.txt`, `cpu2-bd6850.txt` and
`cpu2-bd7478.txt`.

Consequently, a prospective source implementation must observe every native
Poll generation and reject overlap or intervening polls. Hooking only
OnIdle's poll and assuming it is unique is disproved. The existing
SOURCE_BIND contract asserts the branch that starts consuming sampled state,
not completion of every consumer: the proved PlayerControls branch is a
candidate late-control source boundary under that narrow contract, but its
association with the current or following redraw and the worker's admission
state still need proof for broad timeline readiness. The conditional
implementation below adds only the admitted late-control source facts.

The CPU helper `cpu_source_is_current(copy)` now checks active admitted Swap
identity, one primary render association, nonzero source/submission and exact
session/source/submission/began-QPC/frequency equality. The renderer must
separately prove its WorldBegin return address `21f0aeb`. This predicate
also rechecks native input generation/state for a reserved source, while a
rendered fallback makes no such input assertion. It does not classify gameplay.

The parent's callsite is needed to distinguish normal and pause WorldBegin:
normal DrawWorldAndUI calls `21f0ac0` at `c382a0` (return `c382a5`), while
DrawGamePause also calls **the same function** at `c39d1e`, bytes
`e8 9d 6d 5b 01` (return `c39d23`). Both subsequently reach the viewport
caller's return `21f0aeb`. Therefore that viewport return alone proves neither
ordinary gameplay nor a non-pause WorldBegin. The renderer must admit the
correct parent scope before applying an ordinary-world-only flag. This
corrects the initially incomplete interpretation of WorldBegin as a unique
normal-world entry.

Native activation routine `c37c00` derives its active boolean from argument
DL and an additional native override. It writes manager.pollingEnabled+35 at
`c37c2b: 88 58 35`, and writes Main.gameActive+27 at
`c37ce3: 88 5f 27`. This confirms their relationship and offsets but still
does not classify loading, menus or gameplay merely from gameActive=true.

Finally, the reporter now calls adapter `finish_render_scope()` after the
unchanged Swap returns and before FRAME_END or clearing current identity.
The render owner closes only leases belonging to that source. This closes
the previously missing handoff ordering; actual Present identity and result
still require the render owner's native evidence and runtime validation.

## Current prospective implementation and source review

The coordinator admitted the existing prospective fact contract for the
verified late-control path. `cpu_events.cpp/h` now uses seven guarded native
hooks: Swap, the two RAWINPUT handlers, OnIdle, the shared Poll dispatcher,
the input queue dispatcher and actual PlayerControls receiver `ddbe50`.
No generic runtime/SDK file, capability, vendor selection or pacing policy
was changed. INPUT_TIMELINE remains disabled; the adapter emits neither
INPUT_SAMPLE nor SIMULATION_BEGIN/END. The generic runtime owns any later
stage derivation and must distinguish it from measured engine facts.

The native admission sequence is:

1. OnIdle is admitted only from Main::Run return `c315c0`, at top-level
   OnIdle depth, with the native Main singleton `32dd130` matching RCX.
2. Every invocation of native dispatcher `1672540` increments the same
   adapter poll generation and in-flight count, regardless of caller. This
   includes OnIdle `c342b5`, wrapper `c34ec3` and worker `bd764e`; no
   non-main poll is skipped or replaced. Only the selected main return
   `c342ba` can reserve a prospective ID.
3. Reservation requires no other poll or RAWINPUT writer in flight, no
   already open CPU source, the expected Main/manager/queue/PlayerControls
   pointers and PlayerControls vtable, active Main, zero quit/reset/reload/
   freeze/next-freeze/menu flags, and manager.pollingEnabled+35 nonzero.
   The full native flags, polling-enabled byte and owner pointers are
   captured and rechecked later, not just before Poll.
4. RESERVE runs before the unchanged native Poll. QPC is measured after
   RESERVE returns immediately before that Poll, then SAMPLED runs after
   Poll returns only if the generation, writer revision and native state
   remain unchanged. It reports SOURCE_PRODUCED with unknown delivered
   kinds, rather than guessing keyboard/mouse/controller activity.
5. The selected queue dispatcher return `c342de` scopes receiver admission,
   but does **not** emit BIND itself. The native dispatcher acquires its
   lock, selects the queue head and decides whether to call the receiver.
   Only actual PlayerControls entry `ddbe50`, reached from native virtual
   call return `1673b74` inside that admitted dispatcher, emits FRAME_BEGIN
   with the reserved ID followed by SOURCE_BIND. A generation=-1 receiver
   that merely adopts the current native generation never reaches this
   receiver hook, so it cannot claim consumption. Receiver prologue guard
   is `48 8b c4 56 57 48 83 ec 68`.
6. Before binding, native queue generation+1490 must equal the value read
   after Poll, and the native pointers/state, poll generation, writer
   revision and in-flight counts must still agree. Those checks and the
   queue generation are repeated after host callbacks before native
   PlayerControls continues. No engine lock is added by the adapter.
7. The source remains associated with the verified Swap, then closes native
   render leases before FRAME_END. Intervening/overlapping polls, RAWINPUT
   mutation, changed native mode/state/owners, an unrecognized Swap caller,
   or quiescence withdraw the prospective binding. A later valid Swap can
   open a new rendered-only fallback ID. OnIdle return aborts an unconsumed
   reservation, or withdraws a bound source that did not reach redraw, and
   closes any CPU frame. IDs are never reused.

Only one pending native identity is stored per admitted engine thread, with
global native poll and RAWINPUT mutation counters. Reservation validation,
ownership history and runtime behavior remain in the existing generic core
ledger. No alternative adapter ledger or polling framework was introduced.
Counters observe all three poll callsites even though only one can reserve.
Main state snapshots are conservative eligibility facts, not a complete
gameplay/loading classifier. No predecessor Present is asserted when unknown.

Review found and corrected three concrete gaps. First, the initial snapshot
checked manager.pollingEnabled only on entry; it now captures, compares and
requires that byte on every follow-up state check. Second, an unrecognized
Swap could expose an existing thread-local identity to render callbacks;
explicit admitted-Swap visibility now suppresses unowned/nested calls and
restores parent visibility afterward. Third, emitting BIND at queue-dispatch
entry preceded its lock and its generation=-1 no-consumption branch; BIND
now occurs at the actual selected receiver entry. A rejected/aborted bound
source cannot silently become rendered fallback under its old ID. Native
LastError is preserved around callback and caller-admission work.

Read-only scalar verification checked all13 production expected-byte guards
against the installed executable; every guard matched. Receipt:
`.local/fallout4-research/cpu-guard-review-20261116.json`. Source review covered
all three poll callers, overlapping and intervening polls, disabled polling,
native-state/raw-input mutation, skipped receiver callbacks, nested/wrong
Swap callers, no-redraw return, quiescence and partial hook setup/removal.
These are static/scalar checks, not runtime acceptance or a native test run.
The coordinator reported its earlier batch gate as221/14 before this final
review; this worker did not build, launch, deploy or run tests, and that
earlier batch does not verify the subsequent seven-hook review changes.

### Generic latency admission still required

BIND proves entry into the late PlayerControls update that resets movement/
look input and processes the sampled native queue. It is not proof of the
beginning of all world simulation: earlier engine work may already have run,
other consumers can follow, and worker/mode paths remain separate. An SDK
simulation span starting there would therefore be a **core-derived** span,
not an adapter-observed engine SIMULATION_BEGIN. No such span is emitted here.

The coordinator's generic source review identified a remaining admission
gap: rendered-source `start_render_marker` returns early, while the
non-rendered graphics-start path derives SIM_END and normalizes absent
SIM_BEGIN with INPUT_SUBSTITUTE even when a reservation supplied a real poll;
CpuOwner refuses that prospective substitute. See
`runtime/orchestrator/src/native_fg.cpp` (`start_render_marker`) and
`native_cpu.cpp` (missing-stage normalization). A later generic solution must
establish and label derived begin/end semantics and preserve owner/completion
gates for every adapter. Fallout 4 must not select a special core branch or
manufacture SIM events to bypass this refusal.

Admission also requires a matched ordinary WorldBegin parent, unchanged
source/input generation, actual primary render/submission and outer Present
ownership, complete native resource/camera facts, explicit failure and
no-redraw handling, and bounded runtime validation across normal gameplay,
pause, menus/loading, resize, quiescence and F4SE/ENB coexistence. Negative
cases above must keep their native rendering path or rendered-only provenance;
they cannot advertise complete input timeline or SDK latency readiness.

## Screen eligibility research handoff

This follow-up is source/native research only. Production remains frozen;
no classifier, menu hook, option or runtime instrumentation was added. The
question is the smallest game-owned way to distinguish main/loading/menu/VATS
rendering from an ordinary world constructor without marking every WorldBegin
as GAMEPLAY. It uses the executable identity and pinned revisions above.

Pinned `src/World/Sky.cpp:152` implements `IsMainOrLoadingMenuOpen` with
`UI::GetMenuOpen<MainMenu>()` and `GetMenuOpen<LoadingMenu>()`. CommonLib
`UI.h` resolves those names under the native menu-map lock and checks
`IMenu::OnStack`, not merely whether the menu object exists. `IMenu.h:160,202`
and `UI_MENU_FLAGS.h` give menuFlags+58 and on-stack bit0x40. Those source
leads now have a matching installed native query, so no container reader or
F4SE runtime dependency is necessary.

### Verified native query contract

Read UI singleton pointer from module RVA `30e8930` during the engine-owned
world callback. Native query `1a80320` has x64 ABI
`bool(UI*,const BSFixedString*)`, RCX/RDX, boolean in AL. Entry bytes are
`48 89 5c 24 08 57 48 83 ec 40`. Native instructions lock global `3dbd370`,
look up UI+1a8, retain a temporary menu result, unlock, test menu+58 bit0x40
at `1a80377` (`f6 41 58 40`), then release the temporary at `1a80388`.
It returns false for an absent menu. Source F4SE0.7.9 `GameMenus.h` names this
same installed RVA as `UI::IsMenuOpen`, independently corroborating its ABI;
the adapter calls the game's function directly and does not load F4SE to do so.

Native no-argument cached-name getters return borrowed addresses of engine
`BSFixedString` objects, not raw `char*`. Their native lazy initialization
interns the literal and returns the stable engine-owned object. Do not build
an engine string with a copied C++ layout or pass a string literal as that
object. Keep these pointers game-side and use them only while the native
module and callback lifetime are established.

| Fact | Getter RVA | Literal LEA RVA and exact bytes | Literal RVA |
| --- | --- | --- | --- |
| LoadingMenu | `1a87520` | `1a87575: 48 8d 15 cc 8f b2 00` | `25b0548` |
| MainMenu | `1a879a0` | `1a879f5: 48 8d 15 44 7c ab 00` | `253f640` |
| VATSMenu | `1a890b0` | `1a89105: 48 8d 15 3c 8e ab 00` | `2541f48` |
| PauseMenu | `1a87e20` | Recorded in scalar receipt below | `271de40` |
| PipboyMenu | `1a87fd0` | Recorded in scalar receipt below | `253cde8` |
| MapMenu | `1a87a30` | Recorded in scalar receipt below | `271ddf0` |

All six getters begin `40 53 48 83 ec 20`. Guard their literal/return LEAs
as well as that shared prologue; a prologue alone is not an identity check.
`screen-native-query-contract-20261116.json` records each getter's exact
entry bytes, literal LEA, cached-object return LEA and object address.
Native callers independently confirm the getter-to-query ABI: `c33f54`
gets LoadingMenu and `c33f5f` calls `1a80320`; `2d4504` gets MainMenu and
`2d450f` queries it; `acd313` gets VATSMenu and `acd31e` queries it.
These are current installed instructions, not old address-library offsets.

The minimal future reader can return separate factual booleans for these
queries, UI/menu freeze state, Main eligibility, camera mode and the verified
ordinary WorldBegin parent. LoadingMenu open supports LOADING; MainMenu open
supports MENU. Pause, Pip-Boy and map are explicit menu facts. VATSMenu is an
explicit VATS fact, for which the existing screen enum has no dedicated value;
retain that exclusion/unknown fact instead of calling it ordinary gameplay.
Query failures, unavailable owners or changed source scope retain UNKNOWN.
Several separate native queries are point observations, not an atomic combined
snapshot: associate them with the source and recheck at later FG/publication
boundaries if their consistency is required. Do not cache a UI object or
menu pointer across shutdown or infer state from menu allocation existence.

All known menu queries returning false is not positive gameplay proof. The
future ordinary-world basis also needs the parent `c382a5`, eligible Main,
the same source generation and valid leased main-world camera/scene resources.
Pause parent `c39d23` remains distinct even though both reach WorldBegin's
viewport return `21f0aeb`. Non-pausing/third-party menus, cinematic camera
modes and loading transitions outside these observations remain coverage and
runtime-validation gaps; there is no blanket GAMEPLAY assignment here.

### Camera state and correction to the frozen update interpretation

Read PlayerCamera singleton from module RVA `30e6e58` during its engine-owned
callback. Native `102b5c0` is `bool(PlayerCamera*,uint32_t stateIndex)`:
it reads currentState+28, rejects null, and compares that pointer with
`cameraStates[stateIndex]` at +e0. Exact function bytes are
`4c 8b 41 28 4d 85 c0 74 0f 8b c2 4c 3b 84 c1 e0 00 00 00 75 03 b0 01 c3 32 c0 c3`.
Pinned `PlayerCamera.h`/`CameraStates.h` name this `QCameraEquals`, with
VATS index2 and free-camera index3. PE RTTI identifies PlayerCamera primary
vtable `25aa6d8` and VATSCameraState vtable `25ab3a8`; the enum names remain
source-correlated semantics, while the query's array/current-state comparison
is directly verified. Validate index range0..12 and the live camera owner.
VATSMenu open and VATS camera match are complementary facts: absence of the
menu alone does not establish that playback/cinematic camera is ordinary.

This corrects an earlier message's description of native `102b5c0` as a UI
predicate. At `c33d31`, RCX is PlayerCamera and EDX is3. The special
`c33d3d` early UpdatePlayer call therefore requires **not inMenuMode,
freezeTime set and current camera equal to free camera**. It is not proof
of an ordinary gameplay simulation begin. The earlier section's native
branch bytes remain valid; the camera semantics are now resolved.

UI's separate freeze counters also have direct native readers:
`1a80480` tests UI+1ec, bytes `83 b9 ec 01 00 00 00 0f 97 c0 c3`, while
`1a804a0` tests UI+1f0, bytes `83 b9 f0 01 00 00 00 0f 97 c0 c3`.
These match CommonLib freezeFrameMenuBG/freezeFramePause. They describe
freeze presentation facts, not named menus or loading by themselves.

### History reset facts and limits

Native camera-state assignment `827dd0` has x64 ABI
`void(TESCamera*,TESCameraState*)`; source F4SE `GameCamera.h` corroborates
that RVA. It calls old state End at `827dec`, compares the old/new state,
updates reference counts and writes currentState+28 at
`827e01: 48 89 5f 28`. A changed camera-state pointer is thus an observed
camera-state transition. It is not a universal camera-cut/teleport signal;
cuts can occur inside one persistent state, and two transitions between
observations can return to the same pointer.

Pinned `Feature.cpp:44` requests history reset on the actual LoadingMenu
close event, and `SharedData.cpp:466` latches it only after main/loading menus
are no longer open. Those are reference reset policy and source event
semantics. In the installed LoadingMenu ProcessMessage `1066980`, message
type at UIMessage+10 branches show1/hide3, but hide handling can be ignored;
a hide request alone is not proof that the menu closed. Sampling native
IsMenuOpen true then false proves only an **observed** loading-close
transition. First source, resource/extent changes, camera-state changes,
projection changes and missed-frame/history gaps can supply their own
explicit reset facts where already verified. None should be relabeled as
a native all-cuts flag.

For retained evidence only, RTTI LoadingMenu `25b0458` and MainMenu `25b1258`
share virtual OnAddedToMenuStack slot0b `a72d60` and OnRemovedFromMenuStack
slot0c `a72da0`. Their native bases `1b19b80`/`1b19ba0` set/clear menu+58
bit0x40 and invoke display-state change. No observer was implemented, and
no hook/lifetime expansion is part of this handoff. A lossless native
transition callback, if later required, still needs its native caller and
lifetime contract verified before activation.

Local untracked evidence: `screen-native-query-contract-20261116.json`,
`screen-menu-string-getters.json`, `screen-getter-callers.json`,
`screen-vtables.json`, `screen-menu-stack-vfuncs.json` and the corresponding
`cpu2-*.txt` disassemblies and `screen-*.py` read-only scripts. No shared REA
session was used. No build, runtime test, game launch or deployment was
performed for this screen research, and no production source was modified.

## Proposed generic derived SDK interval, source review only

This proposal changes no production code, capability or public ABI. It answers
how the generic core could handle a real prospective poll bound at an actual
consumer when the engine cannot expose complete SIMULATION events. It is an
SDK interval derived from consumer entry to an observed update-to-render
boundary, not a claim to measure the whole world's physics simulation.
The same policy must apply to any adapter supplying these facts.

### Existing behavior and the precise gap

`native_cpu.cpp::source_event` already promotes a real `poll_begin_qpc` into
INPUT_SAMPLE storage only after valid SOURCE_BIND, without emitting a second
input callback or moving the sample to FRAME_BEGIN. This must remain intact.
`native_fg.cpp::rsf_native_fg_cpu` acquires the prospective token and paces
at RESERVE, adopts it at FRAME_BEGIN, and refuses prospective substitute
SIM_BEGIN. It emits simulation-start only at an accepted SIM_BEGIN, never at
the reserved poll. Those ownership/pacing rules are also correct to retain.

The gap is `start_render_marker`: in input-timeline mode it may dispatch a
derived SIM_END, but missing SIM_BEGIN normalization marks that stage
INPUT_SUBSTITUTE even when input itself was the real bound poll. CpuOwner
correctly refuses that prospective substitute. In rendered-source mode,
`start_render_marker` returns before CPU marker derivation altogether.
Changing the substitute flag alone is therefore insufficient.

There are two other constraints. A valid BIND does not currently promote
the frame's INPUT_KEYED flag; a frame whose adapter reported flags0 can have
a real promoted sample yet miss canonical end inference/marker ordering.
Also, the current duplicate path only replaces a derived SIM_END and then
clears the aggregate DERIVED_BOUNDARY flag. That cannot preserve the
provenance of a still-derived SIM_BEGIN, nor replace that begin with a later
real engine fact. These are generic bookkeeping issues, not Fallout 4 policy.

### Smallest proposed production diff

1. **Private CPU-frame provenance in `native_cpu.cpp`.** Add a small derived
   stage mask to each existing private frame record, alongside its existing
   public `rsf_native_cpu_frame` payload. Do not add another identity table,
   reservation ledger, public field or ABI version. Existing public reads
   continue copying exactly `rsf_native_cpu_frame`, with its original size.
   Record the mask when a derived stage is accepted and compute the public
   aggregate DERIVED_BOUNDARY flag from the remaining derived stages.

2. **Bind-derived begin in `rsf_native_cpu_dispatch`.** After SOURCE_BIND has
   passed existing validation, its synchronous source sink has returned,
   and the same source remains BOUND/live/nonfailed, derive SIM_BEGIN only
   when a real nonzero poll entry was promoted and the stage is missing.
   Use the accepted `bind_qpc`, matching session/frequency and source ID,
   flags INPUT_KEYED|DERIVED_BOUNDARY, and the existing normal dispatch path.
   It must run outside `guard`, rechecking the reservation because the sink
   may reenter/abort. Do not emit INPUT_SAMPLE again, pace again, acquire a
   second token, or derive anything at RESERVE/SAMPLED. For this accepted
   bound source, generically establish INPUT_KEYED canonical ordering in
   the stored frame even if its FRAME_BEGIN packet had flags0; this is
   proven input ownership, not a capability/configuration change.
   Duplicate/actual-stage handling must consult that stored canonical mode,
   not just the later packet's flags, so an otherwise valid later v1 fact
   with flags0 cannot bypass dedup and fail merely because the core promoted
   its frame at binding.

3. **Bound missing-begin normalization.** At the existing SIM_END normalization
   point, distinguish a BOUND reservation with a real promoted sample from
   missing input. If such a frame still lacks SIM_BEGIN, the same begin
   derivation above can recover it at `bind_qpc`, not at graphics-start QPC.
   Never clear INPUT_SUBSTITUTE on a genuinely substituted input; unknown
   poll entry, withdrawn/unbound reservation or missing production remains
   ineligible. Preserve the existing BIND `stage_mask==1` admission restriction
   for this minimal change: an engine already reporting simulation before
   binding retains its actual facts and existing bind refusal. Supporting
   that different ordering is separate work, not a reason to weaken this gate.
   A derived begin leaves mask7 when an engine skips rendering before any
   end fact. Extend partial CPU closure only for that explicit
   FRAME_END|RENDER_SKIPPED case (including an aborted reserved source), so
   it can retain an honest incomplete record without inventing SIM_END or
   counting a spurious order violation. Unflagged mask7 closure remains
   refused. CpuOwner's existing incomplete/skip test must still abort its
   token and refuse FG; this is record closure, not completion promotion.

4. **End inference in `native_fg.cpp`.** For a prospective bound source,
   dispatch a missing SIM_END with INPUT_KEYED|DERIVED_BOUNDARY at the first
   accepted primary AFTER_SIMULATION scene/submission boundary, using that
   callback's current QPC. Require matching CpuOwner/session/provider
   generation, current reservation BOUND, real poll entry, and the existing
   submitted/scene association. Do not infer it merely because a Present or
   generic `rsf_native_latency_render(id)` fallback called
   `start_render_marker`: those entry points contain no constructor proof
   by themselves. Preserve existing non-prospective behavior separately.
   Dispatch SIM_END before RenderSubmitStart; if the prospective derivation
   or marker preconditions fail, fail/refuse that owner and use existing
   abort/retirement paths, without freeing live GPU leases or manufacturing
   missing markers. Keep rendered-source early return unchanged.

5. **Actual-fact precedence and status.** Generalize duplicate replacement
   for both SIM_BEGIN and SIM_END when their particular recorded stage was
   derived. Validate session/frequency and chronology before replacement;
   inconsistent actual timestamps remain a refusal/order problem. Replace
   metadata and clear only that stage's derived bit, with no second SDK
   marker or sleep. Ordinary actual duplicates retain first-fact behavior.
   The SDK marker already emitted at the derived boundary cannot be moved
   backward later; report that distinction. Increment the existing generic
   derived-boundary status count once for each newly accepted inferred SIM
   stage in the FG sink. Move the existing inferred-end increment there
   instead of double-counting it at `start_render_marker`. Counters remain
   historical even if a later real fact supersedes stored metadata.

No CpuOwner INPUT_SUBSTITUTE refusal, final `reserved_bound` predicate,
provider generation check, pre-poll `ready_qpc <= poll_begin_qpc`, frame/window/
submission identity check, completion gate, token-capacity rule or resource
lease rule should be loosened. Existing `measured_valid` naming/status should
not be read as proof of a wholly measured simulation interval: input can be
real while the SDK simulation boundaries are derived and labeled as such.

### Required negative cases and focused validation

| Case | Required behavior |
| --- | --- |
| Rendered-only host/capability mode | No reservation sink, new SDK begin, inferred pace or input ownership |
| First reservation before a session exists | Preserve existing refusal; no implicit session bootstrap from the proposal |
| Short/old SAMPLED packet, poll entry0 | No derived input ownership or bound-derived begin |
| Poll not produced; overlap/intervening poll; ABORT; unbound/adopted-only source | No new SIM marker; retain real CPU facts and rendered fallback/refusal |
| RESERVE predecessor pending timeout/failed/cancelled/unverified | Preserve token/pacing refusal and cleanup |
| Sink aborts/reenters during BIND | Recheck BOUND before begin dispatch; no stale callback |
| Provider generation changes, token acquisition/pacing fails, poll precedes ready time | Existing owner/final admission still refuses |
| No verified primary AFTER_SIMULATION constructor/submission, wrong source/session/view | No derived end merely at Present; no completion promotion |
| No redraw/skipped render or quiescence after bind | Close/abort normally; do not invent an end to qualify FG |
| Duplicate BIND/render callback or repeated derived stages | Exactly one start/end marker and one pre-input pace |
| Later actual begin/end | Preserve valid actual timestamp metadata, per-stage provenance and no SDK replay |
| Existing actual SIM stage before BIND | Retain actual fact and existing bind refusal in the minimal patch |
| Legacy/v1 input substitute or real input-after-simulation ordering | Existing behavior/refusals stay covered by their current tests |

Extend `tests/native_cpu_reservation.cpp` for real poll->bind-derived begin,
flags0 frame adoption, reentrant withdrawal, duplicate and missing-poll cases;
its current assertions around lines270..280 deliberately expect mask3 and no
SIM callback after bind, so those expectations must change only in the new
input-mode policy case. Keep its rendered-only tests unchanged. Extend
`tests/latency_event_contract.cpp` for independent begin/end actual precedence,
aggregate provenance and invalid-frequency/time replacements. Existing
`tests/d3d11_fg_api.cpp` ordering tests remain guards against arbitrary
out-of-order CPU completion. A focused generic FG fixture must additionally
prove RESERVE token acquisition/pacing precedes poll, marker order is
SimStart->SimEnd->RenderSubmitStart->RenderSubmitEnd->PresentStart->PresentEnd
once, constructor absence does not promote, and abort/provider-switch cases
retain their ownership/resource behavior. CPU-ledger tests alone cannot prove
vendor callback ordering or physical latency.

The current native D3D owner routes accepted markers through
`rsf_native_cpu_marker` and then the selected presentation provider, so the
proposal can reuse its canonical order/dedup and vendor fallback machinery.
It does not add an SDK-specific marker interpretation to any adapter.
This source review establishes compatibility with those current code gates,
not external vendor certification or device acceptance. Vulkan's current
sink has no prospective RESERVE/BIND owner path and still starts keyed SDK
simulation at INPUT_SAMPLE: this proposal must not accidentally enable a
prospective Vulkan host or feed it a second/late pacing sample. A future
Vulkan migration needs its own generic owner integration under the same
policy, without a game-name branch.

The XeLL provider's current `xess_generation.cpp::begin` performs its sleep
and creates the source-to-SDK-frame mapping together; later `marker` refuses
an unknown source mapping. Keeping that call at the existing pre-poll RESERVE
path and adding only markers after BIND preserves this requirement. Moving
sleep/token establishment to derived SIM_BEGIN would not. Likewise keep
Reflex token acquisition and pacing at RESERVE and Anti-Lag owner selection/
input update in its existing core path. The proposal changes neither the
chosen technology nor generated-frame typing or actual Present semantics.

No builds, tests, SDK/device runs or production edits were performed for
this proposal. It is a concrete implementation/review handoff, not a claim
that complete latency admission or the broader Fallout 4 goal is finished.

## Exact native Present and submission-close handoff

The later read-only task resolved the actual native Present path, correcting
the earlier `17eeed0` lead. No production hook was added. Direct native-call
search and installed instructions establish:

| Boundary | Guard / native behavior |
| --- | --- |
| Selected Main::Swap | `c346ac: e8 9f eb ff ff` -> `c33250`, return `c346b1` |
| World/UI construction | `c33434: e8 07 4e 00 00` -> `c38240` |
| Normal UI and late rendering | `c383c2` -> `21fa200`; `c383f6` -> `21fa3b0`; `c383fb` -> `21fa930`; subsequent UI tail calls remain before Swap's Present call |
| Late camera/object work | `c335db` -> `17eeed0`; this remains **not** the COM Present |
| Exact selected Present routine call | `c335e0: 48 8d 0d 19 27 df 02` supplies Renderer `3a25d00`; `c335e7: e8 64 4f be 00` -> `1818550`, return `c335ec` |
| Native Present routine entry | `1818550: 40 53 48 83 ec 20`; ABI `void(BSGraphics::Renderer*)`, x64 RCX |
| Actual outer chain call | `181859b: 41 ff 51 40`, return `181859f`; ABI `HRESULT(IDXGISwapChain*,UINT,UINT)` in RCX/EDX/R8D |
| Retry call | `18185d5: 41 ff 51 40`, return `18185d9`; same receiver/argument reloads |

At `181856d` the native routine tests Renderer+0, source-correlated as
`skipNextPresent`, and can clear it and skip Present. Otherwise it loads the
current RendererWindow from global `38e13b0`, then RCX from window+18, the
actual engine-held swap-chain object. R9 is that object's vtable and slot8
(+40) is invoked directly. This is the engine's outer chain; do not replace
the receiver with a queried inner chain. The native creation proof already
ties the primary window/chain fields to the same owner.

EDX sync interval comes from `[RendererData(global 38e13a0)+40]`.
R8D Present flags come from global **`3d872dc`**, computed from the installed
RIP-relative load `1818579: 44 8b 05 5c ed 56 02`. An earlier coordinator
message gave `3d873d8`; that arithmetic was wrong and is superseded here.
The routine sets in-Present state `38e13c0` around the call. Flags8 plus
HRESULT `887a000a` (DXGI_ERROR_WAS_STILL_DRAWING) cause SwitchToThread and
retry through `18185d5`. Preserve each native call, receiver, flags and
result handling; a retry or skipped Present is not a new source frame.

Thus native routine entry `1818550`, admitted only from return `c335ec`
inside the selected Swap/source, is the verified pre-Present CPU boundary
for closing **native render SUBMISSION**. All observed native world/UI and
late-frame work in Swap precedes that call, and it is before outer COM
Present can enter any inner RSF prepare callback. It does not claim GPU
completion, final wrapper post-processing completion or successful display.

Keep WINDOW/FRAME/output leases and copied source identity through actual
Present and the outer wrapper's pre/post work. Closing FRAME before Present
would trigger the existing native FG live-token abort; only SUBMISSION
closes at the new boundary. The existing after-Swap finish remains the
closure point for remaining leases. ENB and other wrappers may perform their
own GPU work inside Present, so native submission closure must not bypass
those calls or release their output resources. Other native callers of
`1818550` include worker `bd7629` and unrelated paths; they are not admitted
by the selected return-address guard.

Untracked receipts are `present-native-callers.json`,
`present-renderer-owner-xrefs.json`, `cpu2-1818550.txt`, `cpu2-c38240.txt`
and `cpu-c33000.txt` under `.local/fallout4-research/`. This was static
instruction/source review only, without builds, a game run or shared REA.
