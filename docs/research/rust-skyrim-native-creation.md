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
   `plugin.cpp` keeps the five typed observer detours and their native diagnostics.
   Rust owns the presentation binding read, window-class rule, reconstruction
   admission and scoped TAA override.
2. `compatibility.rs` and `compatibility/initializer.rs` own the Community Shaders
   1.9.1 profile, cold-device and feature checks, resolver capture, initializer
   transaction and HDR caller scope. The typed native detours stay in `compatibility.cpp`.
3. `cpu_events.cpp` with `input_ownership.h` and `screen_classification.h`: CPU frame, input
   ownership, prospective source and screen classification (one hook, about forty RVAs). Its
   fixture `tests/skyrim_input.cpp` compiles the source directly.
4. Rust owns pre-HDR texture discovery and admission, SR and FG delivery, the
   sampler hook at `+1010de0`, interface hook at `+116ac80`, resolution planning
   and auxiliary reconstruction and diagnostics. The native files lend typed
   engine, COM, and Runtime calls.
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
Source comparison matched both baseline blocks, including the cache
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
uses the identity matched at entry. The pre-HDR texture producer checks
its descriptors and hands off through the same GameSDK calls.
Source review matched the hook bytes, copied fields, pre-HDR producer
block and capture names. Builds and live callback ordering are unverified.

## Auxiliary surfaces

`auxiliary.rs` selects and leases the four existing auxiliary surfaces,
builds the same Runtime request and records an accepted scope. The surface
and consumer evidence is in [the integration report](skyrim-se-integration.md#reduced-sr-auxiliary-handoff-and-zoom-correction-15-november-2026).
The underwater mask retains the native clear. The restore ticket requires
the opener's session, host and renderer thread and stays pending after a
Runtime refusal. Opening and closing hold admission across the native
calls, with a matching reentrant finish deferred until opening returns.
Output resolution is restored only for a successful entry. Guarded reads
now cover the diagnostic selector and target records. The selected DOF
alias remains descriptor-only, because capture reads subresource zero.
Source and layout comparisons passed. Builds and GPU validation remain open.

## Resolution and viewport state

`resolution.rs` owns the native ratio and lock snapshots, synchronized
extent publication, Halton sample and temporary output viewport receipt.
It keeps the original reconfigure pass and callback order, prepares
jitter in the same two native fields, and preserves the renderer-thread
upload flags. The production evidence and offsets remain in
[the integration report](skyrim-se-integration.md#reduced-sr-auxiliary-handoff-and-zoom-correction-15-november-2026).
Native callbacks apply the exact field writes and upload constants before
calling the viewport routine. The guarded history read refuses an invalid
pointer. If the renderer cannot restore producer ratios immediately, the
matching auxiliary receipt records whether Runtime restoration already
completed, so a retry only performs the remaining output restore. Source
and layout checks passed. Builds and live quality switches remain open.

## Pre-HDR texture producers

`render_inputs.rs` leases the context, shared textures, optional SR mask
and swapchain while the native leaves query D3D11 descriptors and deliver
the pass. The same device, full allocation and single-subresource checks
apply to both paths. SR accepts its original two colour formats and
twelve mask formats. FG keeps its narrower colour check, exact native
frame and source/submission/scope recheck, before the engine mutates
the inputs. Null devices now refuse, and guarded reads cover the
viewport diagnostic. Source comparison matched the existing pass fields,
mask formats and native addresses. Build and live replay remain open.

## Community Shaders profile and HDR caller

Rust checks the release SHA-256, PE machine, timestamp, image size and
six executable section probes, followed by their corresponding loaded
bytes when a module is present. The native read-only mapping and share
mode span the hash and copy as before. The parser now requires a complete
section table and checks address arithmetic. Feature and device checks use
the same guard reads. The typed HDR detour still captures the native return
address and calls the original with all arguments intact. A Rust thread-local
scope preserves the caller and five arguments through nested callbacks.
The initializer state and path rules are Rust-owned. MinHook and file effects
use typed native calls.
[Guard and initialization evidence](skyrim-cs-handover-20261115.md)

The source comparison matched the fingerprint, six byte sequences, all
HDR call bytes and offsets, file effects and private layouts. Builds and
live handover verification remain open.

## Community Shaders initializer transaction

The Rust owner serializes path and profile admission, publishes the
original initializer for the exact `SKSEPlugin_Load` lookup, and keeps
the loader-callout refusal ahead of file inspection or hook changes.
The resolver restores the last error from the original. No file I/O,
original routine, hook operation or withdrawal callback runs under the
Rust state lock. Rollback disables any created feature hooks and keeps
their pinned trampolines valid. A refused feature hook that remains
reachable forwards with no feature exclusion. Concurrent arming refuses,
and an exception from the native profile leaf is now an unreadable
profile result. The original initializer can still fall back before
device creation. Source and layout checks passed. Builds and live
loader and rollback verification remain open.

## Pre-HDR post-process observer

The native detour captures the same return address and calls the original
on its own frame with all five arguments. `postprocess.rs` checks the live
session, compatibility gate, exact input and output and caller scope. It
reports the native FG inputs before SR mutates them. A successful SR call
requests a clear of main TAA history using the observed mask and lends one
temporary enable-byte receipt to the detour. The native scope restores it
only while the same TAA owner remains readable, and the session's in-flight
count covers restoration. The write leaf preserves Win32 last error.
Source and layout comparisons passed. Build and live fallback remain open.

## Renderer-end and upload observers

Rust borrows retained session services at renderer-end, including during
quiesce. When live and compatible it binds the observed swapchain, then
restores sampler state before the native renderer-end call. The window
and auxiliary scopes retire afterward. The upload detour checks its
renderer and dirty byte before calling the original, and publishes that
callback's flags afterward. Jitter still runs the original before checking
resolution ownership and the bounded diagnostics. The exact window-class
predicate now lives with these engine facts. The originals remain typed
C++ calls. Source comparison and layout checks passed, and build and live
renderer verification remain open.

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

## Startup and CPU follow-up

S01 publishes the checked platform before compatibility and observer admission. Native creation
calls the I01 Steam ABI directly after MinHook and the cold renderer-owner guard. Community Shaders
uses the same guarded owner observation. Repeated lookups of the captured initializer keep the
wrapper, and only its first invocation can claim the handoff. Rust now owns the Win32 module
lookup and pin, fingerprint, compatibility file mapping and startup observations. The
compatibility probe holds a read-only file handle across fingerprinting and mapping to preserve the
existing file sharing contract.

CPU preparation checks the native sites and captures the clock once per prepare. A separate bind
publishes the final Runtime services. Start still rechecks the live hooks before enabling them.
Poll completion and conflict checks share a publication lock. A nested RenderEntry restores its
parent scope and classification from a receipt and retains frame-wide counters. The native detours
keep return-address capture, engine original calls and guarded native exception boundaries.

Runtime activation errors propagate through startup and control calls. Failed rollback records
which stages still owe cleanup. Quiesce can retry that cleanup before the Runtime moves to stop.
Start refuses a session with withdrawn admission. The S02 auxiliary and resolution retirement
checks and session render leases remain part of the lifecycle.

The public D3D11 setup and creation records now share generated SDK bindings across Skyrim,
Fallout 4 and the Runtime. The portable CPU policy assertions moved to Rust. The
`skyrim_input_contract` CTest executable still exercises real guarded input-list reads and the
native RenderEntry ABI. Verification covered source checks, Rust 1.94.1 formatting, locked offline
metadata and focused bindgen 0.72.1 regeneration. Builds and fixtures remain unrun.

## Follow-up from 82b39a63

The startup and input changes were reapplied on `82b39a63`. The shared D3D11 setup binding
includes that base's `depth_far_plane` extension. Skyrim retains finite depth and Fallout 4
retains its own depth convention. The platform is published before both cold render guards.
S02's auxiliary retirement, session admission and retained-lease paths are preserved. Cold
creation calls the existing I01 Steam C ABI.

Rust now invokes MinHook for creation, compatibility, CPU production and observers. CPU
trampolines are published with release ordering before enabling and acquired by the native
detours. Device creation retains idempotent enable status, and observers and CPU hooks keep raw
enable results for diagnostics. The session calls Rust render and CPU services directly and keeps
separate results for Runtime refusal and foreign exceptions.

The remaining native boundaries are:

- `native_creation.cpp` captures its own engine caller address for the verified game and
  Display Tweaks routes. Its typed D3D11 forwarder retains the native HRESULT, arguments and
  stack unwind, and its destructor releases the Runtime scope and restores last error.
- `compatibility.cpp` forwards the original GetProcAddress, initializer, configuration and HDR
  calls. The HDR hook captures the renderer caller's return address and restores the scoped
  observation on unwind. The initializer catches exceptions before returning into Rust.
- `cpu_events.cpp` keeps the typed original calls, detour-local return addresses for World and
  native job provenance, synchronous callback exception guards and the remaining TLS event
  sequencing. The proof store, checked engine input reads and MinHook transaction are Rust.
- `plugin.cpp` publishes the native GameSDK entry and retains the typed observer forwarders,
  the original post-process call frame and callback exception guards around the host and Runtime.
- `guarded_read.cpp` is the common fault boundary used by native creation and CPU input. On
  MSVC the actual copy is inside `__try`/`__except`. The existing MinGW route checks the
  committed readable region with VirtualQuery before copying.

The seven assigned headers now contain declarations, private wire records and layout checks.
`tests/skyrim_input.cpp` was replaced by a Rust fixture body and C entry point with the
same CTest name. It uses the production guarded-copy callback for the valid, cyclic, bounded
and unreadable pointer cases. Render scope and event receipts are checked in Rust.

## Native source follow-up from 76720485

The CPU receipts and thread-local frame, poll, named-job and dispatch scopes now live in Rust.
The 13 native detours keep their typed continuations and forward the original arguments and
results. The four caller addresses for World, submit, join and named poll still come from their
own native detours. Submit, join and named poll retain the matched return RVAs `0x658b14`,
`0x659334` and `0xebc248`. The World match remains in the Rust proof. A poll timestamp is
sampled after the synchronous reservation callback and before native PollDevices.

Each detour lends aligned stack storage for one Rust receipt. The generated x64 wire record is
344 bytes with 8-byte alignment and storage at byte 24. Rust checks that its private receipt fits.
Native scope destruction restores thread-local observations and releases the entry lease on
return and C++ unwind. Unwound control writes invalidate the input proof, and an unwound poll
cannot publish a completed sample. The original native continuations, menu queries and foreign
host calls keep their native exception boundaries.

Observer admission now publishes its verified targets and original addresses in Rust. Enabled
detours acquire the published trampoline and keep their original native signatures. The
Community Shaders resolver still calls the native GetProcAddress trampoline and preserves its
last error. Rust owns the read-only profile probe, repeated initializer lookup and phase proofs.
Native initializer exception containment, configuration exclusion on unwind, and the HDR
caller/restore scope remain. Cold D3D11 creation still enters the direct I01 Steam C ABI. The
S02 session retirement implementation and guarded-copy source match the base byte-for-byte.

Source checks covered the native and Rust hook order, the four original caller captures, the
unchanged Community Shaders loader and phase predicates, the direct Steam call and the
unchanged S02 retirement files. Bindgen 0.72.1 reproduced the private CPU record for
`x86_64-pc-windows-msvc`, and the pinned Rust 1.94.1 formatter and locked offline Cargo metadata
passed. MSVC `/Zs /W4 /WX` parsed all five Skyrim C++ source files and their layout assertions
without producing objects. No Rust compilation, links, fixtures, device checks or game runs
were performed.

## S01 and S02 source replay from e5d73636

Replay the CPU sequencing and observer trampoline port (`45145e79`) first. The follow-up moves
production session, observer and creation table construction to Rust. The GameSDK DLL export
now forwards from a C11 shim. Render observer activity and the render-config and upscale result
handling are Rust-owned. The renderer-end, upload and pre-HDR calls retain their native order.
Runtime host creation and activation still distinguish a thrown callback from its returned status.
The native menu call now returns only three booleans. The screen classifier and live before/after
owner checks are Rust-owned, and the former screen-classification header is retired.

The remaining native source files have these responsibilities:

| File | Native responsibility |
| --- | --- |
| `plugin.c` | C11 GameSDK DLL export forwarding to Rust. |
| `native_creation.cpp` | Typed D3D11 continuation captures the caller and preserves the original HRESULT, output pointers, native last error and scope end across unwind. Guarded calls contain Runtime begin/end exceptions. |
| `compatibility.cpp` | Native GetProcAddress continuation and last error, initializer exception result, configuration exclusion cleanup, and typed HDR original call with its caller address and scoped restore. |
| `cpu_events.cpp` | Typed engine continuations, four detour-local caller addresses, Rust receipt destruction on unwind, and guarded native menu and CPU-event calls. |
| `render_detours.cpp` | Five typed observer continuations, the actual pre-HDR caller address and TAA restore receipt, and guarded host/render callbacks. Rust supplies the trampolines and owns activity and retirement decisions. |
| `guarded_read.cpp` | Guarded native copy with MSVC SEH and the existing MinGW region check. The Skyrim input fixture still uses this same source. |

The source audit compared the 13 CPU entries and exact admission predicates, the Community
Shaders loader/phase predicates, the screen classifications and the S02 quiesce, rollback and
stop bodies with `e5d73636`. The creation owner and its direct I01 Steam call match that base.
The Windows x64 CPU call stays 344 bytes, aligned to 8 with receipt storage at byte 24. Its
new menu record is three bytes. Bindgen 0.72.1 recreated the checked-in bindings, and Rust
1.94.1 formatting and locked offline Cargo metadata passed. MSVC `/Zs /W4 /WX` parsed the C11
entry and all five C++ leaves. No builds, links, fixtures, lint, devices or game runs were used.

## C11 guarded copy and continuation registration from 6c935471

First replay `eaa34a66`, which changes the guarded read to C11. MSVC still performs the copy
inside `__try` and returns false from `__except`. MinGW still queries the committed region,
rejects inaccessible and guarded pages, checks that the copy fits, and then calls `memcpy`.
The adapter and the existing input fixture use the same C source. The fixture keeps its
original C++ link driver and its existing C11 entry.

Then replay the continuation registration change. Rust binds the 13 typed CPU detour addresses
to the checked hook order and the five renderer observer addresses to shared generated indices.
CPU prepare now pins its detour's DLL by the first typed address, which belongs to the same
module as the former C++ table. Trampolines still publish before enable. Rust checks optional
logging, CPU event and render-pass callbacks. Their native call leaves still contain C++
exceptions, and the host callback results retain their previous meaning.

The remaining executable native owners are `plugin.c` for the GameSDK DLL export and
`guarded_read.c` for the fault boundary. The four C++ files retain typed engine or Runtime calls:
`native_creation.cpp` holds the D3D11 caller, HRESULT and unwind scope, `compatibility.cpp`
holds the resolver, initializer, configuration and HDR calls, `cpu_events.cpp` holds the
engine calls, callback receipts and menu-query guard, and `render_detours.cpp` holds the
five renderer continuations, TAA restore receipt and foreign callback guards.

Source comparison matched all 18 typed forwarder bodies after normalizing their symbol names
and observer indices. The CPU admission predicates, caller captures, direct I01 Steam path,
Community Shaders sources and S02 retirement bodies matched the base. Rust 1.94.1 format checks,
bindgen 0.72.1 regeneration for Windows x64, locked offline metadata and diff checks passed.
MSVC `/Zs /W4 /WX` and MinGW `-fsyntax-only -Wall -Wextra -Werror` parsed both C11 sources
and all four C++ sources. No builds, links, fixtures, lint or devices were run.

## Native leaves moved to C11 (S01)

`native_creation.cpp`, `compatibility.cpp` and `cpu_events.cpp` are now `native_creation.c`,
`compatibility.c` and `cpu_events.c`. `plugin.cpp` had already become `plugin.c`, so the S01
checklist entry only needed closing. The bodies are unchanged: the same typed continuations,
caller captures, last-error handling and layout asserts (now `_Static_assert`). C has no
destructors, so each scope that relied on one now uses MSVC `__try/__finally`. This keeps
Leave, the Rust CPU receipt, the configuration exclusion byte and the HDR restore running when
an engine continuation raises. The foreign-exception guards (`catch (...)`) became
`__try/__except (EXCEPTION_EXECUTE_HANDLER)`, which also contains the foreign C++ exceptions
and a fault in the called function. Non-MSVC builds fall back to straight-line calls with no
unwinding. `cpu_call.h` gained C typedefs for its tagged types and stays usable from
`render_detours.cpp`.

`render_detours.cpp` is now `render_detours.c` (S02). Its guards filter on the MSVC C++
exception code 0xE06D7363 so access violations propagate, and the activity lease and TAA
restore receipt use `__try/__finally`. See
[port-introduced C++ closure](rust-port-introduced-cpp-closure.md).
