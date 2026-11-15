# Skyrim SR reference parity, 15 November 2026

Question: does RSF actually give DLSS the same inputs and temporal contract as the
working Community Shaders path, and does the retained capture justify a native
animation-history repair for pickup-only ghosting?

This is an independent offline source and captured-data review. No game interaction,
injection, deployment, GPU fixture, build or new runtime capture was performed.
The existing final image shows an NPC beside the mill, but does not establish the
pickup phase. A clean steady-carry draw is not evidence for the pickup transition.

## Evidence

- SkyrimSE 1.7.104, executable SHA256
  `846efccf0c1374d71f892907f46549560f2fcb0a75cb87a3eed438baa0f1402f`.
- Community Shaders latest reference `038a0a04ce9db5b84707ec662ddfe7824cdac3dd`
  and pinned 1.9.1 reference `265c28f5`. Inspected `Upscaling.cpp`,
  `Upscaling/Streamline.cpp`, `EncodeTexturesCS.hlsl`, `Lighting.hlsl`,
  `Common/MotionBlur.hlsli` and `Common/Skinned.hlsli`.
- User F10 capture `scene-20261115-223624-382-40500062-43160-1`, under the local
  Skyrim installation's `ReScaleFrame/captures/website`. Manifest: session 1,
  source frame 1624, native frame 1836, matched Present, SR result 0.
  Active input 1280x720 in 1920x1080 native allocations; output 1920x1080.
- Native shader archive and draw provenance are in
  [the draw-coverage note](skyrim-native-draw-coverage-20261115.md).
  Independently read the native Lighting PS motion instructions and Lighting VS2
  current/previous skinning instructions, not only their output declarations.

## End-to-end comparison

| Contract | RSF path and captured evidence | Community Shaders reference |
| --- | --- | --- |
| Timing and colour owner | Skyrim `reconstruct` reads target1 before the native image-space/HDR call; successful SR suppresses native TAA and reinserts into target1 | `Main_PostProcessing` performs SR on `kMAIN` before native postprocessing and suppresses native TAA |
| Active rectangle | `native_regions` crops the top-left 1280x720; colour converts R11G11B10 to RGBA16F and depth to R32F | Full allocations with render extent limited to dynamic resolution |
| Colour supplied to DLSS | Capture `color_encoded=1`: `pow(C/(C+4),1/2.2)`, exposure fallback 1 | Native HDR colour; no RSF colour transport |
| HDR and exposure options | `colorBuffersHDR=false`, `useAutoExposure=false`, no exposure texture | `colorBuffersHDR=true` for float main target, `useAutoExposure=true`, `preExposure=1` |
| Motion direction | Native Lighting computes `(-0.5,+0.5)*(currentNDC-previousNDC)`, which is previous-minus-current UV | `MotionBlur::GetSSMotionVector` computes the same expression |
| Motion ingestion | Native crop equals `sr_game_motion` and `sr_motion_decoded` exactly. Shared evaluation resolves to pixel units, then uses scale `(1/1280,1/720)` | Native UV with scale `(1,1)`, after DLSS-specific conditioning |
| Valid stationary motion | 702,379 zero vectors survive the captured chain. Shared evaluation changes the invalid marker to FLT_MAX | Invalid marker FLT_MIN, so ordinary zero is valid |
| Camera and jitter | Camera-inclusive, unjittered vectors; standard depth. Geometry jitter `(-hX,-hY)` pixels from native projection | Same flags and jitter `(-hX,-hY)` from the Halton sample |
| Motion preprocessing | Unit conversion and finite-value sanitation, with no neighborhood dilation in this path | DLSS-only 5x5 search for a longer vector at a nearer depth, faded toward the original vector near the camera |
| Masks | No Skyrim reactive, bias-current-colour or transparency mask supplied in this capture | Bias-current-colour from TAA mask `x*0.1+y`; transparency hint from CS deferred normal packing |
| Reset | Adapter checks camera/frame/dimensions/native previous matrices; runtime also resets on history and configuration changes. Captured reset=0 | Latest uses `ShouldResetHistory`; pinned 1.9.1 explicitly sends false |
| Model request | Captured preset0, SDK default | Default CS quality request is J on RTX20/30 and M on newer RTX; user override possible |
| Resource lifetime | Snapshot/crop before auxiliary commit; separate SR output; D3D11 upload signal, D3D12 queue wait/evaluate/signal, D3D11 wait/copy/reinsert | Native D3D11 evaluation; separate intermediate output and subsequent copy/sharpening |

The direct D3D11 backend leaves `motionVectorsJittered` and
`motionVectorsDilated` at the SDK struct defaults, both false in the checked
`vendor/streamline/include/sl_consts.h`. An omitted explicit assignment is not a
flag discrepancy here. This capture used the shared path and contains the
FLT_MAX invalid marker. It does not validate every direct-path configuration.

The native PS instructions divide current and previous projected positions by
their respective W, subtract previous from current, then multiply by
`(-0.5,+0.5)`. Native Lighting VS2 blends cb10/current and cb9/previous bone rows
after subtracting their respective cb12[40]/[41] pivots. That agrees with the
CS skinning helper. The retained steady-carry draw also verifies the rigid
previous-world binding. These facts support those paths, not unobserved pickup
frames or every shader permutation.

## Numeric checks on the retained frame

Read the packed raw files according to their format metadata, using NumPy on
the CPU. No screenshot-derived vector values were used.
Reproduction script: `.local/skyrim-research/check-sr-reference-parity.py`;
results: `.local/skyrim-research/sr-reference-parity-20261115.json`.

- Native R11G11B10 colour decoded over the active rectangle equals
  `sr_source_jittered` exactly. Native 24-bit depth normalized to float equals
  `sr_scene_depth` and `sr_depth` exactly. No shifted or prematurely upscaled
  colour/depth input is shown by this capture.
- Native motion cropped to the active rectangle equals `sr_game_motion`
  exactly; that equals the R32G32 decoded input exactly. All values are finite.
- Resolved motion agrees with native UV times `(1280,720)` to half-float
  rounding: maximum difference from a NumPy half-float reference is
  0.00048828125 input pixels. It is not a sign, scale or crop mismatch.
- Depth remains standard-Z, range 0.92149264..1. The captured camera
  clip-to-previous transform is identity within approximately 2.8e-8.
- Captured `sr_color` agrees with the display-range transport formula within
  0.000244140625. The HDR/exposure difference is exercised by this frame, not
  merely a dormant source branch.
- In input box `[175,335,215,395)`, covering the NPC and nearby background,
  maximum resolved vector length is 0.3501 input pixels. It contains no large
  stale displacement. It cannot disprove stale history on another frame.
- A CPU simulation of the reference dilation, comparing lengths in native UV
  as its shader does, changes 115 of those 2,400 pixels by more than 0.1 input
  pixels. Maximum change is 0.3503 input pixels. Across the frame, 5,013 pixels
  change by more than 0.1 pixels, maximum 0.4202 pixels. This measures a real
  conditioning difference; it does not simulate DLSS or demonstrate a fix.

The pinned and latest CS reference directories contain the same `nvngx_dlss.dll`
310.5.2.0, SHA256
`a8f23de8116727a160a196f9f43604284cd973d801a62bb68c19c828f78e5f3b`.
The installed RSF copy inspected during this review is 310.9.1.0, SHA256
`3975567b8943c53acce397f2b72380092f84f162d00b0d2c7d08a1025c563983`.
These are on-disk artifacts, not proof of which provider or driver model produced
an earlier CS session. Preset0 is not proof that CS and RSF used the same model.

## Diagnosis and bounded next decision

The earlier interface/bone audit did not establish parity with the working
solution. Colour transport/exposure, model selection and motion conditioning
are concrete differences upstream of DLSS's temporal decisions. Consequently,
the available evidence does not justify selecting native object-history
mutation as the fix. CS has no corresponding object-history repair.

The generic host's enabled-by-default colour transport is a confirmed
configuration difference, not a confirmed inappropriate Skyrim default or
invalid SDK contract. The encoded values match its SDR flags, and decode
restores linear colour before native postprocessing. Its AC7 motivation alone
does not establish a regression in Skyrim. A visual comparison is required
before classifying it as a cause or changing the default on that basis.

The existing shared overlay's **DLSS colour transport** control can restore the
native HDR/auto-exposure path without new code: disabling it supplies native
linear colour, makes HDR true and enables auto-exposure when Skyrim supplies no
exposure texture. This is the smallest available parity check, not a claim that
transport caused pickup ghosting. Preset parity also requires an explicit same
model request and knowledge of the effective provider, not merely matching the
Quality label.

If those contracts are held equal, the strongest integration-level candidate is
the missing depth-aware motion conditioning at moving/disocclusion edges. The
reference deliberately modifies those samples before DLSS, and the retained
capture shows exactly which samples RSF leaves different. The vanilla TAA mask
remains a separate candidate until its contents at the pickup are known. CS's
normal-channel transparency hint cannot be ported by slot number: vanilla
Lighting writes zero into that channel.

A native-history repair needs an attributable pickup frame outside a reset,
with that object's current/previous transforms, bound constants, visible depth
and motion footprint. The failed v5 watch's stale stamps after a world-render
gap do not supply that evidence. Neither does this single-frame capture.
No runtime patch is presented as a verified pickup fix by this review.
