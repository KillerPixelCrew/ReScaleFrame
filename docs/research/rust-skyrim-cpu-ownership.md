# Skyrim CPU source and render ownership in Rust

This source port starts at `e17d1df` on `rust/orchestrator-anomaly`. It asks which parts of
Skyrim's CPU and input admission can move to Rust while keeping the observed engine callbacks
as the producers. The existing Steam 1.7.104 evidence applies to the same sites and layouts.
Evidence for this port comes from source and the prior engine records.
[Native input ownership](skyrim-input-ownership-20261115.md) ·
[Prospective source](skyrim-prospective-source-20261115.md) ·
[CPU review fixes](skyrim-cpu-review-fixes-20261115.md)

## First slice

- `cpu_sources.rs` owns the pending and adopted source slots and the bind refusal decision.
  The slots remain independent because the next late poll can overlap the current owning tick.
  Adoption still requires a produced sample, the same session and a retired native job
  generation. Binding still requires the same completed poll generation and refuses unreadable
  state, queued move, an earlier observed consumer or free camera in that order. The mutex covers
  only scalar transitions. Each call returns before native work or a host callback can run.
- `cpu_policy.rs` allocates the shared source IDs and the RenderEntry scope IDs. The C++ Main
  detour lends a stack record on its original thread. Rust tracks first simulation end, first
  unreserved tick sample, World entries, returned World calls and render identity. Leaving a nested
  RenderEntry restores the enclosing scope ID. The World entry and completion counts still cover
  the owning tick.
- RenderEntry `0x6565b0` starts with a screen query without primary-World evidence. The World
  hook `0x656e60` contributes only for caller return `0x656e32`, output `0`, and an existing scoped
  source with a camera key and swap chain. Rust captures the same native frame, camera, window and
  swap-chain slots, uses the same before/after screen snapshots, and checks both guarded UI query
  entry points in prepare and again before enable. A second primary World refuses gameplay. A
  render identity gets FG eligibility only in an active scope with exactly one primary gameplay
  World. This gate is independent of the prospective input binding.
- Delivery still comes from dispatch `0xcfa650` for the native input-manager singleton. Rust
  reads the same 24-byte headers through the guarded native copy, reports only keyboard, mouse
  and controller kinds, and refuses the whole list on an unreadable node or a non-terminating
  128-node scan. The version 1 unframed event keeps a zero source ID and is delivered after the
  original dispatch with its pre-dispatch QPC timestamp.
- The Rust event builders use the existing GameSDK phase and source layouts. QPC and host calls
  stay in the C++ detours. `FRAME_BEGIN` and `FRAME_END` surround the original Main tick
  `0x658870`. After any bind or abort, `SIMULATION_BEGIN` follows the verified submit branch at
  return `0x658b14`, before the native wake. `SIMULATION_END` comes from the first actual
  RenderEntry. A skipped render is flagged on `FRAME_END`. Version 2 `RESERVE` precedes the
  original poll, `poll_begin_qpc` is read after the synchronous host callback and immediately
  before `PollDevices`, and `SAMPLED` follows the original and precedes local eligibility for
  adoption. This Skyrim path uses versions 1 and 2. Runtime's version 3 receipts have separate producers.

The inputs on the process hook still reach the same consumers. `observe_process` requires the
confirmed post-process caller and targets `1 -> 0`, publishes the pre-mutation FG inputs, then
continues through SR and the original processor. The FG input publication rechecks native frame
plus source, submission and scope. The UI handoff matches the scene's session, native frame,
swap chain and live render identity before publishing `FINAL_SCENE` and `WINDOW`. It propagates
the live screen and FG refusal. The existing diagnostic lines still report the live render and
reservation facts at those callbacks. The adapter reports them to Runtime, which owns pacing,
latency, vendor selection and physical Present dependency handling.

## Native boundary after the first slice

The 13 MinHook targets and typed originals, named callback pointer guard at `0x20b5130`,
native submit/join and PollDevices provenance, completed poll/writer atomics, control sink,
movement/camera envelopes, and teardown ordering stay in `cpu_events.cpp` and
`input_ownership.h`. The callback identities remain `0x658b14` for submit, `0x659334` for
join, and `0xebc248` for the scheduler's named `0x6e6030` poll callback. The native poll keeps
the `float` timestep and pointer-width result. The CPU observer disables and drains before
releasing services. Further ports need to carry those producer and hook lifetimes together.
The existing diagnostic envelopes and prior game evidence still bound the understood movement
and camera coverage.

The screen snapshots and query guards now use the adapter platform's guarded copy. An unreadable
query guard refuses preparation or start. A C++ exception from a typed menu query gives an
unknown screen, and an unreadable render slot contributes zero. These are the intended behavior
changes. The first prepare explicitly lends the platform table because it runs before session
registration. The C++ fixture keeps its own reservation store and links the same Rust policy as
the DLL. The production store has static storage. The public DLL entry, GameSDK records and
`skyrim_input_contract` target retain their contracts.

Verification is source-only. The original hook bytes, caller gates, timestamps and callback
ordering were compared against `e17d1df`. The existing fixture now also describes RenderEntry
scope nesting, World admission and duplicate refusal, active gating, skipped render, and sample
timestamps. Pinned Rust formatting, locked Cargo metadata and Git whitespace checks passed.
Build, link, test, Clippy, device and game validation, deployment and push remain with the user.

## Native job and poll proof continuation

This continuation starts at `9260a14` on `rust/orchestrator-anomaly`. It compares the source
port against that revision and uses the prior Steam 1.7.104.0 executable evidence (SHA256
`846efccf0c1374d71f892907f46549560f2fcb0a75cb87a3eed438baa0f1402f`). The question was
which parts of job, poll and input consumption proof could move together while preserving the
engine thread, callback arguments and ordering that give the proof meaning.
[Native input ownership](skyrim-input-ownership-20261115.md) ·
[Prospective source](skyrim-prospective-source-20261115.md)

- Main's submit `0x60eaf0` publishes a Rust ticket after the normal-simulation gate at caller
  `0x658b14`, after any BIND or ABORT and SIMULATION_BEGIN, and before the original can wake
  workers. Rust preserves the sequentially consistent publication, double-read snapshot and
  poison transitions. An overlapping submit poisons proof and recovery requires the native idle
  fields. The join at `0x60eb80` still validates caller `0x659334`, the same Main and manager,
  the wait result and idle post-state in C++ before Rust records the retired generation.
- The named scheduler callback at caller `0xebc248` keeps a copy of the ticket in native
  thread-local storage only around its original. PollDevices `0xcf95f0` lends its own stack
  record through native TLS. Rust checks exclusive poll and job ownership before the guarded
  native manager read, then rechecks the job and poll. C++ still checks active admission
  before and after. The controls callback `0x7ad900` opens a Rust write epoch around its original.
  A producer receipt requires the same dispatched list and native PlayerControls singleton.
  The unchanged completion stamp requires that poll's exclusive generation and no intervening
  writer or invalidation. Nested polls restore the enclosing TLS and are refused as producers.
- Pending and adopted sources still have separate slots. Rust checks the completed poll and
  matching pending source before the native detour emits SAMPLED, and the detour publishes the
  transition to sampled only after that host call returns. RESERVE, the QPC read immediately
  before PollDevices, SAMPLED, targeted ABORT, tick adoption on a normal join and binding with the
  matching completed generation retain their order. The same native guarded queued-move and
  free-camera observations can refuse BIND. Unreserved tick input and unframed dispatch still
  come from their original sites. Runtime owns pacing, latency and markers.
- Rust stores the main-thread player, movement and camera observations and rejects an ambiguous
  or changed stamp. C++ still takes the stamp before each original and checks the same actor
  and camera evidence around it. Off-thread counts use the bound source and its published job,
  and stay diagnostic. The control and camera envelopes cover the same observed callbacks.
- C++ deactivates the observer before Rust poisons jobs, invalidates input, clears both source
  slots and releases the worker diagnostic source. Startup checks hook state and entry bytes
  before resetting Rust proof and enabling hooks. C++ retains the MinHook transaction and drains
  entered detours before releasing services. The fixture keeps private stores and calls the same
  Rust operations through its existing C++ interface.

Verification is source-only. Callback guards, native forwards, GameSDK receipts, clocks, and
teardown were compared with `9260a14`. Rust 1.94.1 formatting, locked offline Windows x64
metadata, and Git whitespace checks passed. The remaining native layer contains the engine
callbacks, guarded reads, native admission, QPC and host delivery, and hook effects. The existing
proof coverage boundaries still apply. Build, link, tests, Clippy, deployment and device/game
validation remain with the user.
