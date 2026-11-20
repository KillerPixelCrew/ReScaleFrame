# Anomaly SR stage driver in Rust

The port starts from `af65c49`. `sr_stage.cpp` drove all five SR stages: it
admitted views at H1, prepared each frame, ordered the S-B seam, restore and
reinsertion, published jitter and the main-view camera, and kept capture and
burst bookkeeping, options and status. That driver now lives in
`games/anomaly/src/sr_stage.rs`. `sr_stage.cpp` keeps the native leaves behind
the private table in `sr_stage_platform.h`. The `sr_stage.h` API, the three DLL
exports, the profile guards, the hook sites and the shader pack are unchanged.
[SR seam research](anomaly-sr-seam.md), [Stage 5 driver](rust-anomaly-sr-temporal.md),
[C++ baseline](rust-anomaly-cpp-baseline.md)

This change reuses the recorded seam, profile and producer evidence for the
Standard and AVX Anomaly 1.5.3 executables. It changes no shader, buffer,
producer, render-target layout, resolution or jitter formula. The shader and
buffer coverage audit in the seam research therefore still applies.

## Ownership

| Concern | Rust (`sr_stage.rs`) | Native leaf (`sr_stage.cpp`) |
| --- | --- | --- |
| Lifecycle | Configure refusal while installed, install order and refusals, session identity, quiesce order, active flag | Guard byte checks, MinHook trampolines, the position vtable slot, the rmNear slot, engine call pointers |
| Source identity and view admission | Which fact groups a stage needs, the SRScale re-read on a reset generation, the SRScale and backend plan choice, mip-bias inputs | SEH-guarded reads of `CRender::Target`, `HW.pDevice`, the observation reset count, device size, the render view and the MSAA flag |
| H1 and seam order | close_stale, GPU timer, facts, camera, jitter, prepare, publication, world count, capture and burst arm; then the seam, restore and seam pass | `SwapController` and its `SwapHost`, which still sequence the field exchange and restore |
| Stages 1 to 5 at the seam | Capture images, the burst copy, Stage 4/5 refusal order, motion admission, the evaluate or bilinear decision, the pass facts and reinsertion refusal | The round trip, bilinear, prepare and reinsert passes, temporal targets, the motion SRV and the `TemporalDriver` facade over the Rust temporal driver |
| Camera and jitter | `Device.mView`/`mProject` finiteness, the cut test input and frame time, the published `JitterState` and the lock-free UV for the motion stage | The camera copy, `JitterSequence`, `next_state`, `jitter_uv`, `camera_cut` and `BurstGate` from `sr_jitter.h`, `sr_pass.h` and `sr_plan.h` |
| Frame and capture bookkeeping | Frame counts, frames without H1, capture schedule and record, burst frames and JSON | Runtime dump export lookup, staging copies and raw writes |
| Options and status | INI parsing, the six live options and their saved values, the overlay choices, report text and deltas | INI and file-system calls, adapter option registration and persistence, the Runtime colour-transport setter |

The leaf objects run only inside Rust calls that hold the stage lock, as they
did under the old `guard`. The controller's seam pass needs that same state
while the lock is held. The seam call lends a scope to `controller.seam()`,
and `Host::upscale` and `Host::round_trip` return it to Rust. The Rust state
is not borrowed while the leaf runs. Burst staging textures are owned through
the existing `Lease`. The math headers stay native because `sr_swap` and the
motion stage share them.

## Behaviour checks

The INI reader now parses in Rust from the leaf's `GetPrivateProfile*`
results. A prefix parser follows the CRT `wcstod` rules for decimal,
hexadecimal, infinity and NaN text, so the existing SRScale, SRMipBias and
cut rules accept and reject the same strings. The hook contract's INI cases
pass unchanged, and a Rust regression covers hexadecimal and trailing text.

The report now emits its lines after releasing the stage lock, as before.
Its reported totals are now updated under that lock, which removes a race
with the next report. Counter deltas wrap like the previous unsigned
arithmetic, including after quiesce resets the GPU timer.

The overlay's per-Present report and settings request read the configured
stage from an atomic, as the C++ driver read its options without the lock.

The leaf refuses a seam pass that has no active seam and a motion view
without a device. The previous code could not reach these states, so no
running behaviour changes. Configure stores the native path copy after
Rust accepts it, during adapter startup before any hook can run.

No defect in the previous driver was verified during this port. Reviewed
paths included refusal accounting, motion admission by engine frame and
render size, the burst drain countdown, capture abandonment at quiesce and
teardown order.

## Intersection with the SR swap port

The parallel swap port at `d2c1bdb` replaces `FrameFacts`, `SwapCounters`
and the twin records with C records from `sr_swap_platform.h`. It adds the
`sr_jitter.h` and `sr_plan.h` includes to `sr_stage.cpp`. This port adds
the same includes and fills `FrameFacts` and copies `SwapCounters` field
by field with `std::begin`/`std::end`. That code compiles against either
header. The merge needs this `sr_stage.cpp` as a whole and changes no
`sr_swap` file. The stage driver's counter record mirrors the swap
counters field by field and does not depend on their layout.

A scratch worktree merged `af65c49` into `d2c1bdb`, resolving the
`lib.rs` and binding-generator conflicts as unions of both module lists,
and applied this port cleanly. MSVC 19.52 Release built the adapter there.
The Standard and AVX SR hooks, swap, sun-cascade, reconstruction-window and
lifecycle contracts passed, as did 30 `rsf-anomaly` Rust tests and Clippy.

## Verification

MSVC 19.52 x64 (Visual Studio 2026 generator, worktree-local build and
FetchContent directories, the coordinator's MinHook checkout at the pinned
revision) built the adapter in Debug and Release. Each configuration passed
24 focused CPU contracts: Standard and AVX SR hooks, geometry hooks and
input hooks; the SR swap, sun-cascade and reconstruction-window contracts;
jitter, plan, pass, profile and reconstruction; the real Runtime SR API;
the plugin, lifecycle, bootstrap, native hook, overlay API and motion hook
contracts. The SR round-trip, temporal, jitter and FG device fixtures built
and were not run.

Eleven new Rust tests drive the driver through a fake leaf table. They
cover INI parsing, quiesce and install order, a different build after
quiesce, scope and continuity of the jitter sequence, live jitter off,
every Stage 4/5 refusal path, the drained boundary, live option saving,
overlay choices and report deltas. Formatting, workspace Clippy with
warnings denied, and all 235 locked workspace Rust tests passed. The
generator reproduced sr_stage_abi.rs byte for byte.

Both DLLs export sf_anomaly_observation at 1, sf_anomaly_start at 2
and sf_get_game_plugin_api at 3, like the staged 8322179a36c7 DLL.
Neither imports ReScaleFrame.Runtime.dll; both carry its UTF-16 module
name and the by-name lookups for the SR API, colour transport and frame
dump exports. The CRT conversion import is gone because wcstod moved to
Rust. The built motion.rsfpatch matches the staged pack hash
D2AA077A…F8622BB2.

An earlier configure with the Visual Studio 2022 generator (MSVC 19.44)
failed to compile the unchanged nomaly_sr_plan.cpp and
nomaly_sr_pass.cpp tests with warning C4127 under /WX. Those files
are untouched since the initial commit, and the 19.52 build passes them.

## Open gaps

No device, visible window or game ran. The Rust driver's live behaviour on
Standard and AVX, live option changes, the Stage 3 burst files, the Stage 5
capture guides and quiesce with an in-flight frame still need game runs.
The prior game baseline applies to the C++ driver.
