# Fallout 4 reduced-quality capture audit

The first visibly incorrect captured colour resource is **native pre-SR Main**,
`native_color`. Nested scaled scene rectangles are already present before this
frame's crop, SR backend evaluation and reinsertion. The SR source is an exact
crop of that damaged image. This locates the earliest observed failure boundary;
it does not identify an earlier native draw or exclude previous-frame feedback
from prior integration work. No speculative fix follows from the status result0.

## Evidence and method

Read-only capture:
`D:/SteamLibrary/steamapps/common/Fallout 4/ReScaleFrame/captures/website/scene-20261116-155347-123-102742796-64716-1`.
Manifest reports complete, session1/source5737/native6211/scope3156,
matched_present=true, Present0 and SR0. Serialized quality1 uses render LTRB
[0,0,1707,1067], output[0,0,2560,1600], backend1, reset0 and jitter pixels
[0.125,-0.277785718]. The coordinator associates this run with committed07afb10,
Entry463B1EED47090204DBB301B0332CA5CB9E5938907A67274994D59980E5AED0C7
and Runtime8570CCC20A05FCDE6C533E8BD3748C827CF6D1ED842062C1E90596E5BF2375B3.
Its prior full gate223/14 and hidden reset-sequence pass are coordinator evidence,
not a new test performed by this audit. The coordinator reports the controller
and game exited and20-row restoration succeeded; this worker made no process query.

The ignored analyzer `audit-reduced-quality-20261116.py` extends the earlier
metadata-aware decoding method for differing allocation/crop dimensions and
copied depth descriptors. Output `reduced-quality-capture-audit-20261116.json`
is bounded69KB, with every file hash and per-channel finite/min/max/quantiles.
All raw row pitches, lengths, dimensions, formats and subresource0 declarations
were checked. R11G11B10 is unsigned float, D24 is low24-bit UNORM and motion is
half/float as declared. PNGs were inspected as retained images, not live UI.

| Captured file | SHA256 |
| --- | --- |
| manifest.json | 8e9ab09b2296eb69268aa892c6fcdf82e38bce96bcea3ba0946849f76f55b114 |
| engine_pass.json | fe895fca135d17f70adeeddb4eb668631ffbb62ff9c9435d8daa434453c65814 |
| native_color.bin | 394d1397dbbab170943481d42c5b20775f7d849e498976ab1fcf3a4cf5570f89 |
| sr_source_jittered.bin | e2f9043efb1a270d4c3234c6b3ea47588232baad428c21ca332a2b6e395663c7 |
| sr_backend_output.bin | 0d00304abce5660d7bfe5783d67579bc31f407921471a8bd321119a8f024c9c2 |
| sr_reinserted.bin | f88b4c0722bb2d273cd0019d73a0c01941ccca718511cc816e5a6466839b82c4 |
| final_color.bin | 9c2d60a4b84b7f8f97d3b92d123cc821ab939c4e68aa82a4cd638d7c7722f6f4 |

## First bad stage and colour propagation

Native Main's full2560x1600 image already contains a large top-left scaled view,
a second nested view and overlapping scene details. Its cropped1707x1067 area
still contains the inner rectangular boundary near row712; the full allocation
also has a boundary near1067. Mean adjacent-row absolute RGB differences are
0.06641 at712 and0.05641 at1067 versus about0.0055..0.0074 on nearby sampled
rows. These numbers corroborate visible seams; a row discontinuity alone is
not proof of a specific scaling producer.

| Stage | Actual finding |
| --- | --- |
| Native Main before SR | Nested scene rectangles already visible. |
| SR jittered source / SR colour | All5,464,107 RGB components equal native Main[0:1067,0:1707] exactly; no offset, extra resize or conversion error in the captured crop. |
| Backend output / SR output | Full2560x1600 and exactly equal to each other; the damaged source's rectangular content is scaled into the result. This frame does not show the backend introducing the first rectangle. |
| Reinserted Main | Every RGB component equals unsigned-clamped floor quantization of backend output to R11G11B10. Maximum difference0.060546875, at most0.96875 packed-format step after unsigned clamp. Two negative half-float backend components clamp to0. No extra spatial rectangle appears at this conversion. |
| Final Present colour | The same large rectangle/ghost pattern survives native imagespace/UI. HDR and final RGBA8 are different colour domains, so equality is not an expected invariant. |

The capture timing is source-supported: `d3d11_host.cpp::capture_inputs` records
`native_color` from `pass.color_input` before SR processing. The record function
in `input_dump.cpp` immediately invokes raw/PNG texture dumps; it does not merely
retain a mutable native pointer for final-Present readback. This supports the
pre-SR failure boundary rather than assuming the label proves its timing.
It still cannot separate earlier draws in this frame from retained content or
feedback introduced in preceding frames.

## Exact copied depth-domain replay

Unlike the first native-scale capture, `engine_pass.json` contains the complete
ABI23 depth domains: two ranges, endpoint flags, producer viewport and row-major
source_clip_to_target_clip matrices. Both viewports are exactly[0,0,1707,1067].
The CPU replay loads each original D24 sample at the copied render crop, converts
the UNORM load to float32, classifies its domain, constructs source NDC at
allocation pixel centers x+0.5/y+0.5 relative to that viewport, undoes viewport
depth scaling, multiplies the copied row-vector matrix and clamps Z/W.
It uses copied coefficients with double evaluation to make the arithmetic
comparison independent of GPU operation/FMA rounding, rather than claiming a
bit-exact GPU replay.

All1,821,369 selected samples are world domain, with no near or clear samples
and no nonpositive W. Expected versus captured `sr_scene_depth` maximum absolute
error is7.31848908e-8, mean3.35462555e-8. SR scene depth, SR depth and FG depth
are identical. Normalized depth spans0.8669074774..0.9756310582 and is finite.
This corroborates the supplied world-depth transformation at the copied sample
centers; it does not validate first-person virtual-Z or spatial geometry.
The full original native depth spans0.8682383220..1, with the reduced geometry
footprint visually bounded at1707x1067 and clear outside. No first-person hands
or near-domain classification occurs in this capture.

## Original and reconstructed auxiliaries

The source originals and reconstructed full-output images were compared using
the actual existing auxiliary nearest shader: for each output pixel center,
map output UV into the source rectangle, subtract copied jitter, floor and clamp
to valid source texels. Actual1707x1067 source and2560x1600 output dimensions
were used, not a rounded quality ratio. All captured reconstruction results match:

| Reconstructed auxiliary | Exact component matches |
| --- | ---: |
| native_depth_expanded | 4,096,000 / 4,096,000 depth values |
| native_motion_expanded | 8,192,000 / 8,192,000 |
| native_pipboy_expanded | 16,384,000 / 16,384,000 |
| native_vats_expanded | 16,384,000 / 16,384,000 |
| native_refraction_expanded | 12,288,000 / 12,288,000 |

The logical PipBoy-labelled original has a clean reduced scene-shaped numeric
image against a constant outside region; its expansion fills the output without
the nested Main rectangles. It must not be interpreted as display colour from
its label or false-colour preview. VATS and refraction are all zero here, so
their exact comparisons exercise numeric zero handling, not active effects.
The depth comparison covers depth values; it does not independently audit each
stencil bit or prove those reconstructed resources are correct for every later
native consumer.

Native cropped motion equals decoded R32G32 at all3,642,738 components. Resolved
pixel motion differs from the declared scale prediction by at most one half
subnormal step5.96046448e-8; SR and FG resolved motion are identical. This is
consistent with output half-float quantization. Stationary tiny vectors cannot
validate moving object/camera, skinned, alpha or first-person coverage. Reset0
is the actual serialized state for this source, not evidence that temporal
history was clean or that a reset defect caused the rectangles.

## Next proof boundary and limits

The concrete failure is a damaged native pre-SR Main colour image. The measured
crop, world-depth normalization and captured auxiliary expansion do not explain
its creation. Plausible remaining producer boundaries include mixed raster/UV
scales during native Main composition, a pre-SR effect reading retained full
allocation contents, or temporal/prior-frame feedback after full-output
reinsertion. These are hypotheses, not assigned causes. This frame contains no
earlier Main intermediate, draw-by-draw binding history or previous-frame Main
sequence to distinguish them. Native output/resolution scope and the last
pre-SR Main writers are the next owner's investigation boundary; a new backend
or shared GPU-policy change is not supported by this evidence.

No production change, build, GPU run, game/process/UI action, attachment,
deployment or game-target write occurred. The analyzer and bounded JSON are
ignored evidence; the new note is the only tracked change for this task.
Native SR result0 and matched Present remain real receipts, while quality1
visual acceptance is explicitly refused by this captured defect. Full shader,
moving temporal, near-domain and active auxiliary-effect validation remain open.

## Wrapper-absent stationary control, source19050/native19505

Coordinator report `native-raster-20261116-161308-012`, PID71412, temporarily
quarantined only the game-local ENB d3d11.dll wrapper for this control. RSF
Runtime8570CCC2... and Entry463B1EED... are unchanged from the corrupted trial
above. New capture `scene-20261116-161912-234-104267921-71412-1` is complete,
matched Present0/SR0, session1/source19050/native19505/scope13731. Quality1,
render1707x1067 and output2560x1600 are unchanged; actual reset is0. The exact
copied domains remain available. The analyzer now accepts explicit capture and
output arguments, preserving the earlier report; new bounded results are
`enb-control-quality-capture-audit-20261116.json`, with a paired summary in
`enb-quality-paired-summary-20261116.json`.

Inspection of the active native Main crop, SR source, backend, reinserted and
final previews shows a clean stationary shower scene without the earlier nested
rectangles or ghosted duplicate fixtures. Full native Main allocation still
contains retained scene pixels outside1707x1067, producing an allocation-boundary
seam; those pixels are outside the declared source crop and do not appear as a
second view inside the crop or final output. It would be incorrect to classify
that inactive allocation alone as the earlier defect.

| Check | ENB-on source5737 | Wrapper absent source19050 |
| --- | --- | --- |
| Active native Main crop | Nested rectangles and doubled details | No such rectangles observed |
| Native crop versus SR source RGB | All5,464,107 exact | All5,464,107 exact |
| Descriptor replay maximum depth error | 7.31849e-8 | 9.77319e-8 |
| Selected near/clear samples | 0/0 | 0/0 |
| Selected world samples | 1,821,369 | 1,821,369 |
| All five auxiliary nearest replays | Exact | Exact |
| Backend-to-reinserted unsigned R11 floor quantization | All12,288,000 exact | All12,288,000 exact |
| Actual serialized reset | 0 | 0 |

Normalized depth in the control spans0.8755305409..0.9771202207, all finite and
forward-W. SR/FG normalized depth and resolved motion agree as in the first
audit. Decoded motion equals native cropped values exactly; resolved pixel
motion differs from the declared scale prediction by at most5.96046448e-8,
consistent with half-float storage. No first-person hands or independent moving
object/camera sample was captured, so neither near-depth nor full motion audit
has advanced. Maximum backend/reinsert RGB difference0.12109375 remains below
one destination-format step, with no negative backend components in this control.

Control manifest SHA256 is
`2e42274cb88ca3b7e5940ed0f1a9f784b253b156e551427065568b3748a5d933`;
engine_pass `cbf956e1c4596628ccc95257afa0a839be72d74a83fa203f41e7c39f3a2fac82`;
native Main `bb9f8cc9b1220c559fa1edef432bf61f5c59dea2470ba64288f0d79229a452ba`;
SR source `a474f21ef628869464f17fc8794909f9ca063029424f767f8b46a07632dde669`;
backend `1b04ae787d0f83f615d6659d0caf512e705e22afe46d77a0894fd184c0202e41`;
reinserted `7f2cdd55c8594ce5f5c2a1d822f4a03fa9b5214d9634a2a60abd71a74405e3ff`;
final `6cad94e8ded144cd1255d460bf1a1676c26bf0ebc8c1dfd19e8437b5ec797597`.

This is evidence associating wrapper presence with the observed reduced-Main
corruption under these trials, not proof that ENB alone is defective. The control
menu named Sanctuary Hills level1/8min versus previous ExitSave/6min; the root
observed the same shower setting, but the save bytes are not identical. Copied
camera rotations and jitter phases also differ, although positions are close.
The runs do not form a pixel-aligned before/after reference, and temporal/history,
mod interaction or native compositing conditions remain possible confounders.
No ENB instruction, draw or resource producer causing the corruption is identified.
The prior ENB-on Quality1 visual rejection remains; this control supports only
the observed clean stationary wrapper-absent scene, not reduced coexistence,
moving visual quality, FG or latency acceptance.

The authoritative root `post-close-disposition.json` in report161308-012 records
ordinary AltF4 within the controller close handoff, access violation4CB273 read
null+50, exitC0000005, normal_exit=false and20 controller restorations. The
retained observer trace confirms that native AV and termination status. Root
reports the prior ENB-on PID64716 also used ordinary AltF4, reproduced the known
baseline close fault and restored20/20. Neither was native Quit or normal exit.
The separate `enb-control-20261116-161307-318/enb-control.json` records restored=true
for the extra wrapper quarantine and exact SHA
`AC194BD525BB1C3CF363448F1D2B9F96DD859424E6FEAC00695B152B4B951388`.
This is additional ENB restoration, distinct from the controller's20-row manifest.
No worker build, GPU/game/process/UI action or target write was performed.
