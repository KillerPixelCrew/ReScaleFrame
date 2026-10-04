# AC7 motion and depth inputs, 4 October

The question was what limits the motion and depth AC7 hands its upscalers after the native SR
path settled, and which of the remaining gaps engine patches, shader work or 4.27 systems can
close. The user also asked for the translucency hint buffers (`BiasCurrentColorHint`,
`ReactiveMaskHint`, `ColorBeforeTransparency`, `TransparencyLayer`) to be filled.

The approach was measurement first. Existing F9 captures say what is missing today. Ghidra and
the matching 4.18.3 source map every place that decides velocity. The 4.27.2 source says what
Epic changed. RenoDX and Luma show what comparable mods do. TrueSky's shipped effects and DLL
say what cloud data exists. The implementation then covers the gaps that the measurements rank
highest.

Executable: `Ace7Game.exe` SHA-256 `c7da97f5…8f4f` (build 9855922), UE `4.18.3-release`
`0a14a8d5`. TrueSky: `TrueSkyPluginRender_MT.dll` SHA-256
`c6c58ea3a355ed345888b1528e84aa753c6fc69db9c8fb8789e729c7651004d0`. Numbers below are from the
listed captures. None of the changes has run in the game yet.

## What the captures show

The seven captures from 3 and 4 October were analysed with `tools/analyze-ac7-motion-capture.py`
and new scripts:

| Capture | Scene |
| --- | --- |
| `motion-20261004-124144-103792-1` | Hangar |
| `motion-20261004-082226-100100-2`, `motion-20261004-082205-100100-1`, `motion-20261004-075731-104792-3` | Desert flight |
| `motion-20261004-075643-104792-2` | Kill-cam |
| `motion-20261004-075626-104792-1` | Pause |
| `motion-20261003-222356-32284-1` | Above clouds, engine records only |

**Only the player's aircraft writes velocity in flight.**
- 10 to 11 primitives pass each interval, all within 270 units: the body, one skinned part with
  previous bones, four attached missiles and two further attachments.
- Written pixels cover 9.9 to 11.8% of the 533x300 target, exactly the aircraft silhouette.
- The submitted dense field reproduces both the written vectors and depth reprojection with
  0.000 px median error. The old concern about Streamline treating a valid zero as unwritten
  does not arise, because we submit a complete field.
- The 1 October hangar cutscene matches engine vectors to our depth reprojection within 0.01 px.
  That independently confirms the camera convention.

**The size gate costs little.** With the recorded `MotionBlurPerObjectSize` 0.5 and LOD factor
0.70, it rejects primitives under about 3 render pixels of radius:
- 97 to 159 unique primitives per flight fail it, with a median projected radius of 0.14 to
  0.30 px.
- 16 to 25 of those move: groups of four with identical radius, probably enemy flights.
- The motion they lose is at most about 0.15 render px per frame (inference).
- The hangar and menu scenes record `MotionBlurPerObjectSize` 0, so the gate never fires there.

**Translucent effects, the kill-cam and clouds get camera motion only.**
- All translucent primitives are rejected before drawing, including 53 to 95 zero-bounds systems
  per flight.
- The kill-cam writes no velocity at all.
- Sky and clouds sit at device depth exactly 0. Their reprojection is rotation only, so cloud
  parallax is missing.
- The custom-projection branch (primitive row 17.y) has bit-identical current and previous rows
  116 to 119 and 120 to 123 in the two captured view buffers. Any draw taking it would write zero
  motion (inference: the primitive buffer was never captured).

**Recorder defect.** `f*_motion.json` and `f*_motion_decoded.json` report `fraction_unwritten`
0 when 85 to 100% of pixels are unwritten. Their summaries recognise neither the clear value nor
the -1000 sentinel. The raw images are correct.

## The 4.18 velocity gates in the binary

Every gate is now located, matched against `VelocityRendering.cpp`, `PrimitiveSceneInfo.cpp`,
`SceneCore.cpp` and `SceneVisibility.cpp`, and checked in the live Ghidra image and
`.local/observe/Ace7Game.exe.dump`. RVAs and bytes are in the
[hook map](ue418-hook-map.md#velocity-gates-mapped-4-october). The findings that change earlier
conclusions:

- **The 7 September translucent-velocity result needs a retest.** The gate patch at `0x11823de`
  cannot reach translucent batches on its own. In stock code they are rejected earlier, in two
  places:
  - by `GetHasOpaqueOrMaskedMaterial` at `0x1184e4c`, a flag `FMeshElementCollector::AddMesh`
    sets at `0x19ae3cc` from `blend < 2`;
  - by the opaque-and-main-pass relevance test in `ShouldRenderVelocity` at `0x10fdc7c` and
    `0x10fdc81`.

  The patched and unpatched briefing dumps were minutes apart, so something else probably
  changed between them. The result is not withdrawn without a run, but it no longer stands as
  proof that the patch works.
- AC7's view relevance has one more bit than stock: `bRenderInMainPass` is bit 14, not 13.
- `MotionBlurPerObjectSize` is a post-process setting at view+0x1174. That is
  `FinalPostProcessSettings`, based at view+0xC80, plus 0x4F4. It has no console variable.
- Previous transforms are registered for every movable primitive, translucent ones included.
  Stationary meshes rebuild their proxy on movement and never get history.
- If temporal AA is off with FXAA and motion blur also off, the velocity pass is skipped
  (`ShouldRenderVelocities`, `0x118a480`).

## What 4.27 offers

Very little that transfers. The encoding, the vertex-animation history with previous-frame time
and material parameters, skeletal previous bones, Cascade sprite motion and TAA reprojection are
the same in 4.18.3 and 4.27.2.

4.27's changes are of three kinds:
- **Selection:** `r.VertexDeformationOutputsVelocity` and `r.BasePassForceOutputsVelocity`.
- **Where history lives:** `PreviousLocalToWorld` in the primitive uniform buffer.
- **Opt-ins:** the translucent velocity pass and the depth channel.

None needs a backport to get its effect here:
- Vertex-deformation velocity is the `HasVelocity` always-velocity jump at `0x118384c`.
- Primitive-buffer history and the translucent pass need the 4.22 mesh-pass architecture and
  cooked permutations.
- Sprite particles have no previous position in either version.

## RenoDX and Luma

Updated to RenoDX `da608c4` and Luma `5d7ee54`.

- RenoDX has no motion or depth work.
- Luma's Unreal module decodes 4.18 velocity as we do.
- Luma stores motion in R32G32_FLOAT. Its comment says R16G16 float showed segmented thin lines
  under linear camera motion. Our decode and resolve wrote R16G16_FLOAT, which steps by 5e-4
  NDC near one and by a quarter pixel or more above 256 px of displacement. That is coarser than
  the UNORM16 source's 0.06 px.
- Luma dilates by the nearest of four diagonal depths before reprojecting.
- Luma's Metaphor module patches DXBC to add a velocity render target to the game's own pixel
  shaders, and builds a bias mask from a particle layer's alpha.

No code was copied.

## TrueSky clouds

There is no cloud velocity in this build.
- The checkerboard raytrace (CS `0xc6aa9f31`) writes a near/far/depth texture whose `.z` is the
  radial km to the first point where far transmittance drops below 0.75.
- Sentinels: 150 means no hit, 100 means the ray missed the layer.
- The composite (PS `0x83524e47`, 7512 bytes) reads it at t2 but never uses `.z`. The far image
  at t1 carries transmittance in alpha.
- `HdrConstants` (b12) carries `tanHalfFov` at 448, `depthToLinFadeDistParams` (0.1, 150000, 0,
  0) at 464, `maxFadeDistanceKm` at 580 and `maxCloudDistanceKm` at 636.
- Across two captures the cloud volume origin stayed fixed. Only the edge noise scrolled, which
  no vector can describe.

Cloud screen motion is therefore camera reprojection at the cloud's own distance.

## Implemented

All of it builds with MSVC Release and passes `eng/verify.ps1 -Configuration Release -VS2026`
(47 tests, vendor-environment tests skipped). Two new WARP tests cover the GPU paths. It is
deployed for the next run and has no game result.

**Motion precision.** Corrected after the first run, see below: only the decode stays
R32G32_FLOAT; the resolved motion is R16G16_FLOAT. Originally the decoded and resolved motion
were both R32G32_FLOAT. The SR bridge and the
DLSS-G transfer follow the source format rather than assuming R16G16_FLOAT. The translucency
layer's zero motion follows. Unity's D3D12 path is unchanged.

**Translucency hints.**

*The scope.* `RenderTranslucencyPass` (`0x1168d40`) is already hooked. `Render` calls it with
pass 0, 1 and then 4, or with pass 2 alone when separate translucency is off (call sites
`0xef7ac9`/`0xef7ada`/`0xef7af3`). The hook now opens a new `RSF_GAME_RENDER_TRANSLUCENCY` scope
around passes 0, 1 and 2 of a single primary view. It leases scene colour, chosen as
`SceneColor[]` at +0x30/+0x38/+0x40 by the same shading-path and alpha test as
`AllocateSceneColor` (`0x1095010`). For pass 1 it also leases the layer at scene targets +0x1b0
(`AC7_FSceneRenderTargets_GetSeparateTranslucency`, `0x109f4d0`, named in Ghidra). TrueSky draws
in the post-opaque extension just before, so scene colour at the first pass's begin holds opaque
geometry plus sky and clouds.

*The runtime.* It snapshots that colour, rect-local. At the end of the pass it compares scene
colour with the snapshot after a max-channel Reinhard; at the end of the layer pass it adds the
layer as the game composites it (`scene * a + rgb`, cleared to black with alpha 1). It produces:
- reactive: change capped at 0.9, or the layer's opacity;
- coverage: change ×10, saturated, or the opacity;
- bias: coverage ≥ 0.5.

The masks reach the SR pass only for the same family, view, native frame and rectangle.

*Where each one goes.*

| Backend | Receives |
| --- | --- |
| DLSS | Coverage as `TransparencyHint` and bias as `BiasCurrentColorHint`; the snapshot as `ColorBeforeTransparency` and the layer as `TransparencyLayer`; on the D3D11 path and through the DLSS-G shared surfaces |
| FSR | Reactive as `reactive`, coverage as `transparencyAndComposition` |
| XeSS | Reactive as the responsive pixel mask. It is now created with `XESS_INIT_FLAG_RESPONSIVE_PIXEL_MASK` and given a zeroed mask on frames without one |

*Expected effect.* The Streamline 2.14.1 DLSS plugin passes only the transparency, bias, particle
and animated-texture hints to NGX. The DLSS 310.6 guide says bias "has not been introduced for
use with latest models" and only preset F reads it. The colour before transparency and the layer
are read only by `sl.dlss_d` (Ray Reconstruction). So a visible change from the masks is
expected on FSR and XeSS, and not on DLSS SR with current presets. In our route the layer is also
already composited into the SR input, while RR expects it to be left out.

**Cloud motion.**
- *The relay.* TrueSky's composite_tile draw is the render-platform `DrawIndirect` call
  `ff 90 30 01 00 00` at DLL RVA `0xabc6b`. A guarded six-byte `call [rip+rel32]` relay forwards
  it with its three register arguments.
- *The kernel.* While the composite's inputs are still bound, before TrueSky's Unapply, it
  converts `.z` to scene device depth with TrueSky's own constants:
  `d = x / (y * planar)`, where `planar = km / maxFade / sqrt(1 + |ndc * tanHalfFov|^2)`.
  - It qualifies only pixels with `.z` below both maxima, `.w > 0` and opacity ≥ 0.5.
  - The cloud distance is clamped to at least 300 m.
  - The kernel restores compute state.
- *The handover.* The plugin hands the texture over as `RSF_GAME_RENDER_CLOUD_DEPTH`. The
  runtime gives it to the next scene-colour translucency pass with the same rectangle and
  otherwise discards it.
- *The resolve.* Pixels without object velocity now reproject at the nearer of cloud and scene
  depth. The depth given to backends stays scene depth.
- *Prerequisite.* The relay needs the device that TrueSky's depth producer relay recorded. If
  that relay is refused, cloud motion is unavailable.

**Tests.**
- `motion_resolve` now reads float output and checks the layer case: a sky pixel moves, a pixel
  without a layer stays, and a written pixel ignores the layer.
- `native_translucency` covers the masks, the snapshot, the layer copy, refusal for another
  frame, and cloud adoption and expiry.

## Not done, and why

- **Size-gate bypass (`0x10fdd0c`).** The measured loss is sub-pixel.
- **Always rendering velocity.** `ShouldRenderVelocities` `0x118a4e3`/`0x118a4ec` and the
  `HasVelocity` always-velocity jump `0x118384c` are located. They are the cheap 4.27-equivalent
  experiments for animated movable meshes and the kill-cam. They are not applied, so that this
  run isolates the hint and cloud changes.
- **Depth-aware dilation.** FSR already dilates internally. It needs a per-backend A/B.
- **A velocity sidecar by DXBC patching.** Only worth it once a measured class of draws needs it.
- **The recorder's unwritten-fraction summary.** Still to fix.

## Correction after the first game run, 14:27

The user reported strong shimmer and jitter in the hangar and in flight, no visible upscaling and
no frame generation. Captures `motion-20261004-142815-5632-1` and `motion-20261004-142843-5632-2`.

**What the log showed.**
- Every DLSS evaluation failed: -8 on the first frame, then -9 (`RSF_DLSS_ERROR_FEATURE_FAILED`)
  until the pipeline stopped after 120 frames.
- No Streamline error accompanied the failures, which put the refusal on our side of the call.
- The screen showed the native spatial fallback of the jittered render, and frame generation never
  received a completed SR frame.

**The cause.**
- DLSS SR now runs on the DLSS-G D3D12 host, so its inputs cross from D3D11 in shared textures.
- `R32G32_FLOAT` is not a D3D11 shareable format. A probe on this RTX 4070 Laptop reported
  `D3D11_FORMAT_SUPPORT2_SHAREABLE` = 0, and `CreateTexture2D` with the shared NT-handle flags
  returned `E_INVALIDARG`. `R16G16_FLOAT`, `R32_FLOAT`, `R16G16B16A16_FLOAT` and
  `R32G32B32A32_FLOAT` all succeed.
- The resolved motion's shared surface could not be created, so the transfer refused every frame.
- The FSR/XeSS bridge shares the same way.
- An earlier `slSetConstants failed` run at 13:03 predates these builds and is a separate issue.

**The fix.**
- The resolved motion is `R16G16_FLOAT` again. The precision table above no longer applies to
  what the backend receives.
- The `R32G32_FLOAT` decode stays, because only the D3D11 resolve reads it.
- The layer's zero motion is back to `R16G16_FLOAT`.

**The test that would have caught it.** `rsf_d3d11_present_bridge_test <Streamline dir> 0x10de`
drives this shared path against the real runtime. It now also passes the four hint textures. It
passes with the installed Streamline 2.14.1 and NGX 310.9.1, which shows the hint tags are accepted
on the shared host. Redeployed at 14:33.

## The newest UE4, and the material mip bias it carries

**Which source is newest.** Epic published no UE4 release after `4.27.2-release` (`d94b38ae`,
December 2021). The `4.27` branch tip (`3abfe77d`) adds only localisation and branch snapshots;
renderer and shader files are unchanged. The `4.27-plus` branch is still maintained: 129 commits
after 4.27.2, tip `41b2c549` from 15 September 2026. It was fetched into the 4.27.2 reference
checkout as `origin/4.27-plus`. Its rendering changes are:
- Bink drawing through the RHI, and D3D12 resource fixes;
- mobile screenshot-mask fixes;
- a moved compile error for velocity-writing translucent materials;
- three TAA jitter variables: `r.TemporalAAScaleSamples`, which can disable the existing
  upsampling sample-count scaling, and X/Y jitter inversion.

None changes velocity or depth.

**What we already had.** The sample-count scaling that 4.27-plus makes optional already exists in
our native route: phases are `8 * output pixels / render pixels`, set at the view-state boundary.

**The port.** One 4.19+ upscaling system had not been ported: the view's material texture mip
bias.
- 4.18 has no `MaterialTextureMipBias`. AC7 materials therefore sample mips chosen for the render
  resolution, while the upscaler rebuilds output-resolution detail.
- 4.27 sets the bias to `max(log2(fraction) - 0.3, -2)` under temporal upscaling
  (`SceneVisibility.cpp:3244-3245`, `r.ViewTextureMipBias.Offset`/`.Min`). It applies it through
  two biased shared samplers in the view uniform buffer (`SceneRendering.cpp:1460-1500`).
- Only materials cooked for shared samplers read those samplers. AC7's 4.18 materials bind
  per-texture samplers, so the uniform-buffer route cannot carry over.
- The same bias is applied at the sampler level instead:
  - The frame tap hooks `PSSetSamplers` (slot 10) and keeps a shadow of the samplers the game
    requested.
  - While a bias is set, it binds clones with the bias added. Only samplers that blend between
    mips and have a mip range qualify; comparison, min/max and point-mip samplers are left alone.
  - Turning the bias on or off rebinds all 16 slots, because Unreal's state cache skips samplers
    it believes are bound.
- The bias is active only inside the primary view's material passes:
  - `RenderBasePass`, RVA `0xebf050`, named in Ghidra. It was found as the call between the
    `BasePass` phase event and its two `SetCurrentStat` calls (`DeferredShadingRenderer.cpp:951-955`).
    The hook queues a `RSF_GAME_RENDER_MATERIALS` scope around it.
  - The translucency scopes.
- Shadows, the depth prepass, deferred lighting, reflection captures and post-processing stay
  unbiased, matching 4.27's restriction to material samplers. Prepass coverage is unbiased, so
  masked materials keep their depth-equal coverage.

**Status.** The frame-tap test checks the hook on a hardware D3D11 device. Built, verified and
deployed. The visual effect, cost and any moiré are not game-tested.

## Acceptance for the next run

Look for these log lines:
- `native translucency: scene colour leased around passes 0/1/2`
- `translucency hints: opaque-only colour…`
- `native TrueSky cloud motion: composite_tile draw relay installed`
- `…cloud distance converted to scene device depth`
- `cloud motion: unwritten pixels reproject at the nearer of cloud and scene depth`

Compare texture sharpness on terrain, runway and aircraft liveries against the previous build at
reduced render scale, and watch for new shimmer, which would show the bias is too strong.

Then compare:
- cloud edges and parallax in flight near clouds, with the camera panning and translating;
- smoke and contrail ghosting with FSR and XeSS against the previous build.

Any refusal line names its site. An F9 capture with backend samples can confirm mask contents and
cloud depth against the raw TrueSky inputs.

## DLSS-only hangar shimmer, 14:40

The user reports visible shimmer with DLSS that predates today's changes. It appears in the
hangar only; flight looks fine, and FSR and XeSS are stable there. Measurements:
- Recording `Bildschirmaufnahme 2026-10-04 143800.mp4`, consecutive frames with a static camera
  (best global shift 0,0): the frame-to-frame change sits on geometry edges.
- Capture `motion-20261004-144104-39044-1`, seven consecutive route dumps per sample:
  - DLSS output keeps a second temporal difference of 1.15 to 1.20 times its first, against 1.5
    for the jittered input. It removes only about a quarter of the input's oscillation.
  - Sparse samples (f000/f030/f059) put the post-DLSS colour correction at about 7% of the output
    variation.
  - Warping output frame t by the submitted motion confirms the sign: +mv beats -mv on all pairs.
    The best scale was about 8x, but the dumps stall the game while game time advances. The scale
    therefore cannot be judged from these frames, and stable FSR/XeSS on the same vectors argue
    against a motion error.
  - Exposure is a constant 5.0 (scene median about 0.1 after exposure).
  - The far plane reaches DLSS as the 1e7 default.
- The NVIDIA `integrate-dlss-sr` checklist was walked: `motionVectorsJittered` defaults to false,
  matching our unjittered vectors.

The DLSS-only colour correction (colour_fidelity) is now off unless `RSF_DLSS_COLOUR_CORRECTION=1`.
This is an A/B, not a claimed fix. The cause of the remaining DLSS-only hangar instability is open.

**Result and follow-up, 14:55.**
- **Shimmer fixed.** With the correction off, the user reports the hangar shimmer gone.
- **Banding back.** The dark-area banding at Performance and Ultra Performance returned, which is
  what the correction had been masking.
- **Why those two modes.** The DLSS 310.6 guide (section 3.9, changelog 310.5.0) says exposure
  input is only read by presets J and K; L, the Ultra Performance default, always auto-exposes, and
  M is the Performance default. Our pipeline supplies the engine's exposure, and the 30 September
  frozen-input experiment found preset K smooth on the same hangar inputs.
- **The change.** With the Auto preset and a supplied exposure, Performance and Ultra Performance
  now use K; an explicit preset still overrides this.
- **Status.** The shared-host Streamline test and the gate pass, and the build is deployed. A
  driver-level preset override would defeat this, as the 30 September note warned.

**Correction, 15:00: forced preset K withdrawn again.**
- **Why.** The user restated the 30 September objection: a driver override replaces any preset we
  choose, so forcing K is not a fix.
- **The diagnosis.** DLSS 310's auto-exposing models band in AC7's dark linear HDR. Wider formats,
  input scaling and pre-exposure did not help on 30 September. A bounded perceptual input did.

**The fix: colour transport (`colour_transport.h`).**
- **Encode:** `y = (xE / (xE + 4))^(1/2.2)`, with E the engine's exposure texture.
- **DLSS:** runs with `colorBuffersHDR` off and no exposure tag. That is `rsf_dlss_frame.color_encoded`.
- **Decode:** the output is decoded back to linear before the graph, bloom or the colour correction
  read it.
- **Highlights:** DLSS saturates its LDR output at exactly 1.0. Measured with an FP32 output and the
  clamp removed, a doorway pixel decoded to 3.0e5. Saturated pixels therefore take the current
  frame's bilinear, jitter-aligned HDR value, never below the value at the start of the band.
- **Setting:** on by default, `RSF_DLSS_TONEMAP=0` for comparison.

**Real-runtime evidence** (`rsf_exposure_probe`, frozen `input3-fp16.bin` repro, RTX 4070 Laptop,
Streamline 2.14.1, NGX 310.9.1, default preset per mode). Differences are taken in a display
mapping at E = 5 against HDR Quality (preset K).

| Condition | Dark mean difference | Flat-step fraction on dark gradients |
| --- | --- | --- |
| HDR Performance (preset M) | 0.0164 | 0.733 |
| Transport Performance, knee 0.18 to 8 | 0.0019 to 0.0023 | 0.014 to 0.031 |
| Transport Performance, final (knee 4, recovery) | 0.0021 | 0.020 |

- **Doorway.** The final transport keeps a maximum of 480 against the reference 486. Its doorway
  mean is 368 against 455, softened at the edges by the recovery blend.
- **Overshoot.** At E = 1 an unsaturated near-white overshoots to 617, from sharpening amplified by
  the inverse near 1.
- **Ultra Performance.** Default preset L on today's 533x300 hangar frame runs the transport and
  matches the HDR result visually.

**Tests.**
- `colour_transport` (WARP): encode within 1e-3 of the curve, round trip within 2% up to 50
  exposed units (measured within 1%), saturated recovery exact.
- The shared-host DLSS test now evaluates encoded LDR input through the installed runtime.
- The gate passes 49 tests.

**Status.** Deployed at 15:15. In-game validation, including flight brightness, bloom and HUD
colour, is pending.

**Game result, 15:20.** The user confirms that the transport build removes the Performance and Ultra
Performance banding in game, with the colour correction off and no preset forced. Flight brightness,
bloom and HUD colour were not separately reported.
