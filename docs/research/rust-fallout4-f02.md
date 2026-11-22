# Fallout 4 render-input ownership in the F02 port

This is source review of the AE 1.11.240 adapter on base `70e01825`. The native layout, expected
instruction bytes, slot maps and producer order come from the recorded Fallout 4 research.
The camera still uses the recorded DirectXMath SSE2 arithmetic. This work adds no new offsets or
game observations. See the [native input audit](fallout4-native-inputs-20261116.md),
[depth domains](fallout4-depth-domains-20261116.md) and
[reduced output scope](fallout4-native-reduced-output-scope-20261116.md).

## Input and camera leases

- The prepass temporal sample, raster sample and resolution owner now read the current thread's
  TLS slot array through one checked path. Both depth samples still read the TLS device context
  before applying the global render Context fallback. Resolution uses the Context alone. Each
  call reads fresh native state. A missing slot array refuses before reading the indexed block.
- Both input readers snapshot the physical colour, motion and depth records, retain the typed
  COM resources, then compare the live records at the same mapped slots before publishing. A
  mutable record can change while its mapped slot stays constant. The source fixture replaces
  colour, motion and depth view records in place. Temporal retains its individual refusal
  stages and R24/R32 support. Raster recovery still requires R24, sample quality zero and all
  three auxiliary allocations.
- Camera and depth composition share the projection comparison and signed pixel conversion.
  Jitter is measured against the native viewport's fractional extent. Camera construction
  checks a two-pixel magnitude bound and depth admission compares the submitted camera values.
  Camera history and the input reader use one bitwise raster comparison, including signed zero.
- The admitted temporal prepass snapshot carries its validated near, world and unjittered
  projection matrices by value. Execution still verifies the live source, camera, context and
  depth association before using them. Raster-only snapshots carry no valid temporal maps.
- Rust camera fixtures retain the fractional projection, eight jitter phases, recorded large
  origin, relative history, forward W and first-person view-Z checks. The native padding fixture
  calls the Rust comparison over the C++ receipt layout. Rust exercises the individual fields.

## Resolution retirement

Before the selected ordinary World begins, the same render thread can retry a retained input or
output scope whose native frame has advanced. The World guard checks caller, CPU source and
scope ownership before retrying. Cleanup separately revalidates live renderer and Context
identity, and compares each pending write to its saved and written bits. Live values that
superseded a write cause no restore. A newly observed live Context retires the stale shadow
association without following the old pointer. Shadow cleanup preserves unrelated dirty bits.
Failed reads and ambiguous equal bits under a different renderer retain their obligation.
When the pending work clears, the owner count retires, and an advanced scope remains refused for
its original source. The synthetic fixtures inject a failed restore, exercise native counter
advancement with a pending ratio write, and check that retirement preserves the superseding
value and releases its owner count. The World callback and device sequence still need user-run
validation.

## Remaining native boundaries

Depth and render-input COM queries, resource retention and resolution viewport observation now
use the repository's Windows bindings in Rust. The render hook owner still consumes the C++
bridge records and attaches the transferred references to its World. The stack lease guard
releases references on refusal or if the caller leaves a slot unused. The remaining C++ leaves
contain exceptions around original screen and host calls, or format the existing receipt text.
Screen orchestration, the native TLS load and the guarded output viewport call run in Rust. GameSDK screen classes
are generated from the C header and used by the Fallout, Unity Mono and RDR2 call sites in F02.
The S02 shared matrix comparison and render-plan changes compose with these paths.

Bindgen 0.72.1 regenerated the screen constants. Rust 1.94.1 formatting and Git whitespace
checks are source checks. Builds, native fixture execution, Rust tests, lint, capture and device
or game runs are pending.
