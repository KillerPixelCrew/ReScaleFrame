# AC7 TrueSky depth and cloud production

The stationary wing gaps and distant blocky clouds were accepted as clean by the user on
2 October 2026 after the 21:24 deployment. This reference records why that path works and
why earlier clean-looking results were regressions. It is specific to the installed DLL/effect.

## Fingerprints and owners

- `TrueSkyPluginRender_MT.dll`: SHA-256
  `c6c58ea3a355ed345888b1528e84aa753c6fc69db9c8fb8789e729c7651004d0`, 1,711,616 bytes.
- `mixed_resolution.fxo`: 45,263 bytes, CRC32 IEEE `78881ef3`; full SHA-256 and pass/shader
  fingerprints are in [the hook map](../../../../docs/research/ue418-hook-map.md).
- Runtime implementation: [native_renderer.cpp](../../../../games/ac7/src/native_renderer.cpp)
  and [truesky_depth.cpp](../../../../games/ac7/src/truesky_depth.cpp).
- Captures, revisions, precision results and limitations:
  [aircraft/cloud research](../../../../docs/research/ac7-plane-artifacts-20261002.md) and
  [engine evidence](../../../../games/ac7/engine.json).

| Native site | Role |
| --- | --- |
| `0x87f6d` | Consume cloud downscale policy before coordinated native allocation |
| `0xbfb30` | Scene-depth-bounds producer |
| `0xbfc05`, immediate `0xbfc09` | Guarded native depth format request |
| `0xbff0b` | Guarded mixed-resolution pass-selection branch |
| `0xc003f` | Guarded native dispatch call/first-party relay |
| `0xeae10` | `TRUESKY_EnsureTexture2DResourceViews`, rebuild native texture/SRV/UAV |
| `0xefb70` | `TRUESKY_MapPixelFormatToDxgiFormat` |

The format guard at `0xbfc05` is `c744242016000000ff50588b87940000`; native format immediate
`16000000` becomes `05000000`. Enum 22 maps to DXGI 35/R16G16_UNORM; enum 5 maps to
DXGI 16/R32G32_FLOAT. These are TrueSky's enums, not UE pixel formats. Other expected-byte
guards and restoration logic are authoritative in current source/hook map.

## What was actually failing

Coarse x2 depth mixed aircraft/background within one cloud mask footprint. Normalized radial
distance stored in UNORM lost near-aircraft precision: captured conversion `(0.1, 150000, 0, 0)`
gives roughly 2.289 native distance units per UNORM step. Do not label those units metres without
establishing AC7's actual conversion. This is upstream masking, not proof of missing mesh triangles.

For captured aircraft depth, actual native UNORM GPU output had up to 8.57% relative error.
The earlier 7.683% estimate used CPU round-to-nearest conversion and was not the exact GPU result.
RG32F x1 replay reduced the maximum relative error to about `3.23e-7` (0.0000323%), with
maximum absolute error `1.79e-7`. This establishes producer precision, not each visible pixel's cause.

The reference's x1 allocator arithmetic did not imply an x1 compute pass. The shipped effect
contains x2/x3/x4 depth permutations. Simply forcing divisor 1 skipped selection but still
dispatched and cleaned up, leaving a broken sky with missing volumetrics. A clean aircraft in
that 19:10 run was not acceptance.

The next attempt compiled an x1 CS but guarded for Texture2DArray. Native `0xeae10` creates a
Texture2D UAV. The 20:13 captures still showed native x2, cloud colour 268x152 and no verified
replacement dispatch. Shader compilation alone did not activate the proposed fix.

## Accepted producer correction

The first-party CS uses `t1` source depth, `s15` nearest clamp, `b11` native mixed-resolution
constants and `u0` `RWTexture2D<float2>`. It dispatches 8x8 groups, respects target dimensions
and source offsets, converts depth to normalized radial distance, retains the native sky-zero
convention and writes the same one-pixel distance as both maximum and minimum bounds.

The format patch changes only normalized depth bounds through their native owner. Texture,
SRV and UAV rebuild together. The dispatch relay uses the replacement shader and restores the
original before TrueSky's Unapply so cached native state and cleanup stay valid.

Prepare on the actual D3D11 device, then verify CB/SRV/UAV dimensions, view type and RG32F
format. Only after readiness does the downscale consumer use divisor 1. Full scene-grid cloud
colour/depth/history/tile allocation remains native and coordinated. At 1600x900 Ultra, opaque
DLSS input is 533x300 and padded clouds are 534x300, exceeding the user's stock Balanced cloud
baseline. This preserves opaque render scale; it is not output-resolution clouds or post-SR recomposition.

TrueSky vertical projection handedness is corrected for the matched primary view. That earlier
jitter fix is distinct from the later depth/precision correction. Do not apply a global jitter sign
flip or assume depth correction supplies cloud motion vectors.

## Failure and retirement behavior

Device recreation rebuilds its shader. Before readiness, supported native cloud resolution remains.
After activation, a rejected binding restores divisor 2 for the next native allocation and disables
full-grid reactivation. Suppress incompatible x2 dispatch on existing x1 targets, restore the pass
shader and balance cleanup. That frame loses a depth update; it must be reported.

Quiesce execution and restore format/divisor/pass/call patches before retiring callbacks/relays.
A failed restore retains reachable resources/module code. Do not relax guards when sizes or
view types change; establish the new owner's contract.

## What each check proves

Captured-depth RTX4070 replay covers 534x300 and padded 538x302 x1 targets against the native
x2 268x152 path with the captured 352-byte CB. Texture2D and one-slice array values matched
on that device; the shipped owner and replacement still explicitly use Texture2D.
Build/gate checks prove compilation/contracts. Runtime logs prove binding/activation/dispatch.
The user-run aircraft/cloud overlap with volumetrics present supplies visual acceptance.
Cloud wind/history, independent motion vectors and other GPU visual coverage remain open.
