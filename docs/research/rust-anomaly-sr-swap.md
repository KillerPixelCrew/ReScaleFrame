# Anomaly SR world-window swap in Rust

The cutover moves the SR world-window swap and restore owner from
`games/anomaly/src/sr_swap.cpp` into `rsf-anomaly` (`sr_swap.rs`). It starts
from the merged Rust adapter at `9d53556` on `rust/orchestrator-anomaly`. The
[isolated-game C++ baseline](rust-anomaly-cpp-baseline.md) at `8322179a36c7`
supplied the export comparison. This cut uses the existing engine evidence in
[the SR seam research](anomaly-sr-seam.md) and introduces no new addresses,
guards or shader variants.

## Ownership

| Boundary | Owner after the cut |
| --- | --- |
| Eligibility at H1 | Rust: swap off or refused, scope (SVP) frames, MSAA, incomplete Stage 2 facts, verified CRT/CTexture layout, distinct records, absent slots, Device size against the facts, HW.pBaseZB and HW.pBaseRT. |
| Twin key and leases | Rust keys twins on stage, CRenderTarget, every CRT and surface, engine DSV and RTV, device, reset generation and both extents. An allocation failure holds that exact key stock; a refusal or a twin mismatch releases the twins and refuses. The twin generation still feeds the capture record's `resource_generation`. The native host keeps the D3D11 textures and views it creates and releases them in `drop_twins`. |
| Transactions | Rust sequences the 11 field writes per slot, the Stage 2 globals and SSAO static, the RCache texture and constant-table invalidation, and rolls back on any failed write. Restores check the window values first and refuse on foreign changes. |
| Stage 3 | Rust jitters Device.mProject/mFullTransform, jitters each fresh first-person projection once from the rmNear slot, applies and restores the mip bias, suspends and restores jitter around sun cascades and supplies the raster jitter to the deferred position constant. |
| Native leaf | `sr_swap.cpp` and `sr_stage.cpp` keep the SEH-guarded engine copies, `R_xforms::set_P` and `SetMipLODBias` calls under SEH, twin creation, the seam passes, the viewport, hooks, both executable profile guards and capture. Rust reaches them only through `rsf_anomaly_swap_platform`. |

The engine layout stays in `sr_profile.h`, shared by the Standard and AVX
profiles. The facade passes it to Rust once at creation and keeps the
compile-time checks against `target_slots`. The profile guards in
`sr_stage.cpp` are unchanged, and the hooks contract still exercises both
executables.

## Shared binding

`sr_swap_platform.h` declares the records, layout, platform callbacks and the
view the native side reads between calls. `sr_swap.h` uses those C structures
directly, so the C++ facade has no second definition. Bindgen 0.72.1 with
libclang 18.1.1 generated `sr_swap_abi.rs` through
`tools/generate-rust-bindings.py`. The same run reproduced every existing
binding byte for byte. The merge publishes the view in a disjoint part of
the Rust handle after each call. This keeps the facade's pointer stable
and lets the native seam read the last published twin generation while
Rust mutates the swap owner. A fake upscale callback covers that read.

## Superseded C++

The `SwapController` implementation in `sr_swap.cpp` is replaced by a forwarding
facade. `first_person_action` in `sr_jitter.h` and `twin_extent` in `sr_plan.h`
had no other production caller and were removed. Their C++ cases moved to Rust
unit tests. `jitter_engine_projection` and `engine_mul` stay in `sr_jitter.h`
because device and reconstruction fixtures use them. The swap contract now
compares the Rust result against them with exact float equality.

## Defect fixed

A refused sun cascade closes the window mid-frame. The C++ owner used the
prepare-time rollback for this case, which skips `CRenderTarget::dwWidth/dwHeight`
and the SSAO first-call value and leaves the render-size viewport from the H1
epilogue in place. The rest of the frame then renders stock at output size with
render-size engine dimensions and viewport. Every other mid-frame exit
(`combine_end`, `frame_end`, `close_stale`) restores both.

The new `stage3_sun_rollback` swap case was run first against the unchanged C++
owner. Built from `9d53556` in MSVC Debug, the 27 existing cases passed and the
new case failed six checks: all three failure paths left the stale engine
dimensions and the render-size viewport in place. The Rust owner keeps the
rollback's skipped foreign checks for partial writes. It restores the engine
dimensions and SSAO value and resets the viewport to output size. The same case
passes in Debug and Release, and the Rust test
`sun_refusal_returns_the_frame_to_output_size` covers it too. This evidence
comes from source inspection and the contract test. No game frame has shown
the failure.

## Validation

MSVC 19.52 (Visual Studio 2026 generator) built a worktree-local tree with its own
`FETCHCONTENT_BASE_DIR`. The coordinator's MinHook source and pinned yyjson were
read-only inputs. The yyjson download returned a different `yyjson.c` hash, so
the three pinned files were copied from the coordinator checkout after their
SHA-256 values matched the lock. Debug and Release built the adapter DLL, the
CPU contracts and the round-trip, jitter and temporal device fixtures without
warnings. The device fixtures were built, not run.

These 16 CTest contracts passed in both configurations: swap, unknown swap
selection, sun cascades, reconstruction window, SR hooks for Standard and AVX,
jitter, unknown jitter selection, plan, profile, reconstruction, pass, unknown
pass selection, input hooks for Standard and AVX, and the Runtime SR API. The
full swap contract ran 28 cases with no failures in each configuration.

`cargo fmt --all --check`, `cargo clippy --workspace --all-targets --locked
-- -D warnings` and `cargo test --workspace --locked` passed: 231 tests,
including 11 new swap tests. They cover the jitter rule, row-vector
composition, the first-person rule, twin extents, Stage 1 byte restoration,
Stage 3 main and first-person jitter, sun-cascade tokens and reentry,
sun refusal, scope frames, rollback at each exchange write, and creation checks.

The Debug and Release Anomaly DLLs export `rsf_anomaly_observation`,
`rsf_anomaly_start` and `rsf_get_game_plugin_api` at ordinals 1 to 3, the
same as the staged `8322179a36c7` DLL. Neither the new DLLs nor the baseline
import the Runtime statically; they look up `ReScaleFrame.Runtime.dll` by
module name through `GetModuleHandleW`.

## Gaps

No device fixture, visible window or game ran on this revision. The fixed
sun-cascade path still needs a game frame that refuses inside
`render_sun_cascades`, which the normal path does not produce.

## N07 residual source checkpoint

Base commit `8df04a40`. This pass follows the pinned Standard and AVX
profile, S-B seam and render history in the [Anomaly SR seam research](anomaly-sr-seam.md)
and the [N06 stage checkpoint](rust-anomaly-sr-stage.md). The profile file,
shader tree and temporal reference tree have Git object IDs
`82c08110670d05f95f58d404dc6ad1e445033453`,
`c0df39d77fcd9c5b12f12b23089b2fff36cda40f` and
`7ad34176e364d3d9650299bc75924488b87f4cd7` respectively.

| Path | Source disposition |
| --- | --- |
| `games/anomaly/src/sr_swap.cpp`, `sr_swap.h`, `sr_swap_platform.h` | The native controller still checks the profile layout and passes guarded memory, engine calls and twin leases through the existing private ABI. The Rust owner checks completion before reporting successful restoration or authorizing release. The production scoped seam still enters from N06. |
| `games/anomaly/src/sr_swap.rs`, `sr_swap/tests.rs`, `tests/anomaly_sr_swap.cpp` | Read back restored matrix bytes, sampler bias and cache spans. An incomplete return leaves the affected state and twin lease pending across H1 and quiesce. The first-person pair is retried while rmNear still owns it, and an unresolved rollback forces spatial recovery. Fixtures cover persistent target, global, cache, camera and sampler failures, then cleanup retries. |
| `games/anomaly/src/sr_jitter.h`, `tests/anomaly_sr_jitter.cpp`, `tests/anomaly_sr_jitter_fixture.rs`, `tests/anomaly_sr_jitter_reference.h` | Seven numerical cases now run in Rust through the production helpers and existing math ABI. The runner keeps the same case names and unknown-selection status. The independent transpose, jitter and Fmatrix oracle lives with the native fixtures. |
| `games/anomaly/src/sr_temporal.cpp`, `sr_temporal.h`, `sr_temporal.rs` | The guarded adapter call reads the Runtime's borrowed D3D11 texture descriptor in typed Rust. The output-format result still uses zero for an unavailable texture. The Runtime table, temporal driver API, negotiation and settings are preserved. The native facade still adapts C++ callers and catches C++ log exceptions. |
| `games/anomaly/src/sr_tonemap.h`, `sr_tonemap.rs` | The existing adapter for the Rust low-tonemap and UNORM8 reference remains the colour oracle for round-trip and temporal fixture checks. The device reads still inspect the same format and shader outputs. |
| `tests/anomaly_sr_jitter_device.cpp`, `tests/anomaly_sr_temporal_device.cpp`, `tests/anomaly_sr_readback.h`, `games/anomaly/src/sr_fixture_readback.rs` | The two D3D11 fixtures share typed Rust staging creation, resource copy, map, row copy and unmap. The jitter case reuses its existing staging texture. Native fixture setup, shader bindings, debug checks, vendor selection, numerical comparisons and skip codes remain in their original drivers. |
| `tests/anomaly_temporal_tests.cpp` | The native temporal model remains independent. Its projection comparison also checks production refusal of a degenerate history extent. |

The projection offset remains in render pixels with positive X right and
positive Y down. The reference tracks a separate scope history, previous
render dimensions and the existing motion cancellation convention. The
retained temporal fixture compares the FP16 scene colour, R24G8 depth and
motion validity at the existing decode, resolve and reinsertion points.
The existing shader source and provider code set those contracts.

For a closed window, the saved target, global, camera and sampler state
remain live until verified restoration. A retry checks each outstanding
CRT and CTexture chain before touching its saved addresses. The native
resource destructor still retains a live twin batch if quiesce does not
finish. No D3D11 device or game was used for this pass.

Source checks used Rust 1.94.1 formatting, bindgen 0.72.1 regeneration,
locked offline metadata for Windows x64, Git whitespace checks and a
comparison with the pinned ABI, shader, profile and history files.
The isolated verification Cargo cache lacked `ash`. The existing default
Cargo cache resolved the same lockfile offline. Builds, fixture execution,
Clippy, and Standard and AVX game validation remain open.

## N07 swap scenarios from 6cb0c169

The 29 original named CTest scenarios in `tests/anomaly_sr_swap.cpp` at
`6cb0c169` now run in `games/anomaly/src/sr_swap/tests.rs` and
`sr_swap/tests/scenarios.rs`. The runner preserves the case names and
unknown-selection result. The native entry lends the production
`verified_layout` and the independent jitter and Fmatrix oracle.
The Rust runner compares the fixture layout field by field and checks
actual main and first-person swap output against that oracle.

The Rust cases retain stage admission refusals, allocation retry keys,
the 22 Stage 1 write faults, all Stage 2 exchange and cache write faults,
optional and odd-size targets, exact state and viewport restoration,
the SSAO init and in-progress branches, all four non-seam exits, and
set_P, mip and sun failures. They observe the restored stock bindings as
the seam callback sees them, then exercise repeated restoration, lost
CRT chains and live twin retention. The native production guard and sun
unwind paths and both profile tables remain those of `6cb0c169`.

Pinned formatting, locked offline Windows metadata and source comparison
completed. Builds, fixture execution and device validation remain pending.

## N07 temporal planning scenarios from 6cb0c169

All 17 names from `tests/anomaly_temporal_tests.cpp` at `6cb0c169`
are retained. `games/anomaly/src/temporal_fixture.rs` owns selection and
results. Its `math.rs` covers dimension admission, native matrix indexing,
projection, inverse, motion math, and comparison with production Rust
jitter. Its `history.rs` exercises per-view and first-person history,
snapshot identity, malformed producer refusals, retirement and the
separate geometry, reconstruction, screen UI and PDA canvases.

`tests/anomaly_temporal_oracle.h` only lends the independent C++ model
from `games/anomaly/temporal/{planning,math,history,frame_plan}.h`. Opaque
capture owners keep their native shared pointers alive until Rust drops
them. The fixed C callback records come from
`games/anomaly/temporal/fixture_platform.h` and bindgen generates
`games/anomaly/src/temporal_fixture/abi.rs`. The native oracle headers
that implement the model remain byte-identical to `6cb0c169`.

Source comparison matched all 29 swap selections, all 17 planning
selections and all 35 CTest labels in `tests/anomaly_support.cmake`.
The cleanup also makes jitter, round-trip and temporal device scenarios
use the same debug queue reader. It preserves the original jitter
assertion that no debug errors are present.

### F01 typed callback admission from 10dac39f

- `games/anomaly/src/sr_swap.rs` admits the eight common leaves when the
  handle is created. The native seam constructor still requires both
  draw callbacks. The scoped constructor accepts either callback as
  absent and keeps the native entry's draw refusal if that callback is
  omitted. Production continues to supply its Rust seam operation.
  Readback verification, native fault return values and twin retirement
  remain in the same transaction order.
- `games/anomaly/src/sr_temporal.rs` validates the existing API size, API
  and pipeline versions, and all twelve Runtime entries before storing
  typed callbacks. Reset and sharpening remain ABI requirements.
  Production requires both selected-provider start and admission
  exports, while the synthetic table keeps its DLSS-first start. The
  colour transport export remains optional. Refusal messages and the
  original returned record are preserved for the Runtime query fixture.
- `sr_swap/tests.rs` checks each absent common leaf, the legacy seam's
  requirements and a scoped draw with neither native seam callback.
  `sr_temporal/runtime_fixture.rs` submits all twelve missing-entry cases
  and still checks that refusal leaves the output record untouched.
  These are fixture source checks for this checkpoint.

Only the internal Rust representations changed. Targeted bindgen
regenerated the existing swap, stage, Runtime SR and admission records.
Pinned formatting, locked offline Windows x64 metadata and source
comparison passed. No compilation, fixture execution or device results
were gathered.
