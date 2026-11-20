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
binding byte for byte. The view is owned by the Rust handle and updated by each
call, so a held `counters()` reference still sees later counts as before.

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
`render_sun_cascades`, which the normal path does not produce. The native
SwapHost callbacks may read the view while a Rust call is in progress. The
only current case is `twin_generation()` from the Stage 5 seam, which does
not change during the seam.
