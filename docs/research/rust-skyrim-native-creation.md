# Skyrim SE native creation in Rust

This note covers the first Skyrim SE Rust slice: the cold native creation root and its Steam
Present cooperation moved from `games/skyrim-se/src/native_creation.cpp` and
`games/skyrim-se/src/steam_present.cpp` into the new `rsf-skyrim-se` static library, linked into
`ReScaleFrame.Game.SkyrimSE.dll`. It is a behaviour-preserving source port without a new
experiment. Nothing was compiled, linked, Cargo-tested, Clippy-checked or CTest-run for it, and no
game, window, device or deployment run was made. The user owns that verification.

## Why this owner first

`plugin.cpp` (GameSDK identity and lifecycle) orchestrates nine C++ sub-owners and five MinHook
observers whose trampolines it owns. Porting it first would leave every observer and its
trampoline in C++ behind a table of lifecycle callbacks. The creation root is self-contained: one
guarded hook, two route profiles, one Runtime transaction, the Steam facade and the Community
Shaders gate. Its C++ entry points (`native_creation_prepare/start/quiesce/stop/busy`) stay, so
`plugin.cpp` and its lifecycle order are untouched.

## What moved

| Former C++ | Rust | Owns |
| --- | --- | --- |
| `native_creation.cpp` | `layout.rs` | thunk `+15b9412` and its bytes `ff 25 c8 fe 20 00` (IAT `+17c92e0`), the game call `+1012320` (`e8 ed 70 5a 00`) and return `+1012325`, renderer owner `+3330188` with device `+56`, context `+64`, window `+72`, swapchain `+96`; Display Tweaks 0.5.25 module name and SHA-256 `df7f352a...ba9f4`, relay `jmp [rip+0]`, entry `+27250` and its 20-byte prologue, invoke `+272e9` and its 77 bytes, retained slot `+5e168` and return `+27334` |
| `native_creation.cpp` | `creation.rs` | thunk guard, both route checks with module pinning and fingerprint caching, `VirtualQuery` mapping rules, cold-owner rule, prepare/start/quiesce/stop/busy state, Runtime export lookup, trampoline publication, call lease, refused-route reports (at most four), owner/output/window admission, Display route receipt, mode-3 `begin`, post-call output verification, `end`, the scoped-end receipt and last-error restoration |
| `steam_present.cpp` | `creation.rs` | the cold-owner callback handed to `rsf::steam::prepare_d3d11_chain` |

`native_creation.cpp` now holds only native leaves, lent through `native_creation_platform.h` as a
size- and version-checked table: the SEH-guarded copy (VirtualQuery-checked under mingw),
`VirtualQuery`, module handle, export, module file SHA-256 through `rsf::sha256_file`, module pin,
`GetLastError`/`SetLastError`, MinHook create/enable/disable, exception guards around the host log
and the Runtime's `rsf_d3d11_host_creation_begin/end`, the Community Shaders gate
(`compatibility_render_allowed`) and the shared Steam facade. The detour itself also stays native.
It fills a call record with the native return address, the description and the three output
pointers, calls `rsf_skyrim_creation_enter`, calls the original on its own frame, and a destructor
calls `rsf_skyrim_creation_leave` on return or unwind. No Rust frame sits between the game and the
original creation call, so a foreign exception or SEH unwind cannot cross Rust, and the Runtime
scope is still closed on unwind as the former `Scope` destructor did. The record and table layouts
are hand-mirrored in `platform.rs` because bindgen-cli 0.72.1 is not installed here; both sides
assert sizes and offsets, and `RSF_D3D11_HOST_ABI_VERSION` reaches Rust through the table.

The Steam overlay owner is not duplicated. Rust calls the existing facade through the table, and
that facade calls the `rsf-steam` archive with its own platform. The DLL therefore links two Rust
static archives, as `rsf_native_vulkan` already does inside the Runtime.

## Preserved behaviour

- DLL name `ReScaleFrame.Game.SkyrimSE`, the `rsf_get_game_plugin_api` export, ABI 24 info,
  detection and every lifecycle call in `plugin.cpp` are unchanged.
- Both supported creation routes: the game's own call returning to `+1012325`, and the exact
  Display Tweaks 0.5.25 relay returning to its `+27334`, with the same fingerprint, pin,
  mapping and byte checks in the same order. Any other caller or byte difference refuses.
- Prepare still refuses while a call is in flight or live, logs thunk/route refusals, requires a
  cold owner, prepares the Steam chain before resolving the Runtime services, creates the hook once
  for the process and logs the same prepared text. Start still re-runs the guard and accepts an
  already enabled hook; stop still accepts an already disabled hook and reports busy while a call
  holds its lease.
- Admission order inside the call is unchanged: live, verified caller, owner, the three output
  pointers, description, `OutputWindow` equal to the owner window, the Community Shaders gate, then
  the Steam facade. Only then does `begin` run. The original receives the current IAT and every
  argument; its HRESULT and outputs return unchanged.
- After the call, outputs are lent to `end` only if the call returned success, the owner is still
  live and allowed, and owner, window, device, context and outer chain all match. Otherwise `end`
  cancels. The entry last error is restored before the original and the native last error after
  `end`.
- Refusal and receipt texts are byte-identical. The scoped-end receipt prints the outer chain as
  sixteen upper-case hex digits, matching MSVC `%p`.

## Deliberate differences

- Every host log call is exception-guarded. The former prepare, start and detour logs were not, so
  a throwing log could reach `plugin.cpp`'s catch (prepare returned `NOT_READY`) or unwind into the
  game. The log is now swallowed and the call continues.
- The cold-owner check for the Steam facade reads through the SEH-guarded copy instead of
  `ReadProcessMemory`. Both are non-faulting reads of the same two pointers.
- The per-thread `refusal` string became the route check's return value. The logged text for each
  failure is the same.
- A null trampoline in the detour returns `E_FAIL` instead of calling null. MinHook enables the
  detour only after the trampoline is published, so this is unreachable.

## Checks performed

Source review against the removed C++; the RVA arithmetic is restated as Rust tests (the game call
reaches the thunk, the thunk reads the IAT, the Display invoke reads the retained slot and returns
at the profile return). `rustfmt --edition 2024 --check` with rustfmt 1.11.0 (the pinned 1.94.1
toolchain is not installed), `cargo metadata --locked --offline`, `engine.json` parse and
`git diff --check`. Binding regeneration was not possible without bindgen. Not built or tested.

## Remaining Skyrim owners

Still C++ in `rsf_game_skyrim_se`, in suggested order:

1. `plugin.cpp`: lifecycle (prepare/start/quiesce/stop/status, process-lived D3D11 host session and
   its `rsf_d3d11_host_setup` identity, units, motion, capability bits, presentation bridge 3 and
   window class policy, per-monitor-v2 DPI request), plus five observers and their guards: jitter
   `+101e360`, ProcessMain `+14edbf0` with caller `+153ade7` and TAA bypass bytes `+18`/`+30`,
   reset `+1544df4`, resolution `+101e130`, renderer end `+1009f70` with presentation binding from
   `+3330198`, viewport guard `+1009960`, frame upload `+100ae80`, and the bounded native
   diagnostics. `plugin.rs` now owns the GameSDK identity and eight observer guards.
   `layout.rs` holds the same verified RVAs and bytes, and the platform reuses the cold creation
   path's guarded reads and file hash. Rust checks the platform size before its version and
   publishes the five hook targets only after every guard passes. Source comparison matched
   the profile and guard data. Build, ABI and in-game verification remain open.
   `observers.rs` owns the five observer hooks' creation, activation and disable.
   The start path reuses the same admission as prepare. Failed disable or removal retains
   its hook record so a later stop or prepare can retry. A Rust regression covers a failed
   start with a refused disable. The regression remains unrun.
   `session.rs` now owns the lifecycle; see "Session lifecycle" below. Still C++ in
   `plugin.cpp`: the five typed observer detours, their engine facts (TAA bypass bytes,
   presentation binding read, bounded diagnostics) and the window-class policy.
2. `compatibility.cpp`: Community Shaders 1.9.1 handover (DLL SHA-256 `10ba95e6...097827`, PE image
   validation, six guard sites, `GetProcAddress` interception of `SKSEPlugin_Load`, Feature
   configure `+3a4b30` and HDR `+45cbd0` hooks, state machine, pre-device retention and the
   post-process call scope).
3. `cpu_events.cpp` with `input_ownership.h` and `screen_classification.h`: CPU frame, input
   ownership, prospective source and screen classification (one hook, about forty RVAs). Its
   fixture `tests/skyrim_input.cpp` compiles the source directly.
4. `presentation_inputs.cpp`, `resolution.cpp`, `render_inputs.cpp`, `auxiliary.cpp`:
   pre-HDR inputs, reduced-resolution ownership, SR reconstruction and auxiliary views.
   Rust now owns the sampler hook at `+1010de0` and interface hook at `+116ac80`.
5. `camera.rs` owns the native camera conversion for both SR and presentation. It
   uses the shared GameSDK inverse with the old float output and double accumulation
   rules. The adapter still supplies the same copied row-vector cache. Refusals clear
   the camera output and history flag. `tests/skyrim_camera.cpp` now links the Rust
   archive. Source comparison and layout review were the scope of this change.

`camera/render.rs` now reads the same pre-HDR cache for SR and FG, replacing
their identical native cache and history code. The exact site and camera
convention remain documented in [the integration report](skyrim-se-integration.md).
The old stores were separate, so Rust keeps a receipt per producer with the
same first-sample refusal, QPC interval bound and reset rule. The copied history
holds scalar identities and the prior view-projection, origin bits and jitter.
The guards now refuse overflowing cache addresses and timing differences.
The native consumers still lease their textures and submit their original
passes. Source comparison matched both baseline blocks, including the cache
offsets and bitwise origin check. No build or live test was run.

## Interface and window handoff

The Rust owner keeps the interface hook, capture provenance and the
thread-local pre-HDR receipt. The receipt copies the same scalars and
camera fields and stores the swapchain only as identity. Native callbacks
lease the actual D3D11 backbuffer, context and swapchain for the operation.
Final-scene begin and end precede window begin. The retained window owns
references until renderer-end and keeps retirement busy while a callback is
opening or closing it. A reentrant end waits for the begin attempt to return.
An exception in final-scene delivery skips window publication, and capture
uses the identity matched at entry. The pre-HDR texture producer still
checks its descriptors and hands off through the same GameSDK calls.
Source review matched the hook bytes, copied fields, pre-HDR producer
block and capture names. Builds and live callback ordering are unverified.

## Sampler hook

Rust uses the existing guarded read and MinHook leaves for the sampler flush.
It reads the same cached colour and depth targets, shadow selector, active
ratios, 16 address and filter pairs, and baseline sampler table. The
calculation and restore mask follow the evidence in
[the Skyrim integration report](skyrim-se-integration.md). The C++ flush
and by-name Runtime sampler call keep their signatures. A snapshot of the
host binding is copied before any host callback. A refused export lookup
leaves the hook uncreated, and a refused enable restores the previous
binding. Restore reports now print zero render dimensions when the scene
check refuses. Stop rechecks in-flight entries after native disable and
keeps the trampoline available. Source comparison, layout review and pinned
formatting passed. Build, linkage, device and game checks remain open.

## Session lifecycle

Quiesce clears observer admission both before waiting and under the session lock.
The second clear covers a start that publishes while quiesce is waiting. The
regression is present in source and remains unrun.

Source port without a new experiment. `plugin.cpp`'s prepare, start, quiesce, stop and status,
its session state and the compatibility withdraw callback moved to `session.rs`. Nothing was
compiled, linked, Cargo-tested, Clippy-checked or CTest-run, and no game, window, device or
deployment run was made.

Rust now owns the lifecycle order, results and reasons, the once-per-process per-monitor-v2 DPI
request (through the creation platform's user32 export lookup and last-error leaves), the Runtime
export lookup, the `rsf_d3d11_host_setup` identity (skyrim-se, engine 1.7.104, project id, units
0.0142875, qualities 0x1f, native colour transport, bridge 3, `FG_INPUTS | INPUT_TIMELINE`,
unit motion with invalid -1000), the process-lived host and its original-session check, the
observer hook record and the start order: observers, sampling, presentation inputs, CPU events,
native creation. `session_platform.h` lends a size- and version-checked leaf table: one guarded
subsystem call per C++ operation, the compatibility checkpoint and status, a guarded host log,
`GetModuleFileNameW`, guarded Runtime create and activate calls, the observer leaves and the
window-class callback. `rsf_d3d11_host_setup` and `rsf_motion_decode_params` are mirrored by hand
with size and offset asserts on both sides. The `rsf_skyrim_creation_*` and
`rsf_skyrim_observers_*` exports, `native_creation.h` and its C++ wrappers are gone; the session
calls those Rust owners directly.

One lock now serializes the lifecycle, the services lent to the detours and the withdraw
callback. Detours take a copy through `rsf_skyrim_session_lease` (only while accepting, except the
renderer-end detour, which keeps reading the retained services so pending auxiliaries finish) and
never hold it across native execution. The presentation-binding export also comes from the lease.
Previously quiesce read the services and activation export without the lock and the renderer-end
detour read the binding export unlocked. Quiesce and the withdraw callback still clear accepting
before taking the lock, and the callback takes it only when it was accepting, as before.

Preserved: every result code, reason and log text, the DPI receipt text, prepare's two CPU
preparations (upstream host, then Runtime services), services published before the Runtime
lookup, the start reason set before re-admission, the rollback order of a refused start stage,
the racing-checkpoint close, stop's retryable order and status's compatibility override.

Deliberate differences, all on exception paths the former code left partial:

- A start stage that throws is retired together with every earlier stage (in reverse order) and
  the observers, then start reports `NOT_READY`. Before, the catch left them enabled.
- Each rollback, quiesce and withdraw step runs even when an earlier one throws. A throwing
  rollback still reports `NOT_READY` without the refusal reason, as before. Quiesce now reports
  `NOT_READY` after a throw instead of terminating through `noexcept`.
- A throwing Runtime activation in start now suspends the host and quiesces the started
  subsystems; stop retires the hooks. Before, they stayed live without accepting.
- `rsf_skyrim_publish_api` reports `NOT_READY` for a malformed leaf table, which the compiled
  constant never is.

Checks: source comparison against the removed C++, `rustfmt --edition 2024 --check` with rustfmt
1.11.0, `cargo metadata --locked --offline` and `git diff --check`. Two Rust regressions in
`session/tests.rs` cover the refused and the throwing start stage; they remain unrun.
