# Generation ownership and lease port

C04 moves the D3D11 FG table, dispatch, scene COM lookup, provider lookup, and
lease COM operations to Rust. The AC7 carrier still calls the same embedded
dispatch symbols. The Anomaly adapter still obtains the DLL's version-4 FG
table. The table's argument order, 120-byte x64 layout, refusal order, and
callback targets are preserved. The public capture table validates normalized version-1 depth and motion and derives the frame extent from the pass. The new lease teardown is an additive export.
The GameSDK records and C05 CPU dispatch contract keep their existing layouts.

## Findings

1. Scoped capture enters the scene owner's restoration guard. It uses the
   same binding and explicit refusal path as the ordinary scene setter.
2. Session switching and lease sealing use one retirement descriptor parser.
   It refuses the reserved fence target before polling. Both use the same
   fence-completion classification, including the device-removal sentinel.
3. Provider marker failure invalidates the ledger entry at the accepted
   marker. Prepare and finalization cannot reuse the failed entry. Abort
   releases the sequence through the existing provider path.
4. Shared SR evaluates in a separate transfer slot when an explicit FG lease
   already exists for the source. Its depth, motion, commands and completion
   fence are separate from the explicit FG slot. An SR result does not update
   that slot's provenance, readiness or HUD-less state.
5. Input-derived timing uses consecutive verified source receipts on the same
   presentation thread and session, chain, provider generation, viewport and
   view. It rejects frequency changes, unsigned wrap, backward time, large
   gaps and missing receipts. Repeated Prepare for the same receipt reuses
   the measured delta. When neither the engine nor an admitted measurement
   supplies a positive delta, configuration declines interpolation.
6. A separate lease-owner operation consumes retained references after the
   caller verifies device loss and tears down GPU work. It does not turn the
   failed retirement into success. The fault fixture keeps the failed fence
   at the removal sentinel through teardown.
7. FG session host/provider callbacks and the native D3D11 FG graphics table
   are checked once and stored in typed Rust tables. The findings for the
   other backend, latency, overlay and shader owners are separate work.
8. FG Prepare and the native window observer classify the scene already
   returned by the registry. The public standalone scene and classification
   queries preserve their existing contracts.

## Port inventory

The declaration-only paths under runtime/orchestrator/include/rescaleframe are
d3d11_fg_api.h, fg_choice.h, fg_leases.h, fg_session.h, frame_assembly.h,
frame_sequencer.h, gpu_policy.h, native_fg.h and native_scene.h. The private
headers runtime/orchestrator/src/fg_auto_choice.h and
runtime/orchestrator/src/native_fg_platform.h are also declarations.

The removed native implementations under runtime/orchestrator/src are
d3d11_fg_api.cpp, fg_leases_com.cpp, fg_providers.cpp, native_fg_dispatch.cpp and
native_scene_platform.cpp. The fixtures tests/fg_choice.cpp,
tests/frame_assembly.cpp and tests/gpu_policy.cpp moved to tests/generation.
The choice fixture still calls the C inline SDK reference for default FG
preference. SR policy retains both DLL and embedded coverage. Assembly still
calls the DLL, and its motion-sign check now exercises the Rust runtime helper.

The follow-up removes runtime/orchestrator/src/native_fg_platform.cpp and
runtime/orchestrator/src/fg_session_platform.cpp. The graphics table now has
required Rust function types at construction. The platform module keeps the
existing shared-surface service, paired command allocation, closed initial
command list, texture layout, full-resource barrier order, 10-second fence wait,
and exact C calls to the presentation bridge and frame dump services. Failed
COM outputs retain an owner until release. The session's C ABI entry and fence
query are Rust. The small runtime/orchestrator/src/fg_frame_policy.c leaf uses
the GameSDK's original inline admission predicates, which native backends also
use. The C05 platform in runtime/orchestrator/src/native_fg.cpp continues to
bridge DXGI adapter discovery, Streamline and presentation callbacks.

- tests/d3d11_fg_api.cpp retains independent C++ DLL import and typed callback
  identity checks alongside the separate C11 import fixture.
- tests/fg_session.cpp retains the public ABI session host and provider mocks and
  adds refusals before and after Prepare.
- tests/fg_leases_faults.cpp retains the C COM fake vtables and adds terminal
  teardown coverage without making the removed fence complete.
- tests/fg_leases.cpp retains real D3D12 queue and fence retirement.
- tests/fg_provider.cpp and tests/fg_switching.cpp retain native provider,
  swap-chain, device and window paths.

Source verification covers pinned Rust formatting, regeneration of the changed
bindings with bindgen 0.72.1, locked offline Cargo metadata, and the C entry
point and CMake source lists. Build, test, lint, DLL export inspection and
device execution are pending.
