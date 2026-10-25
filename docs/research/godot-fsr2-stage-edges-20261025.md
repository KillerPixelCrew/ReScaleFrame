# FSR2 intermediate edge displacement, 25 October 2026

The six planned diagnostic cases completed and produced 24 actual intermediate
images. They localize the persistent positive horizontal shift to the temporal
boundary path: reprojected history is already displaced, and history
rectification sometimes advances it further. They do not establish a production
correction. All original image gates and failed controls remain unchanged.

This follows [the earlier reconstruction controls](godot-fsr2-controls-20261025.md).
Those controls rejected precision, allocation extent, jitter sign changes and
an exact output-resolution motion oracle. The earlier history-weight probe
measured an eroded object core; it did not explain silhouette coverage.

## Saved-data result

The new `tools/diagnostics/analyze-fsr2-stage-edges.py` uses the existing cyan
material weight and decomposes its centroid error into exact first moments of
the reference's three-pixel-eroded core and remaining boundary region. It also
records horizontal coverage profiles. This is a diagnostic decomposition, not a
replacement image gate.

For stock ordinary frame 14, source displacement is
`(+0.64492, -0.13032)` output pixels. The boundary contribution is
`(+0.63213, -0.12699)`; the core contributes only `(+0.01278, -0.00333)`.
The previously nearly passing candidate's aged frame 42 similarly splits
`(+0.42125, -0.05905)` into boundary `(+0.40592, -0.05620)` and core
`(+0.01533, -0.00285)`. The other six measured frames agree that boundary
coverage dominates. The actual stock frame-14 and candidate aged frame-42
comparison PNGs were inspected; opposing cube edges carry the main residual.

Saved-data reports:

- `.local/godot/fsr2-diagnostic/stock-stage-edge-moments.json`
- `.local/godot/fsr2-diagnostic/candidate-aged-stage-edge-moments.json`

## Controlled SDK stages

`tools/diagnostics/prepare-fsr2-stage.py` checks stock accumulation shader SHA-256
`F9564CB2CA4DE0D50DC852A01F1FAC71C066A4BE92A8BCC9A27AAAC345D082C6`
from FidelityFX Vulkan commit `c6efa6bf7f2027b3ec94f28578bb5965eabb9e55`,
FSR2 2.3.3. Three isolated SDK builds replace only the final displayed output:

1. `current`: `ComputeUpsampledColorAndWeight` color.
2. `history`: `ReprojectHistoryColor` result before `RectifyHistory`.
3. `rectified`: history immediately after `RectifyHistory`, before accumulation.

Each diagnostic value remains in signed YCoCg until
`UnprepareRgb(YCoCgToRGB(value), Exposure())` at the final output store. The
normal `StoreInternalColorAndWeight`, accumulation, locks and temporal reactive
factor remain unchanged and execute first. `UnprepareRgb` divides by exposure
and multiplies by current pre-exposure. No absolute value, saturation, history
reset or motion/jitter override was introduced. The source's signed YCoCg/RGB
transform was also checked numerically for round-trip behavior. Raw RGBA16F
readbacks retain negative current-slice RGB values, rather than silently clipping
them. The engine's later display conversion can clip those diagnostic values;
the tabulated material coverage below uses actual HUD-less display output.

The previous isolated provider jitter-sign hook was restored to its checked
baseline before building. The original compiler location and existing isolated
SDK build were reused. All three builds passed MSVC Release. No production SDK,
Native source, shared package or immutable runtime cache was changed.

The captures use the preserved standard engine
`E60D28D55689EF5DA3E28A98023032E21C6EB8C5264B5BF224EAF41AEB0034EF`
and Native
`EE22B1A698E6DD906E1C294A0DC72805C37857543368F379DE2E1F54713978F3`.
Ordinary source frames are 12 through 15; aged frames are 40 through 43 with
animation offset 28. Both use the existing matched current-pose 640x360 8x-MSAA
references, 426x240 scene inputs, and the original cube/floor fixture. The older
exact Streamline dependencies were reconstructed once from their retained SDK
artifacts; each case shares that read-only dependency directory and owns its
diagnostic FidelityFX DLL privately.

## Actual results

Horizontal material-centroid displacement in output pixels:

| Pose frame, ordinary / aged | Current slice ordinary | Reprojected history ordinary | Rectified history ordinary | Current slice aged | Reprojected history aged | Rectified history aged |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| 12 / 40 | 0.0729 | 0.3542 | 0.6147 | 0.0636 | 0.4092 | 0.6536 |
| 13 / 41 | 0.0315 | 0.6760 | 0.5688 | 0.1264 | 0.7128 | 0.6069 |
| 14 / 42 | 0.1912 | 0.4636 | 0.6855 | 0.1444 | 0.5306 | 0.7292 |
| 15 / 43 | -0.0947 | 0.7349 | 0.3957 | 0.0001 | 0.7676 | 0.4543 |

The current slice does not contain the consistent positive advance. History
already carries it before clipping. Clipping increases the advance at poses 12
and 14 but reduces it at 13 and 15. Thus simply blaming all displacement on
rectification would also be incorrect. Boundary first moments dominate every
stage; the largest core contribution is below 0.029 pixels. The retained signed
profiles support inspecting the color clipping box and surface coverage next,
alongside the motion field used to reproject that history.

## Provenance, validation and retained artifacts

The complete manifest is
`.local/godot/fsr2-stages-20261025/experiment.json`. It records every actual output
raw/metadata SHA-256, SDK DLL hash, source-receipt hash, session, view, generation,
SDK index, jitter match, reset flag, completion boundary and finite/color range.
The per-build `{current,history,rectified}-source.json` receipts retain exact
shader replacements and before/after hashes. Corresponding `*-build.log` files
record compilation. Six `*-edges.json` files hold signed profiles, coverage
masses and two-dimensional centroid decompositions.

All 120 checked scene-color/depth/motion guide readbacks are byte-identical to
retained ordinary stock or aged candidate inputs at the same source frame. The
old stock aged guide raws had already been removed during cleanup, so the aged
candidate's identical producer inputs were used explicitly for that comparison.
All 24 intermediates are finite, have SDK reset zero and match logged SDK jitter
at source frame minus two. Readbacks complete at drained context retirement.
No input guides were deleted during this wrap-up.

All six fixtures used the shared named GPU lease, portable Khronos validation,
synchronization validation, implicit-layer filtering, per-process legacy
extension-point mitigation and the owned-process foreground monitor. API and
synchronization checks passed. Five cases recorded no runtime foreground loss;
`rectified-aged` recorded one foreground-loss sample and reassertion. This is
preserved in its receipt. These are SR intermediate measurements, with no
generated-frame image acceptance inferred from that case.

Retain the six case directories under
`.local/godot/fsr2-stages-20261025/{current,history,rectified}-{ordinary,aged}`,
especially their four `sr-output-linear` and `fg-hudless-color` raw/metadata pairs,
capture manifests, validation/foreground receipts and logs. The compact master
manifest identifies matching retained baseline guides for any later
byte-verified deduplication. The six cases are diagnostics and must not be added
to the passing image matrix.

The PS harness parsed successfully; both analysis scripts ran on the saved and
new readbacks. All six captures and all three builds were terminal before the
user requested wrap-up. There are zero owned live handles, GPU leases or queued
captures/builds. No additional experiment was started afterward.

## Remaining work

The next causal experiment should separate the surface coverage admitted by the
current rectification box from the pre-existing history reprojection bias, while
retaining normal history storage and measuring both ordinary and aged cases.
That experiment has not been implemented or run. The current observations do
not justify disabling clipping, hardcoding a scene mask, changing a threshold,
permanent resets or claiming the original displacement fixed. Production
correction and unchanged end-to-end image acceptance remain outstanding.
