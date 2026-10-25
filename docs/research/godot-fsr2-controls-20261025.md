# Godot FSR2 reconstruction controls, 25 October 2026

The moving cube's FSR2 displacement remains unresolved. These controls found no
defensible production correction. No diagnostic shader, analytic scene geometry,
jitter override or history reset was added to the runtime. The existing image
gate remains 0.35 output pixels.

This extends the [Godot geometry and history investigation](godot-engine-integration.md).
The question was whether an incorrect input convention, motion selection or
history rejection causes the displacement already present in the SR source.
Single-variable SDK controls were chosen because the existing geometric oracle
already measures accurate original vectors and raster jitter. A change that only
passes one jitter/history phase is insufficient.

## Source and device identity

- Godot 4.7.2 source: `ed1daf0bf001b61586d9930840f2f1394092c079`.
- FidelityFX Vulkan SDK 1.1.4: `c6efa6bf7f2027b3ec94f28578bb5965eabb9e55`,
  FSR2 2.3.3. Godot's separate built-in FSR2 is 2.2.1.
- Standard editor SHA256:
  `E60D28D55689EF5DA3E28A98023032E21C6EB8C5264B5BF224EAF41AEB0034EF`.
- Native library SHA256 for the controls:
  `EE22B1A698E6DD906E1C294A0DC72805C37857543368F379DE2E1F54713978F3`.
  This library was preserved and copied explicitly into later isolated projects
  while unrelated provider work changed the shared package.
- Device: NVIDIA GeForce RTX 5080, Vulkan. Loaded validation-layer SHA256:
  `E86FA4F70676A120F00A32BEAC6979CEB3CAFCE70DE42FA9107C89F587AB206E`.

SDK sources, builds and per-project overrides are isolated under
`.local/godot/fsr2-diagnostic`. The vendor reference checkout remains unchanged.
The current SDK storage-format and WSI corrections were retained. One initial
capture used an older WSI1 staged source against the current WSI2 native owner
and omitted the portable validation-layer path. It failed at startup and is not
rendered-frame evidence. The corrected controls used the current WSI sources and
confirmed the loaded validation layer. The previously recorded copied-compiler
launch failure also occurred; the pinned original compiler location worked.

`.local/godot/fsr2-diagnostic/experiments.json` records individual SDK binary hashes,
engine/native hashes, input/reference locations, image failures and the jitter
dispatch receipts. It contains 24 completed diagnostic analyses, each with four
actual generated images and a loaded validation layer. These are diagnostic
results, including failures, not 24 accepted rendering cases. The history-weight
capture is separate and intentionally changes displayed RGB into diagnostics.

The final `decoded-controlled-reference` variants pair all 24 controls with
references using identical editor and native-library hashes. Fresh ordinary and
billboard references are `fsr2-controls-reference-box-object` and
`fsr2-controls-reference-billboard-object`; camera, combined and aged references
already used the preserved native library. These variants retain the same
numeric results and decisions. Earlier reports remain intact.

## Runtime data and measurement

The ordinary scene uses source frames 12 through 15, 426x240 scene color/depth
and normalized previous-minus-current UV motion, and a 640x360 output. The
independent midpoint/current references render at output resolution with 8x MSAA
and HDR resolve before tonemapping. The aged control captures source 40 through
43 with animation offset 28, preserving the original poses and velocities while
changing the history and jitter phase.

Readbacks belong to each capture's session, view zero and recorded resource
generation. The engine records SDK work in its own command buffer; the callback
leases and images survive GPU submission. Captures are flushed at drained context
retirement. The controls use the existing foreground GPU mutex and do not submit
an independent diagnostic queue. Source frame identity is not SDK history age:
the dispatch log matches source frame 12 to SDK index 10 after initial context
replacement in this fixture.

The analyzer keeps geometry, camera, finite-vector, raster-jitter and generated
image gates. Across the recorded controls, maximum object/floor vector p95 error
is below 0.001805 render pixels. Material-centroid errors below are output pixels.
They describe the actual generated image against the independent midpoint, not
presentation counters or inferred FPS.

## Reconstruction controls

| Control | Maximum ordinary error | Result |
| --- | ---: | --- |
| Stock SDK, refreshed positive-sign control | 0.5843 | Fails |
| Full FSR2 FP32 path | 0.5840 | Fails; rejects precision as the cause |
| Context maximum render extent equal to 426x240 | 0.5860 | Fails; rejects that allocation hypothesis |
| Remove nearest-depth motion/depth dilation | 0.4541 | Fails and changes advance into lag |
| Jitter-aligned integer motion lookup | 0.5803 | Fails |
| Jitter-aligned bilinear dilated motion lookup | 0.5020 | Fails |
| Add actual-pixel depth rejection to original rejection | 0.4045 | Fails |
| Combine bilinear lookup and actual-pixel rejection | 0.3433 | Ordinary passes; aged control fails |
| Godot built-in FSR2 2.2.1 at 426x240 | 0.5733 | Fails through the engine's own implementation |

The built-in control explicitly selects engine FSR2 after configuring the native
spatial branch, which preserves the render scale without executing native vendor
SR. Its raw depth confirms 426x240. The earlier SR Off attempt was forced to
640x360 by `plan_internal_size`; its successful native-resolution AA result is
not lower-resolution upscaling evidence. Both attempts remain recorded.

`tools/analyze-godot-fsr2-dilation.py` replays the pinned `FindNearestDepth` selection
order on captured inputs. This is a CPU source model, not a readback of an SDK
intermediate. On cyan cube samples, selecting a neighboring motion vector changes
the vector by up to 0.2600 output pixels, with p95 up to about 0.0562. Original
vectors remain geometrically accurate. Dilation deliberately assigns foreground
motion to some neighbors, so a selection difference alone is not proof of an
incorrect algorithm. The passing billboard has lower p95 differences but also
large isolated changes where a nearer occluder selects zero motion.

## The nearly passing candidate is rejected

The combined candidate samples the dilated vector field bilinearly at the
jitter-aligned location and takes the maximum of existing depth rejection and
rejection computed from actual current-pixel depth/motion. It preserves the
original surface heuristic. Ordinary midpoint errors are
`0.2445, 0.2868, 0.3433, 0.2256`. Camera-only, combined and billboard controls
also pass, with maxima `0.2361`, `0.2540` and `0.1733`.

However, the aged same-pose errors are
`0.3075, 0.3273, 0.3701, 0.2461`. Frame 42 still fails. Moving the added rejection
outside `EvaluateSurface` worsens that control to
`0.3504, 0.3639, 0.3872, 0.2401`. Neither variant is a production fix.
Exact candidate replacements are retained in
`.local/godot/fsr2-diagnostic/current-surface-reprojection-source.json`; its SDK
binary SHA256 is `CCFFD6B87DEF6BF579699ACC338EB0D2FB905081B75FB7E97F8699A23C7B7AF6`.

An additional shader readback encodes depth rejection, red-channel history
fraction and accumulation mask into displayed RGB after normal internal-history
storage. Original scene-input raw bytes match the ordinary candidate on all four
frames. The eroded object core retains median history fractions of 0.699 to
0.756, with all sampled core pixels above 0.5; the sampled floor has median 0.780
to 0.810 and zero depth rejection. Therefore the improvement is not explained by
silently resetting the object or the whole frame. This diagnostic does not make
the aged failure acceptable. See `history-weights.json` beside the experiment
manifest. Its displayed debug images are not quality comparisons.

## Exact-motion oracles do not justify a new engine pass

A scene-specific shader oracle ray-intersects the known cube at each output
pixel and derives motion from the fixture's actual current/previous box poses.
This hardcoded geometry is a diagnostic only. An initial SDK-index-plus-one
version appeared to pass but used the wrong object silhouette; dispatch logging
exposed the source/index relation and that result is rejected.

The corrected accumulation-only oracle still fails two frames. A coherent
oracle then enables `FFX_FSR2_ENABLE_DISPLAY_RESOLUTION_MOTION_VECTORS` and uses
the analytic field for all SDK motion consumers, including depth reconstruction.
It retains stock color/history sampling and gives
`0.3098, 0.4064, 0.2526, 0.4664`. Additional actual-surface depth rejection still
leaves two failures. Thus exact output-pixel motion alone is not established as
the correction. No extra engine geometry pass or C ABI change follows from this
control. A conditional design, explicitly marked unproven, is retained in
`output-motion-design.json` under the diagnostic directory.

## Jitter sign is not the missing correction

FSR2's `ComputeUpsampledColorAndWeight` places an input sample at
`integer_pixel + 0.5 - Jitter()`. Its current-mask lookup uses
`output_uv + Jitter() / render_size`. The independently fitted Godot raster
moves samples by the supplied positive jitter, matching both equations.
This checks the consumer's equations as well as the producer metadata. The
[upstream contract](https://github.com/GPUOpen-Effects/FidelityFX-FSR2#providing-motion-vectors)
also separates vector values without jitter from the jittered scene inputs.

All four API dispatch sign pairs were tested with stock shaders, both ordinary
and aged same-pose inputs. Actual SDK jitter constants were logged and checked
against every captured raster jitter/sign pair. The raster metadata and original
image gates were unchanged.

| SDK jitter X/Y signs | Maximum ordinary error | Maximum aged error |
| --- | ---: | ---: |
| + / + | 0.5843 | 0.6106 |
| - / + | 0.5550 | 0.4745 |
| + / - | 0.6681 | 0.6562 |
| - / - | 0.5969 | 0.5423 |

Every sign-pair case fails. A sign override is rejected, even where it happens
to reduce one centroid error.

## Remaining limit and checks

The evidence supports a bounded FSR2 coverage/history-reconstruction limitation
in this scene. It does not prove a general vendor defect or identify one safe
production change. Both the original shader and the best candidate retain image
failures. A temporal reset, a tuned blend, a different capture phase or a larger
gate would conceal the unresolved result.

These controls cover the standard Vulkan editor and this synthetic cube/floor
scene. They do not validate a new .NET, exported-player, DX12, arbitrary-material,
skinning or transparency correction, and establish no performance or latency
claim. The SDK diagnostic builds used MSVC Release. The replay tool passed
`py_compile` and ran against the preserved cube and billboard captures. No native
or production SDK source change is delivered by this investigation.
