# AC7 carrier startup and renderer lifecycle

A01 started from `ab173d96`, with the A03 capture and L01 loader changes present.
The question was whether the carrier could preserve its admitted Present dimensions,
expose install-time controls and keep refused native hooks reachable until retirement.
This is a source migration of the existing Steam 9855922 integration. The executable
identity, native frame inputs and expected patch windows come from the existing
[carrier startup](rust-ac7-carrier-startup.md), [renderer transaction](rust-ac7-renderer-transaction.md)
and [engine record](../../games/ac7/engine.json).

## Review findings addressed

- **Expose the AC7 carrier's startup toggles as adapter host options.** `games/ac7/src/carrier.rs`
  describes the choices. `loader/proxy/src/dinput8_proxy.c` loads them before observer
  setup and publishes their values through the existing generic panel contract in
  `loader/proxy/src/dlss_bridge.c`. Numeric values use the shared profile write lock
  in `runtime/orchestrator/src/settings_store.rs`. Saved values in the per-user AC7
  INI's `Startup` section take precedence over the legacy environment and local
  INI defaults. Changes are labeled for the next game start. A zero output width
  or height uses the matching Present dimension. The three RVA overrides remain
  developer settings and require the same expected bytes as the default sites.
- **Pass the admitted Present extent into automatic startup.** `carrier.rs` passes
  the same nonzero width and height that spent the automatic attempt. The manual
  action takes its own snapshot and calls the same start-at-extent operation.
- **Share the GameSDK call-header validator across adapters.** The handwritten
  `sdk/game/src/call_header.rs` checks size before version for AC7, Fallout 4
  and Unity Mono. The separate, size-only probe retains its existing validation.
- **Distinguish a prepared renderer from one retained only for cleanup.**
  `games/ac7/src/lifecycle.rs` keeps the retained renderer busy to new prepare
  calls, reports it unprepared and refuses start. Quiesce and stop still reach
  that handle. The lifecycle fixture covers a stop that needs a retry.
- **AC7 Ops re-unwraps leaves that publish already proved present.**
  `games/ac7/src/lib.rs` stores the complete leaf table as function pointers.
- **Reuse the AC7 synthetic image setup in renderer, site and patch tests.**
  `games/ac7/src/fake_code.rs` provides independent, base-aware seed methods
  for admission and for temporal patches.
- **AC7 NativeCode keeps validated Option leaves and re-checks them per call.**
  `games/ac7/src/sites.rs` and `renderer.rs` retain validated callbacks directly.
- **Make exact-build admission accept a read-only code view.** `sites.rs`
  admits compare-only tables. Patch and restore still require a writer.
- **Check the AC7 target initializer fingerprint once during admission.**
  `sites.rs` keeps the guarded entry at hook 23. The C++ direct call uses
  that table entry and the site fixture checks for duplicate admission reads.

## Port and verification

Rust owns the capture hook batch order and rollback in `hook_batch.rs`.
`ac7_hooks.h` supplies the existing MinHook calls and preserves its cleanup
results. The session fixture runs its scale, draw and preferences cases from
Rust under the existing CTest target. The native graph fixture still constructs
private UE objects in the renderer translation unit and drives actual D3D11
resources and detours. The production renderer retains the native detours,
COM operations, MinHook leaves and SEH-guarded reads. The game DLL export
and exception guard remain in C++. The pinned I01 input-facts link change
is independent of these edits. G02's frame-tap and promotion changes
touch a different part of the carrier bridge.

Checks were source-only. Pinned Rust 1.94.1 formatting, locked offline Cargo
metadata, source contract comparisons and `git diff --check` passed.
Targeted bindgen 0.72.1 regeneration and pinned formatting reproduced the
AC7 platform, site and scene-color bindings and added the GameSDK module
declaration. Build, lint, fixture execution and game acceptance remain pending.

## Startup, lifecycle and fixture follow-up

Source base `0251a81a` includes shared input classification and the A04/A05
velocity changes. This pass traces the remaining startup and lifecycle paths
for Steam build 9855922 using the executable fingerprint in
[engine.json](../../games/ac7/engine.json) and the recorded
[startup evidence](rust-ac7-carrier-startup.md#guarded-sites).

- `loader/proxy/src/dinput8_proxy.c` calls the measure-only decryption wait in
  `games/ac7/src/carrier.rs` through `games/ac7/include/rescaleframe/ac7_carrier.h`.
  The wait requires two consecutive entropy samples below 7.0, sleeps 250 ms
  after each incomplete sample and returns the measurement failure or the
  original 180 s timeout result. The optional dump and preparation calls retain
  their order.
- The same carrier paths use the original 82-byte jitter relay template for
  the gate at `0x112b1f3`, owned by `PreVisibilityFrameSetup` at `0x112afa0`.
  The three exit displacements reach gate + 6, gate + 6 and the stock JNE
  destination. The entry displacement uses gate + 5 for `E9 rel32 90`. A
  displacement outside rel32 range refuses before writing the page, then the
  carrier releases it and takes its existing all-views fallback. The guarded
  patch windows and inline storage retain their original bytes and offsets.
  Rust also computes the jitter sample count and the translucency scale and
  ceiling, using the existing scale ratio in `games/ac7/src/scene_color.rs`.
  C retains executable allocation, cache flush, console calls, live
  size publication and immediate writes.
- `games/ac7/src/lib.rs` and `games/ac7/src/renderer.rs` dispatch the
  lifecycle operations directly using the guarded per-call snapshot from
  `games/ac7/src/native_renderer.cpp`. Only `games/ac7/src/plugin.cpp` called
  the former private wrappers in `games/ac7/include/rescaleframe/ac7_native_renderer.h`.
  The replacement uses `games/ac7/src/ac7_platform.h` and
  `games/ac7/src/ac7_sites.h`. Prepare preserves the cleanup-only handle and
  maps a failed snapshot to the former prepare refusal. Other snapshot
  failures follow the existing exception path. Pinning still uses an address
  in the same native module. The detour bodies retain the pixel enqueue caller
  address capture and the original C++ engine unwind scopes.
  `games/ac7/src/tests.rs` preserves the GameSDK publication refusal case.
- `tests/ac7_native_graph.cpp` passes C++ hook indices and production
  protected compare/write callbacks to `tests/ac7_view_fixture.rs`.
  The Rust fixture checks the native enum against the hook table and runs
  temporal gate mismatch refusal, duplicate install, foreign-byte
  retention and final restore. C++ still supplies the independent UE graph
  objects, guard scopes, queued calls and D3D11 resources.
  `tests/CMakeLists.txt` tracks the new fixture binding dependency.
  `tests/ac7_session.rs` adds relay relocation, refusal, inline storage and
  scale cases using writable test data.
- `tools/generate-rust-bindings.py` generates `games/ac7/src/carrier_abi.rs`,
  `games/ac7/src/platform_abi.rs` and `tests/ac7_graph_patch_abi.rs` from the
  C declarations. The carrier table remains version 2 and 72 bytes.

Source comparison verified the relay template and patch windows, native
detours and caller checks, carrier exports and linkage, shared D3D11 host and
the A02 view owner against the base. Rust 1.94.1 formatting, bindgen 0.72.1
regeneration, locked offline Windows metadata and whitespace checks passed.
Compile, link, fixture, device and game verification remain pending.
