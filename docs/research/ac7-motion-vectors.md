# AC7 motion vectors and shader replacement

Research history: findings, hook addresses and pending statuses below apply to their recorded
experiments. Later increments can supersede earlier conclusions. See
[current implementation and validation](../current-status.md) before using this as a feature list.

Investigation on 30 September 2026 against repository revision `0e2df170a9f2`.
The question is whether engine and shader changes can improve independently moving geometry,
particles and temporal reconstruction. The answer is yes, with two distinct tasks: correct the
submitted conventions, then recover motion from producers that the sparse pass excludes.
Replacing a shader does not create previous-frame data, but some engine producers already have it.

This investigation used matching engine source, existing AC7 shader/view captures and the deployed
Streamline binary. It did not launch AC7 or change its installation. The
[metadata](evidence/ac7-motion-shaders-20260930.json) records revisions, hashes and checks.
No native implementation changed. The shader experiment below compiled and created a shader;
it did not render pixels or validate an improvement in the game.

## Evidence and method

The installed `Ace7Game.exe` still hashes to
`c7da97f5f8a807d4f1264adbb074146fcffe9bdc2ffa98791b822cd28e558f4f`, matching
the [build 9855922 hook research](ue418-hook-map.md). The authorized engine checkout is exactly
`4.18.3-release`, revision `0a14a8d537a31ecc77488ced41dbaa0166612ef8`.

The 29 September `briefing-tessellation-fix-20260929-214407` session retained 207 original DXBC
creation blobs: 55 vertex shaders and 152 pixel shaders. These were disassembled with Windows
`d3dcompiler_47.dll`. Encoding constants led to a velocity producer and two consumers; their
instructions and bindings establish their roles more strongly than a texture format does.
Creation does not prove a shader executed in this capture. This is not a complete inventory of
flight materials, compute shaders or tessellation stages. Raw blobs, disassembly and local scripts
remain under the ignored `.local/ac7-motion-research-20260930/` directory.

| Shader role | ReScaleFrame CRC32C | IEEE CRC32 | Evidence |
| --- | --- | --- | --- |
| Rigid velocity VS | `3060c987` | `f88d607b` | Emits current/previous clip positions in TEXCOORD6/7; uses a loose previous transform and previous view matrix |
| Velocity PS | `45946e43` | `1ddf3049` | Perspective divides, jitter selection and packed two-channel output |
| TAA-shaped PS | `c618a960` | `0fa916c5` | Depth t0, colour t1, history t2, velocity t3; camera fallback and history filtering |
| Reflection-history consumer | `a4d87960` | `aa9b26ca` | Ray traversal, velocity t6 and previous-colour sampling; exact engine pass identity remains a lead |

The later 18:25 capture establishes a narrower role for `c618a960` in that scene: it filters
the `a4d87960` reflection output, and reflection/environment composition `1ff7837d` consumes
its result in slot 12. It is an SSR temporal filter there, not evidence of the main scene TAA
insertion point. Preserve this pass while finding the actual main temporal graph node. The
[renderer ownership audit](ac7-renderer-roots-20260930.md) records that distinction and the
upstream GBuffer lifetime defect affecting lighting/reflections in the same capture.

The captured velocity VS uses ordinary current view rows 0..3 and previous rows 79..82.
Its separate CB0 contains the previous object transform, consistent with
`FVelocityDrawingPolicy::SetMeshRenderState` and `FVelocityVS::SetMesh` in stock source.
The captured colour VS also accesses a custom primitive matrix, but that is not evidence that
ordinary colour draws carry the previous object transform. Preserve the earlier distinction.

## Written vectors replace camera motion

The ordinary producer subtracts previous clip position from current clip position after the
perspective divides and removes each frame's jitter. These are NDC differences, with two units
across the viewport. Previous camera projection participates in the VS, so written pixels carry
total camera plus object displacement, not an additive object-only contribution.

The native temporal consumer agrees: it computes camera displacement from depth and view rows
110..113 (`ClipToPrevClip`, offset `0x6e0`), then replaces that displacement with decoded t3
velocity where the stored x channel is positive. Motion remains an internal register value; this
shader writes filtered colour to one target. It does not export a reusable dense motion texture.
That finding covers this consumer only, not every AC7 pass. The motion-blur polar texture and the
previously misidentified half-size mask are still unsuitable ordinary motion inputs.

There is a custom projection path. The captured velocity VS selects view rows 116..119 for
current projection and 120..123 for previous projection when primitive row 17.y exceeds 0.5.
The PS selects the raw clip difference in that case, without subtracting temporal jitter.
The flag's engine owner, class coverage and actual projection jitter remain unverified.
A replacement must preserve this branch until its meaning is established. One blanket jitter
convention for all geometry is not yet justified.

The old observation that most of a panning frame is unwritten establishes sparse coverage.
It never established that written vectors exclude camera motion. Adding camera displacement
to every written vector would count it twice.

## A concrete conversion discrepancy precedes producer changes

The deployed `sl.dlss.dll` is 2.14.1.0. Its single embedded DXBC compute shader was extracted
read-only and disassembled. Its resource bindings, constants and instructions match SDK
`shaders/mvec.hlsl`. The DLL and embedded shader hashes are in the metadata.

For valid written input, that kernel produces `-decoded * mvecScale * renderExtent` in pixels.
For unwritten input, it reconstructs current/previous UV positions and produces previous-minus-
current pixel displacement. With the proxy's decode output scale and Streamline scale both
`(1, 1)`, the written branch treats NDC as UV displacement. The conversion required by the
ordinary AC7 producer is `mvecScale = (0.5, -0.5)` when decode output scale stays `(1, 1)`.

| Motion at 1024x576 | Required previous-minus-current pixels | Current DLSS written branch |
| --- | --- | --- |
| Object moves 10 pixels right | `(-10, 0)` | `(-20, 0)` |
| Object moves 10 pixels down | `(0, -10)` | `(0, 20)` |

These are CPU algebra checks of inspected native shaders and call-site defaults, not measured
aircraft displacement or a live visual defect. `sr_legacy_adapter.cpp` already uses
`(-width/2, +height/2)` for FSR/XeSS. Rust's `MotionToPixels::unreal` expresses current-minus-
previous pixel motion, so its opposite sign is a different temporal direction, not by itself a bug.
Apply an AC7 convention correction at an explicit boundary; changing a generic backend default
would also affect other inputs. Require controlled flight comparison before deployment.

The deployed Streamline kernel also treats decoded `(0, 0)` and any component outside `[-1, 1]`
as missing. Its constants contain no invalid-sentinel value. Our `-1000` marker triggers fallback,
but valid total zero motion under a tracking camera also triggers fallback and becomes camera
motion. Passing `motionVectorsInvalidValue` does not repair this kernel. A dense field with an
explicit validity test, submitted with `cameraMotionIncluded = true`, avoids that extra resolve.
The existing `motion_resolve.cpp` already distinguishes the sentinel from valid zero; its use
for DLSS is a proposed experiment, not an implemented result.

Camera reprojection itself has supporting capture evidence. Recomputing current-to-previous
clip transforms from both unjittered translated projections and the origin delta matched the
stored `ClipToPrevClip` with maximum absolute matrix error `1.44e-6`. Using jittered projections
instead reached `0.0429`. This covers 472 repeated VS/PS/HS/DS bindings across capture labels
f0/f1, not 472 independent frames. Keep the engine matrix; a stationary-camera jitter test and
flight/cut validation remain outstanding.

## What RenoDX and Luma establish

The requested checkouts were cloned under ignored `references/`, without submodule installation
or changes to the game. Both were clean after inspection.

| Reference | Inspected mechanism | Use here |
| --- | --- | --- |
| [RenoDX `25a0e773147f`](https://github.com/clshortfuse/renodx/tree/25a0e773147f0022eb2e56db381fa592eb9017c7) | Shader replacement on creation and binding, replacement lifetimes, injected constants, DevKit shader/draw inspection | AC7 already has a hash-selected colour/UI mod. This demonstrates an AC7 replacement pattern, not improved motion support. Its metadata says it was superseded by the generic Unreal mod. |
| [Luma `8ff992c450b8`](https://github.com/Filoppi/Luma-Framework/tree/8ff992c450b8f8164fd5790759c8b22d2284d338) | Cached shader replacements, synchronous/asynchronous token patches, DXBC container repair and UE motion resolve | The UE resolve replaces camera motion with written object motion and converts NDC to previous-minus-current pixels using `(-0.5, +0.5) * extent`. Depth-aware dilation is useful to compare, not proof it improves AC7. |

Their hashes must not be copied directly into our settings: ReScaleFrame uses CRC32C, while
RenoDX's inspected lookup uses IEEE CRC32. Store a strong original-bytecode fingerprint and verify
stage, signatures and bindings as well as any short diagnostic hash.
Ten active hashes in RenoDX's AC7 replacement table match the original captured blobs after
computing IEEE CRC32, including holograms, icons, UI composition and output shaders. This is
direct bytecode overlap with our build, not just a matching game name.

RenoDX is MIT. Luma has a custom MIT license with attribution and commercial-use restrictions.
Neither framework's code was copied. The ReShade addon route is useful for temporary analysis;
the first-party replacement service should stay in this monorepo. The AC7 plugin owns exact
shader fingerprints, engine gates, vertex-factory interpretation and conventions. The runtime
owns shader/resource creation, binding restoration and backend submission.

## Producer improvements worth attempting

### Attached weapons and moving ground vehicles

The user identifies missing weapon/missile colouring in older RenderDoc captures of carrier
launch and refuelling scenes, possible missing moving ground vehicles, and missing cloud vectors.
These are coverage targets. They are user observations, not newly reproduced captures. Diagnose
the raw written mask as well as decoded motion: black in a vector visualization can represent
valid zero displacement, and a camera-tracked object can have zero total screen motion.

A relevant selection gate is now located in AC7's decrypted image, using Capstone and the known
dynamic velocity loop as the entry point. The fingerprint and permitted binary metadata are in
[coverage evidence](evidence/ac7-velocity-coverage-20260930.json). The method was chosen because
shader replacement alone cannot affect a primitive rejected before its draw is scheduled.

`RenderDynamicVelocitiesMeshElementsInner`, RVA `0x1184dc0`, calls `0x10fdc00` at `0x1184e60`,
then draws through the previously identified `DrawDynamicMesh` at `0x1182390` only on success.
The callee matches stock `FPrimitiveSceneInfo::ShouldRenderVelocity`: visibility, mobility,
opaque/main-pass relevance, bounds/distance size rejection, `HasVelocity`, and base-pass routing.
Its source counterpart also controls the static mesh velocity visibility map.

The size gate compares:

```text
boundsRadius^2 <= (MotionBlurPerObjectSize * 0.02)^2 * distance^2 * LODDistanceFactor^2
```

The binary uses proxy bounds origin at `+0x100`, radius at `+0x118`, view camera origin at
`+0x40c`, LOD factor at `+0xc2c`, and the size setting at `+0x1174`. The multiplier at RVA
`0x2671470` is `0.02`. Stock settings initialize the percentage to `0.5`, but AC7's effective
per-scene value has not been measured. At that reference value and LOD factor 1, a primitive
with radius/distance ratio below `0.01` is rejected. A separately bounded missile, weapon or
distant vehicle can fail while a larger aircraft body passes. A weapon that is merely a section
of the same aircraft primitive shares its bounds, so size culling alone would not explain it.

The rejection branch is RVA `0x10fdd0c`, bytes `76 38`, a `JBE` to the false-return block at
`0x10fdd46`. A guarded diagnostic bypass would replace those two bytes with `90 90`, retaining
visibility, material relevance, history and later selection checks. No patch was applied.
First correlate a missing target's bounds, threshold and rejection reason with its colour draw.
If size rejection is responsible, patch this gate after decryption rather than relying on a
post-process setting the scene can overwrite. Validate added draws and GPU cost.

The subsequent `FVelocityDrawingPolicy::HasVelocity` analogue is RVA `0x1183820`. Its binary
matches camera-cut rejection, mobility selection, an always-velocity flag, previous-transform
lookup, and a 16-element matrix-equality test with tolerance `0.0001`. Missing history and an
unchanged component transform can still suppress a draw after the size gate is removed. Stock
skeletal proxies set always-velocity when per-bone motion blur is enabled; a rigid component
test is insufficient for a mesh animated through bones or scene-specific deformation.

The native size gate is confirmed; its responsibility for the reported scenes is not. For each
weapon, attached missile, launched missile and moving truck/tank, first distinguish a separate
primitive from a material/LOD section. Check parent-to-child world-transform updates, previous
transform registration, bone history, material permutation, camera cuts and depth-test outcome.
Dense camera fallback alone cannot recover independent vehicle motion. Do not fabricate motion
from a parent aircraft when a missile has detached or a turret moves independently.

### TrueSky clouds

AC7's installed `TrueSkyPluginRender_MT.dll` was inspected read-only. Its exported render boundary
includes `StaticRenderFrame`, `StaticRenderFrameEarlyAsync`, view registration, matrix and texture
setters, and render float getters/setters. Native string leads include `windSpeed`, `windDirection`,
`OverrideWind` and `raytrace_reproject_clouds_compute`. These support investigating TrueSky's own
wind and reprojection history. They do not establish that wind runs in the reported scene or that
a usable depth/motion texture is exposed. Simul describes trueSKY as a volumetric weather system
updated in real time, but that general capability does not establish AC7's configuration.
[Vendor description](https://simul.co/).

Static density still has camera-induced screen motion. Finite-distance clouds also show
translational parallax, so opaque ground depth or clear sky depth cannot be assumed correct.
Wind advection and evolving density are additional changes; there may be no single exact motion
vector for a pixel integrating multiple depths through a volume. An effective depth and dominant
motion can be useful approximations, with transparency/disocclusion handling measured separately.

Capture a stationary camera over several unpaused frames to separate density/wind movement from
camera movement, then repeat camera rotation and translation. Inspect the cloud producer's view,
time, history, depth and wind inputs at the TrueSky rendering boundary and trace its reprojection
shader before exporting motion. Do not freeze its animation to manufacture zero vectors or assign
opaque background motion as though it described the volume. No new cloud runtime test was run.

### Particles and deformation

CPU sprites provide a separate producer lead. Stock `ParticleSpriteVertexFactory.ush` receives
`OldPosition` at ATTRIBUTE1, and `ParticleSystemRender.cpp` populates it from `Particle.OldLocation`,
including prior orbit offsets. Nevertheless its `VertexFactoryGetPreviousWorldPosition` returns
the current position. A custom VS can use the existing old center for translational motion,
provided AC7's actual input stream, input layout and simulation-to-render timing agree.
Previous billboard orientation, size, rotation, emitter transform and material deformation are
separate requirements; the old center alone is not complete sprite motion.

Beam/trail inputs also declare `OldPosition`, but several stock fill paths write the current
location into it. Do not assume a usable history merely because the semantic exists. Ribbons
may need prior generated vertices or engine history capture. GPU particles need their simulation
history inspected separately. Aircraft control surfaces need previous bones or deformation
state; stock GPU skinning supports previous bone buffers but their AC7 use is still unverified.
TrueSky clouds require their own producer/history investigation rather than opaque background
vectors. Transparent colour can contain multiple differently moving layers, which one vector
and opaque depth cannot represent exactly.

The existing dynamic blend-gate patch at RVA `0x11823de` reaches some map icons, and previous
flight evidence shows no tracer-coverage improvement. A wider gate alone cannot supply absent
permutations or previous geometry. A replacement/replay pass can supply its own shaders, but
must reproduce material alpha coverage, deformation, culling and actual layer depth.

An independent probe was authored from the observed velocity PS interface and equations. It
preserves the packed `SV_Target0`, preserves the custom projection/jitter selection, and adds
unquantized current-minus-previous UV motion at `SV_Target1`. `D3DCompile` accepted `ps_5_0`,
and Windows D3D11 WARP accepted `CreatePixelShader` at feature level 11.0. No draw/readback was
performed. This establishes a compilable sidecar design, not equivalence of rendered pixels.
It does not add missing geometry: only draws already reaching the velocity pass gain the output.

For a real sidecar, bind a matching owned motion target at those draws, clear it to a distinct
invalid value, and preserve the original packed output for game consumers. Check per-target blend
state, write masks, depth tests, extent, MSAA, stage linkage and resource hazards. Dense camera
fallback can fill unwritten pixels afterward. Changes to sprites require compatible VS/PS and
input layouts; changing pixel arithmetic alone cannot recover a missing previous position.
Creation-time replacement cannot fix a draw that the engine never schedules.

## Next experiments and acceptance

1. Audit carrier launch and refuelling first, then moving ground vehicles in a mission. For each
   missing class, match its colour/depth draw to a velocity draw or a recorded rejection reason,
   and verify previous-frame transforms/bones. Capture the custom projection flag, frame/view
   identity and backend scales. Check tracked and crossing aircraft/missiles on both axes at
   native and reduced resolution. Correct coverage takes priority alongside unit/validity checks.
2. Compare the corrected ordinary-vector scale with the baseline. Separately compare the existing
   explicit-sentinel dense resolve against Streamline's sparse resolve, especially tracked zero
   motion and disocclusions. Do not combine scale, coverage and dilation changes in one trial.
3. Add guarded shader substitution and a float sidecar for the identified rigid producer. Require
   original-bytecode and resource-signature matches, logged refusal, and original behavior on
   mismatch or creation failure. Capture before pooled inputs are overwritten, with the matching
   scene/view; retaining a texture allocation does not preserve its frame contents.
4. Find a CPU sprite in a flight capture and prove that its old center represents the prior
   rendered frame. Patch that producer class first, preserving alpha coverage. Then investigate
   skinned geometry, beams, trails and clouds from their respective history owners.
5. Verify static-camera jitter, camera pans/roll/FOV, camera cuts, skipped evaluations, resolution
   changes, material coverage and GPU cost. Keep colour/depth/motion aligned and UI outside the
   scene history. Report capture, synthetic, game and target-device results separately.

Ghidra's configured local bridge was not running during this investigation. Capstone mapped the
coverage gates through the known decrypted-image caller chain and matching source/control flow;
no RVA was inferred from source alone. Ghidra headless subsequently named and decompiled all
three functions in `references/ghidra-ac7-motion`, using the matching decrypted image.
The initial headless attempt failed because Ghidra rejects project paths containing `.local`;
moving the project under ignored `references/` resolved it. No eligibility bypass is applied.

## Bounded capture implementation, 30 September

`RSF_MOTION_CAPTURE=1` enables F9 capture for 60 Present intervals. Expected-byte guarded
detours at `0x10fdc00`, `0x1183820` and `0x10ee670` record selection facts and previous
transforms after decryption, forwarding every original decision. Samples at intervals 0, 30
and 59 collect immediate-context draw/dispatch state, shader bytecode, constant-buffer bytes,
and paired backend raw colour/depth/motion resources with camera constants. Deferred command
lists are not decomposed. CPU decisions and GPU records have separate timing identities;
matching them requires view, resource and transform evidence, not interval numbers alone.

Limits are 16,384 engine records, 12,000 GPU records, 4,096 constant-buffer blobs/32 MiB,
4,096 shader blobs/64 MiB, and 128 fallback CB readbacks. Metadata reports drops and failures.
Shadow draw facts describe requested state; live resource bindings include applied target
promotion, with temporary CB overrides restored. Raw image files preserve native values.

Windows WARP tests exercised six shader stages, draw/dispatch recording, CB bytes, exact
colour and packed depth/stencil readback, and refusal of engine hooks on a different image.
The Release verification gate passed: 30 tests passed, four vendor-environment tests skipped,
plus Rust formatting and lint. This is synthetic validation; AC7 scene captures remain pending.
Use the [F9 procedure](../../loader/README.md) and `tools/analyze-ac7-motion-capture.py`.
