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
| Frame and capture bookkeeping | Frame counts, frames without H1, capture schedule and record, burst frames and JSON, burst staging admission, descriptor, subresource choice and map admission | Runtime dump export lookup, the typed context calls of the burst readback (below) |
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

Both DLLs export `rsf_anomaly_observation` at 1, `rsf_anomaly_start` at 2
and `rsf_get_game_plugin_api` at 3, like the staged `8322179a36c7` DLL.
Neither imports ReScaleFrame.Runtime.dll; both carry its UTF-16 module
name and the by-name lookups for the SR API, colour transport and frame
dump exports. The CRT conversion import is gone because wcstod moved to
Rust. The built motion.rsfpatch matches the staged pack hash
D2AA077A…F8622BB2.

An earlier configure with the Visual Studio 2022 generator (MSVC 19.44)
failed to compile `anomaly_sr_plan.cpp` and `anomaly_sr_pass.cpp` with
warning C4127 under `/WX`. The merge reproduced both warnings and moved
the two constant checks to `static_assert`. MSVC 19.44 compiled and ran
both fixtures with warnings denied. The MSVC 19.52 Debug and Release
fixtures also passed after that change.

## Open gaps

No device, visible window or game ran. The Rust driver's live behaviour on
Standard and AVX, live option changes, the Stage 3 burst files, the Stage 5
capture guides and quiesce with an in-flight frame still need game runs.
The prior game baseline applies to the C++ driver.

## Stage 3 burst readback staging

The port starts from `8f5d0b49`. The native `staging_copy` and `map_staging`
leaves ran the whole readback transaction. `sr_stage/staging.rs` now owns it.
Rust checks for a null or deferred context and a null source. It queries the
source as a 2D texture and refuses a multisampled one. It writes the staging
descriptor and compares the source's device with the context's. It then creates
the copy and names subresource 0 for the copy, the map and the unmap. The map
side checks for a known pixel size, an immediate context and the same device. It
reports the size and format even when it refuses. The D3D11 facts come from the
existing `rsf_sr_d3d11` table: `as_texture`, `describe_texture`, `texture_device`,
`texture` and `release`. Every returned reference is a `Lease`. The stage table
keeps the typed context calls as leaves: `immediate_context` (GetType),
`context_device` (GetDevice), `copy_subresource`, `map_read`, `unmap` and
`pixel_bytes`. `copy_subresource` saves the predicate, clears it, runs
`CopySubresourceRegion` and restores it, all in native code.

No public ABI changes. The `sr_stage.h` API, the DLL exports, `game_api.h` and
the other capture paths (`take`, the frame dump and F10) are unchanged. The
private table grows from 600 to 624 bytes. The old staging slot becomes
`immediate_context`, `context_device` and `copy_subresource`, and the map slot
becomes `map_read`. `unmap` gains a subresource argument and `pixel_bytes` is
new. Every later slot moves by 24 bytes. `rsf_anomaly_sr_mapped_texture` is gone.
`sr_stage.cpp` asserts the new size, the first and last new offsets, the
`bilinear` offset and the three D3D11 constants. The generator's allowlist
dropped `mapped_texture`. The merge reproduced the edited
`sr_stage_abi.rs` byte for byte with bindgen 0.72.1.

Source comparison against `8f5d0b49`:

| Path | Before (native) | After |
| --- | --- | --- |
| Copy order | GetType, QueryInterface, GetDesc, sample check, context GetDevice, texture GetDevice and Release, CreateTexture2D, GetPredication, SetPredication(null), CopySubresourceRegion, SetPredication(restore), Release of predicate, device and source | The same calls in the same order. The QueryInterface, GetDesc, texture GetDevice and CreateTexture2D go through the `rsf_sr_d3d11` table; the device lease drops before the source lease |
| Descriptor | Source width, height, format and `SampleDesc`; MipLevels and ArraySize 1, `USAGE_STAGING`, BindFlags 0, `CPU_ACCESS_READ`, MiscFlags 0, no initial data | Identical, written by `readback()` over the bit-cast `D3D11_TEXTURE2D_DESC`; `CreateTexture2D` gets no initial data |
| Copy region | `(copy, 0, 0, 0, 0, source, 0, null)` | `(copy, SUBRESOURCE, 0, 0, 0, source, SUBRESOURCE, null)`, with `SUBRESOURCE = 0` |
| Copy refusals | Null context or source, deferred context, failed QueryInterface, multisampled, null or foreign device, failed create; each returns null before any copy | The same set, each returning `None` before any copy |
| Map order | GetDesc, pixel size, context GetDevice; then pixel size, GetType, texture GetDevice, each short-circuiting; then Map(0, READ, 0); device Release after Map | The same order. Refusal keeps width, height and format, with a null data pointer and pitch 0 |
| Cleanup | `Mapping` drop calls Unmap(0) after the rows are written or refused; refused or failed maps never unmap; `burst_write` then drops both frame copies | Unchanged. `Mapping` now borrows the held `GpuLease`, so the texture outlives its mapping |
| Held identity | World copy from the seam's `twin_surface` while the frame is armed and has no world image; presented copy from `back_buffer`, then the back buffer reference drops | Unchanged |
| Burst output | `world_NN.bin`, `final_NN.bin`, `burst.json` fields and result text | Unchanged; `burst_file::staging` returns the same tuple |

There are two intended differences. A copy is now released through the
`rsf_sr_d3d11` `release` leaf instead of the stage `release` leaf. Both call
`IUnknown::Release`. A missing table leaf now refuses the copy. Before, a
missing stage `release` leaf leaked the copy. Neither happens with the shipped
tables.

Four new Rust tests drive `copy` and `map` against the `sr_d3d11` fixture. They
check the descriptor, the stage leaf order, the subresource indices, every copy
and map refusal and reference balance. They have not been run.

Pinned Rust 1.94.1 formatting, locked offline Windows Cargo metadata and Git
whitespace checks passed. No compile, link, Clippy, test, device or game run
was performed.

Remaining verification, all user-owned:

- Build both Anomaly configurations with MSVC. The new `static_assert`s check
  the private table layout and the D3D11 constants. Run Clippy and
  `cargo test -p rsf-anomaly --locked`, including the four `sr_stage::staging`
  tests and the `sr_d3d11` and `sr_stage` suites.
- Play a Stage 3 burst (`SRStage=3` with the burst enabled) on Standard and
  AVX with the D3D11 debug layer on. Check that `burst.json` reports every
  world and final frame as written, with the render and output sizes and
  DXGI formats 10 and the back-buffer format. Compare the `world_NN.bin` and
  `final_NN.bin` bytes or the 0.05 px jitter estimate with a burst from the
  `8f5d0b49` build. Check that the debug layer reports no Map, Unmap,
  CopySubresourceRegion or predication errors, and no live staging textures
  after the drain.
- Interrupt a burst with a loading screen and quiesce while frames are held.
  Confirm that the copies are released and no map is left open.

## SR guard admission

The verified Standard and AVX profiles and image reader are shared with
the native fixture. Rust now selects the SR checks inside the existing
stage guard callback. The native leaf lends the matched profile and
guarded image. Stage 2 checks the SSAO viewport helper first. Stage 3
then checks the set_P unique guard, SetMipLODBias, all six first-person
call pairs and the reconstruction guards. The common path checks the
same six function sites, the blur and sun-cascade unique guards and
the call sites, and returns the first mismatch with the original name.
The geometry owner still supplies the shared rmNear slot.

Source comparison against `c509a09b` matched the sequence, refusal names,
16 fields of the private record and its x64 layout. The existing stage
ABI, profile tables and both `anomaly_sr_hooks` contract sources are
unchanged. Pinned Rust 1.94.1 formatting, locked offline Windows Cargo
metadata and Git whitespace checks passed. Compile, link, fixture and
game verification remain user-owned.
## Frame and loading admission

Compared with `c57905d1`, the Rust stage now admits the render-target
identity at the verified scene-prepare hook and stock capture seam.
It uses the original guarded `CRender::Target` read for each callback.
A mismatched target skips the seam read. A null target now also skips
the read before native code forms the texture address.

The native leaves borrow the same Standard and AVX profile addresses
and report the render view, options word and read results. Rust
applies the original scope and MSAA exclusions, including unreadable
values. The loading gate reads the registration byte before the
precache count. If the first read fails, it skips the second read
and holds capture. A nonzero value or failed second read also holds
capture. The original guard profiles and active producer and buffer
paths come from the recorded seam evidence.
[SR seam research](anomaly-sr-seam.md)

The frame-read record is 168 bytes and the private stage table has 80
slots. Focused bindgen regeneration and source checks covered both
profile reads, the admission order and public declarations. The
extended fixtures, current build, and Standard and AVX game checks
remain open.

## Seam draw bindings

Compared with `9a1a9516`, the swap controller now lends its existing
twin and engine records directly to the Rust SR stage. The native host
no longer publishes pointers to local seam or round-trip view records.
Rust assembles the existing 96-byte draw record for the synchronous
pass callback. The controller retains the resource leases through the
seam, and the native leaf still borrows the same pass and context.

| Pass | Binding comparison |
| --- | --- |
| Stage 1 round trip | Read twin generic0 at the twin extent and write the original generic0 RTV at its engine extent. |
| Spatial reconstruction | Use the twin colour, position, distortion and depth views and the three original colour targets and original depth view. |
| Temporal preparation | Keep the render extent, colour, depth and selected motion inputs, and the colour and motion output views. Keep the 0.02 depth threshold and the live first-person zero flag. |
| Temporal reinsertion | Use the spatial bindings with the reconstructed output SRV in the fifth input slot. |

The stage table now has 77 callbacks because the four draw entries
share one native leaf. The fixture inspects the borrowed bindings at
that leaf. Source comparison and focused binding regeneration passed.
The public declarations, shaders, capture order and profile data
match in source. Build, fixtures and live Standard and AVX checks
remain open.

## N06 source review and port checkpoint

Base `9bb9421d` includes the N07 restoration and retirement work. The stage
calls the same Rust swap owner after the same verified native guard. It uses
shared frame facts and counters. Twin retirement remains conditional on
restoration. The scoped seam forwards the spatial recovery flag to the
bilinear path and keeps the engine viewport handoff in the swap owner.
Paths in the findings below are relative to `games/anomaly/src/` unless
they specify another repository directory.

- **Report whether failed constant correction restored all engine state.** sr_reconstruction.rs, sr_reconstruction.h, sr_swap.rs and sr_swap_platform.h. Stock vector and dirty byte are read back and retried synchronously while the engine staging slot is valid. An incomplete rollback is reported distinctly and the current seam uses spatial recovery. The native reconstruction fixture covers one-shot failures and a persistent rollback refusal.
- **Resolve Anomaly's vendor runtime directories once for SR and FG.** sr_stage.rs and fg_stage/options.rs. One UTF-16 resolver uses the production INI lookup, explicit paths and the package defaults. FG preserves capture-path semantics and converts vendor paths to UTF-8 at the Runtime boundary.
- **Give the remaining Anomaly SR keys overlay rows.** sr_stage.rs and sr_stage/tests.rs. All seven rows are registered. Scale and cut thresholds affect subsequent frames. Schedule edits change the diagnostic budget, and Stage 3 schedule edits start a separate burst when the start or count changes. Signature enforcement is applied at pipeline startup and the row signals restart.
- **Make native guard availability part of SR stage admission.** sr_stage.rs, sr_stage.cpp and sr_stage/tests.rs. A fresh install requires the verification operation before engine setup or hook installation. An already installed matching build can reuse its verified trampolines. The missing-operation fixture expects admission refusal.
- **Share the frame-dump admission transaction between Anomaly SR and motion.** frame_dump.rs, sr_stage.rs and motion/stage/capture.rs. Both stages use the same poll/request/begin transaction. The existing motion schedule fixture covers refusal without spending budget and the retry through that shared method.
- **Pass the seam operation directly between its two Rust owners.** sr_stage.rs, sr_swap.rs, sr_stage.cpp and sr_stage_platform.h. The stage only obtains the native controller handle during its lock. It passes a borrowed Rust seam operation directly to the swap owner, which calls it after verified restoration. The native opaque cookie and C round trip are gone from the production path.
- **Publish the actual bilinear fallback result.** sr_stage.rs, sr_temporal.rs and sr_stage_platform.h. The driver receives the reason and draw result after the spatial pass, counts success and failure separately, and emits a recovery status consistent with the result.
- **Derive the overlay mip-bias range from the SR option bounds.** sr_stage.rs and sr_stage/tests.rs. The offset descriptor derives both limits from the INI bounds and shares its divisor with the getter and setter. The fixture checks the endpoints and a positive value.
- **Keep pending GPU query results distinct from terminal failures.** sr_stage.cpp, sr_stage_platform.h, sr_stage/gpu_timer.rs and sr_stage/tests.rs. The native leaves return GetData HRESULTs. Pending queries are retained and a terminal failure makes its slot reusable. The existing stage fixture now covers pending, failure and a later accepted sample.
- **Exercise the production Win32 INI callbacks in the SR package test.** ini.rs and sr_stage/tests.rs. The disk-backed test now uses the two actual Win32 callbacks. Its injected filesystem probe stays separate.
- **Create only the depth state required by temporal preparation.** sr_d3d11/passes.rs. Temporal state creation requests only the no-depth state. Shared rasterizer and blend leases remain in the containing pass.
- **Store the creating device in the temporal target cache key.** sr_d3d11/targets.rs. The cache key uses the original creating device identity, which remains live through the owned textures. Matching dimensions and format no longer calls GetDevice.
- **Read swap counter buckets with the same constants used to increment them.** sr_stage.rs and sr_swap_abi.rs. Stage reports read the generated skip and restore constants used by the swap owner.
- **Anomaly SR stage state is an Option<Box> that every accessor lazily recreates.** sr_stage.rs and sr_stage/tests.rs. Stage state initializes once in LazyLock<Mutex<State>> and install holds one guard.
- **Reset the GPU timing report baseline when the D3D11 device changes.** sr_stage.rs, sr_stage/gpu_timer.rs and sr_stage/tests.rs. Timing samples and baselines are generation-bound. The report fixture records a nonzero sample and observes a zero count and average after device replacement.
- **Share the Halton sample calculation used by Anomaly and Skyrim.** sdk/game/src/sampling.rs, sr_stage/math.rs and games/skyrim-se/src/resolution.rs. One algorithm supplies f64 and f32 versions without changing arithmetic precision or projection conventions.
- **Keep mapped row data inside the mapping guard.** sr_stage/staging.rs, sr_stage/burst_file.rs and sr_stage/staging/tests.rs. Layout carries only descriptor metadata. Byte slices borrow the map guard, which unmaps on drop.

### Native file and fixture disposition

| N06 paths | Disposition |
| --- | --- |
| `games/anomaly/src/sr_plan.h`, `tests/anomaly_sr_plan.cpp` | Removed. Production and fixtures use the Rust plan implementation. The native swap fixture calls its narrow plan entry directly. |
| `games/anomaly/src/sr_stage.cpp`, `sr_stage.h`, `sr_stage_platform.h` | Rust owns stage admission, locked state, policy and seam dispatch. Native retains guarded engine-memory and vtable access, hook installation and trampolines, D3D11 query leaves and the adapter's C++ entry surface. |
| `games/anomaly/src/sr_d3d11.cpp`, `sr_d3d11.h`, `sr_d3d11_platform.h` | Retained for COM, D3D11 state, shader compilation and SDK descriptors. The existing Rust pass and target owners keep allocation, refusal and lease order. |
| `games/anomaly/src/sr_reconstruction.h` | Native layout and guarded access leaves remain. Rust owns validation, correction and rollback verification. |
| `games/anomaly/src/sr_pass.h`, `sr_settings.h` | Native pass and settings adapters remain for C++ consumers. The duplicate native settings admission was removed. Backend-requested rectangle and settings admission fixtures now exercise the Rust owners. |
| `games/anomaly/src/sr_profile.h`, `tests/anomaly_sr_profile.cpp` | The Standard and AVX offsets and expected bytes remain in the native profile shared with guard evidence tools. The fixture checks relationships across the native profiles. |
| `tests/anomaly_sr_hooks.cpp`, `anomaly_sr_runtime_api.cpp`, `anomaly_sr_roundtrip.cpp` | Retained native hook, real Runtime API negotiation and D3D11 device fixtures, respectively. |
| `tests/anomaly_sr_reconstruction.cpp`, `anomaly_sr_pass.cpp` | Retained native ABI fixtures. Reconstruction coverage includes the new rollback result. The pass fixture keeps the CPU motion oracle and the real Rust pass through the C ABI. |

### Backend admission gap at the first checkpoint

**Let the Runtime choose and default the Anomaly SR backend.** The shared
D3D11 API's start leaf still calls the DLSS-specific pipeline. In
`runtime/orchestrator/src/dlss_pipeline.rs`, startup sets up Streamline,
checks DLSS support and obtains a DLSS render plan before allowing any
alternate selection. The evaluation path also requires Streamline to be
loaded. Using the Runtime's vendor preference alone would still refuse AMD
or Intel devices before their selected providers can run. Closing this
requires a generic Runtime start contract and alternate-first D3D11
startup, followed by the adapter Auto setting and elimination of local
availability/vendor ordering.

Source review compared ordered stage, swap and D3D11 callback declarations
(76, 11 and 23 slots), native initializers where designated, and generated
stage and swap offsets. The private stage table is 608 bytes on Windows
x64. Timing return types, the seam handle, the fallback outcome and the
constant-correction result were checked on both sides. The C++ and Rust
temporal counter layouts are declared as 432 bytes after adding the failure
counter. Git whitespace checks passed. Binding generation, compile, link,
lint and fixture/device execution were not run for this checkpoint.


## Runtime-owned Anomaly SR backend admission

The follow-up starts from `b9e0a480`. The integrated Runtime selected-provider
entry can start an alternate provider directly. This closes **Let the Runtime
choose and default the Anomaly SR backend.**

- `games/anomaly/src/sr_stage.rs` and `overlay.rs` pass the three configured
  package directories, the caller's concrete backend mask and the saved request
  to `rsf_d3d11_sr_admit` with a null device. The returned mask controls the
  panel and the service supplies Auto when an explicit request is unavailable.
  The adapter's vendor filenames, native file probe and fallback order were
  removed.
- `games/anomaly/src/sr_temporal.rs` binds admission and selected-provider
  startup from the carrier-owned Runtime. At the drained boundary the
  borrowed engine device is supplied to admission. The resolved concrete ID
  reaches startup or a live switch, and the persisted request retains Auto.
  Live requests reuse the admitted result for the same request and device.
  Startup or switch refusal preserves spatial recovery or the active provider
  as applicable. A Runtime without these exports cannot start Stage 5.
- The SDK admission binding shares `RSF_SR_AUTO` from `sr_session.h`.
  `runtime/orchestrator/src/overlay_service.rs` admits the bit, and
  `ui/overlay/src/panel.rs` offers Auto where the host advertises it.
  DLSS presets remain editable with Auto when the package offers DLSS.
- The Rust fixtures trace the pre-device mask and default, the actual device and
  directory supplied at startup, and the difference between requested Auto and
  the concrete provider sent to startup. Build, lint, fixture and device
  execution remain pending.


### Reconstruction and scalar fixture port

- Removed `games/anomaly/src/sr_reconstruction.h`. The shared private C
  stage header now declares the 72-byte layout and 32-byte callback table.
  `sr_stage.cpp` passes the same profiled destination, buffer and stage
  offsets with the three synchronous guarded native leaves. The Rust owner
  continues to admit, write and verify restoration.
- Replaced `tests/anomaly_sr_reconstruction.cpp` with
  `games/anomaly/src/sr_reconstruction/tests.rs`. It exercises the real
  projection and correction operations, all six staging destinations, aliases,
  refused layouts and every single write failure, including incomplete
  restoration. The native swap reconstruction-window fixture remains registered
  for the engine field handoff.
- Moved scalar INI cases from `tests/anomaly_sr_hooks.cpp` to the Windows Rust
  fixture in `games/anomaly/src/sr_stage/tests.rs`. It calls the actual Win32
  reader, checks selection without configuring hooks, and retains extent,
  jitter, capture, threshold and backend-admission cases. This removes three
  native INI reader wrappers and the unused native scale setter. The native
  hook fixture still checks guard refusal and native lifecycle wiring.


### SR pass fixture port and remaining native boundaries

Removed `tests/anomaly_sr_pass.cpp`. The Rust pass fixtures in
`games/anomaly/src/sr_pass/tests.rs` cover motion validity, finite standard
depth, rectangles, resource identity, reprojection, pass refusals, reset
precedence and camera cuts. The fixture also submits failed inputs through
the retained C entry and checks that it clears the output record. The
camera, pose and reset adapters used only by the old C++ fixture were
removed from `sr_pass.h`, `sr_pass.rs`, the private stage header and its
bindings. The remaining `sr_pass.h` interfaces build the typed device
fixtures and provide their motion reference.

`sr_stage.cpp`, `sr_stage.h` and `sr_stage_platform.h` retain the native
read, hook, trampoline, D3D11 and bootstrap interfaces. Verified RVA and
layout data in `sr_profile.h` continues to participate in Standard and AVX
build profile tables, engine probes and the evidence scripts. The D3D11
files retain COM and shader compiler calls, native descriptor handling and
typed wrappers for the D3D11 device fixtures. The native hook, profile,
Runtime API and GPU round-trip fixtures exercise those boundaries and
remain in CMake. The CMake entries for the deleted CPU-only pass and
reconstruction executables were removed. The tests replacing them now
live in the Anomaly Cargo package.


The source check matched all 75 stage, 11 swap and 23 D3D11 callback
slots across the C declarations, Rust declarations and designated native
initializers. Stage and swap offsets matched the checked-in assertions.
It also matched the 72-byte position layout, 32-byte position callback
table, admission ABI version, Auto value and exported startup and admission
names. The named-table bind seam used by synthetic Runtime API fixtures
continues to exercise legacy startup directly. Production Stage 5 requires
the admission and selected-provider exports. Git whitespace checks passed.
Binding regeneration, builds, lint, fixtures and device runs remain pending.
