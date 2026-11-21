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
