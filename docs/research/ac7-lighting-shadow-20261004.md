# AC7 close-ground shadow stippling, 4 October 2026

Research history: findings, hook addresses and pending statuses below apply to their recorded
experiments. Later increments can supersede earlier conclusions. See
[current implementation and validation](../current-status.md) before using this as a feature list.

The user supplied a flight screenshot with regular dark stippling across the ground and
aircraft, plus three F9 captures. The defect is already visible in the raw pre-SR scene
colour. The investigation therefore follows native lighting production before changing
DLSS, frame generation or final composition.

## Identity and captured evidence

- Executable SHA-256: `c7da97f5f8a807d4f1264adbb074146fcffe9bdc2ffa98791b822cd28e558f4f`.
- Retained decrypted dump SHA-256: `fafd1db2808d32333caecf4056e8bcd676c2f07e4c1fabc0f270ef29f22d24db`.
- Ghidra MCP confirmed `Ace7Game.exe.dump`, image base `0x7ff741350000`, 71,407,384 mapped
  bytes and 161,531 functions. No program switch or binary patch was made.
- Matched engine checkout: `references/UnrealEngine`, revision
  `0a14a8d537a31ecc77488ced41dbaa0166612ef8`, verified with `git rev-parse HEAD`.
- Capture directories under `%LOCALAPPDATA%/ReScaleFrame/AC7`:
  `motion-20261004-075626-104792-1`, `motion-20261004-075643-104792-2` and
  `motion-20261004-075731-104792-3`.

The third capture has 60 intervals, zero engine/draw/blob drops, zero texture readback
failures and complete raw scene/backend/graph/downstream images. Its first backend frame
uses DLSS with a 533x300 active input and 1600x900 output. The engine allocation is 536x300;
this padding is separate from the valid input rectangle. Captured current jitter is
`(-0.371318191, 0.0729243532)` input pixels.

`f000_input.bin` is FP16 scene radiance, four channels, tightly packed row pitch 4264.
SHA-256: `b0872794b45ca5408d6c754ec3c006ff7910404edf0a06841315b20ac07239f5`.
The same stippling is visible in `f000_input.tga`, DLSS `f000_output.tga`, native tonemap
`f000_route_f1_s5_native_pass_2_output.tga`, and final
`f000_route_f1_s15_final.tga`. TGA previews alone do not establish radiometric values;
the raw FP16 image confirms this is an upstream spatial feature rather than a UI pattern.
The first native route identity reports source frame 60297, native frame 18129, submission
60311 and viewport key 2235119832712. Capture interval and queued native frame are different
timelines, so a sampled draw is not automatically the owner of every paired image.

Local diagnostic scripts/images remain under `.local/ac7-lighting-20261004` and are
untracked. They decode captured bytecode with system `D3DDisassemble` and inspect the
existing raw images; they do not modify capture inputs or game resources.

## Native producer lead and its limits

The sampled 536x300 directional-light draw uses pixel shader hash `3619939816`
(`shader_1_643_3619939816.dxbc`). Its SHA-256 is
`60a2ca61e5d83b853f692ee8c0bc3580ea46b8e33d72c644949e5f37c1060a12`.
The bytecode declares View CB0 and a nine-register light CB1. It checks light
`CB1[8].x > 1` and contact length `CB1[5].z > 0`, then traces eight depth samples with
ray length proportional to scene depth. This matches the contact-shadow branch in
`Engine/Shaders/Private/DeferredLightingCommon.ush` and the light uniform producer in
`Engine/Source/Runtime/Renderer/Private/ShadowRendering.h`.

Its interleaved gradient noise is computed directly from `SV_Position.xy` using
`(0.06711056, 0.00583715, 52.9829189)`. There is no frame offset in this computation.
The matched stock `DeferredLightPixelShaders.usf` calls the noise function with
`View.StateFrameIndexMod8`; `Random.ush` shifts the pixel coordinates by that phase.
The captured SSR shader hash `2765650272` does use the corresponding temporal noise:
unsigned `CB1[137].z` converted to float, then a pixel offset of
`(32.665001, 11.815000)` before the same noise computation. This contrast establishes
a shipped shader difference, not a Function ID guess about a CPU routine.

The stock PCF projection shaders observed in this interval perform regular depth filtering;
the earlier generic PCSS lead did not identify the active stippled producer. Changing
the current 72-phase Ultra jitter sequence cannot add a missing phase operand to the
directional shader. No jitter-phase edit was made.

A local ground-region correlation against the stationary noise had a maximum threshold
correlation of 0.476 at interval 30, with weaker correlations elsewhere. Geometry,
other lighting and different capture timelines confound this test. It supports investigation
of contact shadows but does not prove they are the sole source of the visible dots.
The shader contains a contact branch; whether it executes depends on the actual light values.

Those values are missing in the current capture: both directional PS CB slots report
`blob=0` at intervals 0, 30 and 59. The capture spent its existing 128 per-interval CB
readbacks before lighting. Consequently neither contact length nor actual view phase
can be recovered from these particular draw records. A shader replacement or disabling
contact shadows would exceed the available evidence.

## Scale clarification from the user

The user subsequently reported that the defect is worst in Ultra Performance and weaker
at native resolution or Quality. This is user-run visual evidence. All nine paired samples
across the three supplied captures use the same DLSS active rectangle, 533x300, with
1600x900 output. They contain no captured native/Quality comparison. The mode correlation
therefore does not identify DLSS as the producer.

The observed shader has two distinct coordinate domains:

- Its noise uses input raster `SV_Position.xy`, measured in input pixels. At this scale,
  one such pixel spans approximately three output pixels in each dimension. A fixed noise
  pattern can consequently become visibly coarser even though DLSS receives it faithfully.
- Light contact length at `CB1[5].z` multiplies reconstructed scene depth before applying
  the light direction. The length multiplier is dimensionless; the resulting ray is in
  the same world units as the reconstructed geometry, rather than a fixed pixel radius.
- The ray start and end are transformed through the current View matrices into normalized
  clip coordinates, then normalized depth-texture UVs using `View.ScreenPositionScaleBias`.
  Eight samples and the `1/8` step remain fixed across resolutions. The depth comparison
  tolerance is proportional to projected ray depth and `2/8`, not an output-pixel footprint.
- Scene depth sampling reads the reduced native depth texture. Reducing raster resolution
  therefore reduces the depth detail available to the same projected ray; projection and
  comparison tolerance do not reconstruct the lost geometry. The directional shader also
  samples native shadow-mask and precomputed-shadow inputs, distinct from the depth ray.

These operands are visible in the captured bytecode and match
`DeferredLightingCommon.ush::ShadowRayCast`. No evidence shows an incorrect world-unit
conversion or viewport uniform here because the decisive View/light CB contents are absent.
The regular PCF shadow-map filters are a separate path; their shadow-map texture resolution
must not be conflated with the scene input rectangle.

The reserved capture still discriminates the producer at Ultra Performance: if the
pre-light image is clean but the post-light delta contains dots, the directional draw
introduces them; if pre-light colour, depth or shadow mask already contains the pattern,
investigation moves to that input's producer. An actual zero contact length or shadow flags
below two reject the contact-ray hypothesis. A matched native/Quality F9 comparison would
then measure scale dependence rather than infer it from appearance alone.

## Applied capture correction and next acceptance

`games/ac7/src/motion_capture.cpp` now reserves eight of the existing 128 CB readbacks for
the exact directional shader. At its first sampled draw, it reads the two live PS constant
buffers before execution and emits a `contact_light_before` record with queued scope.
It captures the pre-light colour, light mask at SRV3, depth at SRV4 and static-shadow input
at SRV5, then the post-light colour. Eighteen slots of the existing 72 texture-read budget
are reserved for these bounded observations across the three sampled intervals. The global
budgets and resource retention limits remain unchanged. This code runs only during F9
capture, changes no render bindings and introduces no new native patch/hook site.

One new close-ground F9 capture can now answer whether the stippling appears in the
directional-light delta, whether the contact branch is enabled, and whether the supplied
view phase advances correctly. A production correction must preserve contact shadows and
use the actual current view phase if temporal noise is repaired. A global Present index,
blanket lighting disable or guessed shader operand is not supported by this evidence.

`git diff --check` passed for the capture source. Compilation, the required native gate,
deployment and the user-run capture belong to the parent session and are pending at this
note's creation. The visible lighting defect remains unresolved.

## Fresh capture isolates the directional-light draw

The user ran the deployment containing the reserved diagnostics and made
`motion-20261004-082205-100100-1` and `motion-20261004-082226-100100-2`.
Both again have 533x300 active input and 1600x900 output at all sampled intervals;
neither supplies a Quality comparison. Each has three complete `contact_light_before`
records with successful pre/post-light and input snapshots. The live View CB is 4096 bytes;
the light CB is 256 bytes.

Across both captures, the light contact length at `CB1[5].z` is
`0.03999999910593033` and unsigned shadow flags at `CB1[8].x` are 3, enabling the contact
ray. Unsigned current View phase at `CB0[137].z` is 7, 5 and 2 at intervals 0, 30 and 59.
Thus the View phase advances; this is not a frozen view-state counter.

The regular dots appear in the HDR colour added by the directional draw, including on the
aircraft, and remain visible in its post-minus-pre colour delta. They are absent from the
corresponding near-black pre-light contribution and do not appear as the same dot pattern
in the shadow-mask or static-shadow inputs. This isolates the directional lighting draw as
the producer of the visible pattern. It does not establish that every dark pixel is a
contact hit; material normals and visibility still contribute to the full lighting result.
The local full-draw correlation remains region-dependent, so it is not used as a substitute
for the pass-boundary evidence.

These captures correct the earlier incomplete evidence: contact shadows are actually enabled,
their current View phase is available and changing, and the shipped directional shader omits
that phase specifically from the contact-ray noise. Repairing that omission is a source-grounded
production increment; moving-game appearance still requires validation.

## Guarded temporal-noise correction

`games/ac7/src/contact_shadow.cpp` and `.h` provide an internal plugin helper accepting the
DXBC span and returning owned transformed bytes with `unmatched`, `patched` or `refused`.
No STL object crosses the game DLL ABI. It recognizes only the full SHA-256 above and
the exact 10,456-byte container. It also verifies three chunk offsets/sizes, the pixel
shader token header, the 14-temporary declaration and the existing noise instruction.
Other shader variants remain unchanged.

The helper allocates temporary `r14` and changes `dcl_temps 14` to 15. Immediately before
the existing noise dot product it adds these independently encoded instructions:

```text
utof r14.x, cb0[137].z
mad r14.xy, r14.xxxx, (32.665001, 11.815000, 0, 0), v2.xyxx
dp2 r1.x, r14.xyxx, (0.06711056, 0.00583715, 0, 0)
```

The constants match the active SSR temporal-noise convention. The third instruction is
the original dot product with only its coordinate operand redirected. No light parameter,
depth comparison, ray length, ray sample count, branch, signature or resource binding changes.
It consumes the actual current View phase, not a global Present counter. No game shader blob
or licensed engine shader source is embedded in the implementation.

The inserted instructions add 72 bytes. Container length, SHEX chunk length and token count
are updated. Input/output signature chunks remain byte-identical, as does any supplied engine
trailer. The checksum uses standard MD5 compression with DXBC's documented final-block framing:
hash from byte 20, place the bit count before a short final remainder or in a separate block
after a long remainder, and place `(bits >> 2) | 1` in the last word. Algorithm descriptions
are available in Microsoft's [DXIL hash implementation](https://microsoft.github.io/DirectXShaderCompiler/coverage/home/runner/work/DirectXShaderCompiler/DirectXShaderCompiler/lib/DxilHash/DxilHash.cpp.html)
and AMD's [DXBC checksum implementation](https://github.com/GPUOpen-Archive/common-src-ShaderUtils/blob/master/DX10/DXBCChecksum.cpp).
The production implementation was written independently and verifies that its checksum
reproduces the original captured container before editing.

Attempted system compiler shortcuts were rejected: `D3DSetBlobPart` returned `E_FAIL` for
a container with an invalidated incoming checksum; `D3DAssemble` rejected the SM5 profile.
These failures are why the helper computes the container checksum itself.

An ignored standalone fixture compiled the actual helper with MSVC 18 under `/W4 /WX`.
The 10,528-byte transformed shader passed `D3DDisassemble`; its listing confirms the new
phase instructions, redirected dot product and 15 temporaries. Both WARP and hardware
`ID3D11Device::CreatePixelShader` accepted it with `S_OK`. The fixture also checked unchanged
signature chunks, retained trailing bytes, short-input refusal and rejection of a single-byte
fingerprint mismatch. These prove bytecode/device acceptance, not final moving-game quality.

Parent integration must hook the matched native pixel-shader creation owner, preserve the
UE resource-table prefix and optional trailer, announce matched patch success/refusal, and
run the combined native gate. Deployment and user-run acceptance remain pending. The requested
visual acceptance is a settled close-ground pass at Ultra Performance with aircraft and ground
contact shadows retained, followed by a native/Quality comparison and normal camera motion.

## User-run result and phase execution check

The user reports the lighting defect persists after the 08:45 deployment. This is a negative
game acceptance result; the temporal-noise correction is insufficient to claim the defect fixed.
The complete frozen log `.local/reflex-light-0845-live.log`, line 858, contains the successful
guarded shader-transform message. An initial inspection of only the first filtered lines
missed it. The hypothesis that the native factory hook never matched is therefore rejected.
No F9 capture newer than 08:22 was present at this investigation checkpoint.

Current Ghidra decompilation confirms native creation at RVA `0xe31e90` reads the table prefix
and five serialized arrays, derives DXBC length by subtracting the optional trailer size,
and passes those bytes synchronously to D3D11 `CreatePixelShader`. The driver resource lives
at returned RHI shader `+0xa0`. The wrapper's prefix/trailer parser matches that path. No
fingerprint weakening or another noise-coordinate change was made.

The ignored `.local/ac7-lighting-20261004/phase_replay.cpp` probe executes the actual original
and transformed shaders offscreen on D3D11 hardware. It uses captured View/light CBs,
scene depth and shadow-mask/static-shadow textures, with explicitly synthetic GBuffer normals,
roughness, shading model and base colour. The depth input is `R32G8X24_TYPELESS`, eight bytes
per pixel; the depth channel is the first float and the stencil padding is not sampled as depth.
The fixture supplies a fullscreen vertex stage matching the recorded UV and camera-ray domains.
It changes only the current View phase between 0, 1 and 7, then repeats with contact length zero.

| Shader/contact condition | Phase 0 to 1 changed pixels | Phase 0 to 7 changed pixels |
| --- | ---: | ---: |
| Original, contact length 0.04 | 0 | 0 |
| Transformed, contact length 0 | 0 | 0 |
| Transformed, contact length 0.04 | 55,488 | 53,716 |

Counts use the 536x300 allocation. Mean absolute red-channel changes for the last row are
0.0731246 and 0.0580399, maximum 1.27039 and 1.25641. This control establishes that the
inserted phase reaches contact-ray jitter and changes actual shader output through the enabled
contact branch. It does not establish that the shipped game's GBuffer, active shader binding
or reconstructed moving output is clean. The probe compiled with MSVC `/W4 /WX`; its results
are retained in `.local/ac7-lighting-20261004/phase-replay.log`.

The wrapper's bounded creation report now runs after the native factory returns, reports whether
RHI `+0xa0` is nonzero, and names the RHI/D3D11 pointers plus transformed CRC `2950909598`.
This avoids treating a pre-call message as driver creation evidence and supplies an identity to
compare with the next captured draw. The existing F9 capture predicates already recognize both
the original and transformed CRC, so no additional capture edit was necessary.

The next discriminating evidence is a post-patch F9 capture showing the actual bound CRC,
current phase and directional pre/post-light images at Ultra Performance, plus a matched
Quality capture if available. If transformed CRC/phase are present while dots remain in the
light delta, the remaining problem is spatial depth/ray undersampling or downstream temporal
integration rather than a missing phase operand. That distinction is not settled by the current
older captures. Parent compilation/deployment of the post-factory report remains pending.

## Active-view scale and depth-layout audit

The six live directional View CBs from the 08:22 captures are coherent with the actual
536x300 depth allocation and 533x300 active viewport. This rules out a stale output-resolution
size/bias uniform in those particular draws; it does not establish every later view path.

| Captured field | Value | Matched expectation |
| --- | --- | --- |
| `ViewRectMin`, CB0[126] | `(0, 0, 0, 0)` | Zero-origin active viewport |
| `ViewSizeAndInvSize`, CB0[127] | `(533, 300, 0.00187617261, 0.00333333341)` | Active dimensions and reciprocals |
| `BufferSizeAndInvSize`, CB0[128] | `(536, 300, 0.00186567160, 0.00333333341)` | Allocated texture dimensions and reciprocals |
| `ScreenPositionScaleBias`, CB0[54] | `(0.4972014725, -0.5, 0.5, 0.4972014725)` | `(533/536/2, -300/300/2, 300/300/2, 533/536/2)` |
| `InvDeviceZToWorldZTransform`, CB0[53] | `(0, 0, 1, -9.99999994e-9)` | Captured reversed-depth projection with the engine's small denominator bias |

The largest bias difference from the matched stock viewport/allocation formula is
`2.002e-8`; reciprocal differences are below `8.072e-11`. Current `ViewToClip` and
`ClipToView` multiply to identity within `2.683e-8` across all six CBs.

The actual directional vertex bytecode derives its camera ray from CB0[44..46]. A local
numeric check used that ray, the captured depth conversion, camera origin/pre-view translation,
CB0[0..3] world-to-clip matrix, and the same screen scale/bias. Four depth samples per CB,
including near viewport corners and centre, reconstruct back to their original pixel centres
within `5.065e-5` input pixels. Reprojected device depth differs by at most `1.071e-8`,
consistent with the engine denominator bias. This connects the relevant ray/depth operands
to their real raster/texture domains rather than inferring coherence from dimensions alone.
The ignored script and full values are `.local/ac7-lighting-20261004/check_scale.py`
and `scale-check.json`.

Actual depth-sampler filter/address state was not recorded; the offscreen probe's sampler
choice is not a readback of game sampler state. The two captures remain exclusively Ultra,
so no measured Quality/native matrix or sampling comparison exists. Shader binding/phase
after the patch and the remaining effect of reduced spatial depth resolution still require
the newer capture. No exact scale/layout correction is supported by the existing values,
and this audit made no native or shader change.

Parent validation: the combined Windows Release gate passed 39 executed tests with six explicit
opt-in skips. The capture correction was deployed at 08:16 in the same pair as the native Reflex
pacing correction, backed up under `.local/deploy-backups/ac7-pair-20261004-081610-248`.
The user was asked for matched close-ground Ultra Performance and Quality F9 captures.
The lighting diagnosis and production correction remain pending those inputs.

## Native integration and deployment, 4 October 08:45

The fresh 08:22 captures resolve the earlier uncertainty: live light values enable contact
shadows (length 0.04, flags 3), the current View phase advances 7/5/2, and the directional
pre/post-light delta introduces the dots. The mask/static-shadow inputs do not already have
that pattern. Both fresh captures remain 533x300 to 1600x900; there is no captured Quality A/B.

The parent integrated the helper into the matched native pixel-shader creation factory at
RVA 0xe31e90, expected `48895c240848896c2410488974241857`, named/saved in GUI Ghidra. It
preserves the serialized resource-table prefix (bits plus five uint32 arrays) and optional
shader trailer, calls the original factory with temporary owned code, and leaves unrelated
shaders unchanged. The helper's patched DXBC CRC32 is 2950909598; capture diagnostics now
recognize that variant too. The exact shader fingerprint and instruction guards protect the
transformation, and all native entry guards match the retained executable dump.

Parent reran disassembly, signature/trailer/fingerprint refusal and WARP/hardware shader
creation checks successfully (`.local/ac7-light-phase-device.log`). The combined native
Release gate passed 39 tests with six explicit opt-in skips, plus Rust format/Clippy. Retail
FG/input device contracts also passed. Deployed 08:45, backup
`.local/deploy-backups/ac7-pair-20261004-084546-571`, with matching proxy/plugin/overlay hashes.
The real overlay Insert/render/resize check passed. This proves shader validity and deployed
source, not the moving-game lighting result; visible acceptance is still pending.

The subsequent user run reports the defect still visible. The full session contains a
transform activation line; the earlier parent statement that it was absent came from an
incomplete filtered view and was corrected. An offscreen execution probe (captured View/light/
depth inputs, explicitly synthetic GBuffer data) verifies that the inserted phase affects the
enabled contact branch. That semantic test does not prove the shader actually bound in the
user's defective draw or that temporal noise alone repairs the image. Existing View/viewport/
projection checks found no scale-layout mismatch. The 09:20 deployment improves the creation
log to report the returned native shader resource and CRC, with no further lighting change.
One post-patch F9 is needed to check bound CRC2950909598, actual phase and the light delta.

## CPU replay of the contact ray, 4 October 10:25

This section corrects the working conclusion above. The missing noise phase is real but is
not what produces the stippling. No shader, capture or runtime code changed here, and the
defect remains unresolved; the user redirected work to hangar pacing before a correction was
written.

Question: why does the directional draw add dots at 533x300 that largely disappear at higher
internal resolutions, and why did temporal noise not remove them?

Method: `.local/ac7-lighting-20261004/raysim.py` replays the shipped ray march of pixel
shader 3619939816 on the CPU, operand by operand from the captured bytecode, using the live
View and light constant buffers and the captured scene depth of all six `contact_light_before`
records in `motion-20261004-082205-100100-1` and `motion-20261004-082226-100100-2`. It
includes two things the earlier notes missed: the tolerance uses a view-Z ray of twice the
contact length, and AC7 fades the contact term by distance with a smoothstep between
`CB1[6].x` and `CB1[6].y`, captured as 1000 and 2000 units. It reads capture files only.

Evidence. The light direction in these captures is `(-0.705, 0.705, 0.0698)`, a sun 4 degrees
above the horizon, so contact rays run almost parallel to the ground. Each ray sample reads
scene depth with a point sampler, which returns the depth at the texel centre rather than at
the sample position. On a surface seen at a grazing angle the depth changes steeply across
one texel, so the returned depth can sit in front of the ray although the ray is above the
surface. The shader counts that as an occluder. With point sampling the replay marks 8,735
to 50,031 pixels as hit per interval; within the faded-in band under an open shadow mask it
marks 61 to 72 percent of pixels in the second capture. The measured light delta agrees:
ranking pixels by measured luminance separates replayed hits from misses with an AUC of 0.88
to 0.99 across the six intervals. The visible pattern is therefore a falsely shadowed ground
with the noise deciding which pixels escape, which is why per-frame noise only made it flicker.
Texel size is three times larger per axis than at native 1600x900, which is the scale dependence
the user observed.

Two corrections were replayed on the same data. Bilinear depth sampling is exact on planar
surfaces and reduces hits to 5,900 to 12,777. A slope-scaled bias, half the sum of the
smaller neighbour depth difference per axis added to the ray depth, reduces them to 5,367 to
12,969 and raises the AUC against the measured delta to 0.98 to 0.995, meaning the remaining
hits are the ones the game also shows as dark: the aircraft's own contact shadow and real
occluders. The bias needs four neighbour depth samples and one changed instruction
(`mov r10.z, r8.z` becomes an add), outside the unrolled eight-sample loop.

Limits. All six intervals are one sun angle and one internal resolution; no native or Quality
capture was replayed. The replay uses real depth and constants but not the GBuffer, so it
predicts the contact term, not final colour. The bias has not been encoded, executed on a
device or seen in game. Sampler state for the depth slot was not recorded in these captures;
the agreement above is consistent with point sampling and was not independently read back.

## Depth quantisation bias in the contact ray, 4 October 13:21

The user asked for the stippling to be fixed now that the pacing work is done. This section
turns the 10:25 CPU replay into a shader correction and checks it on the GPU.

Source check. The replay assumed the depth texture is point sampled. The matched 4.18 source
binds `SceneDepthTextureSampler` to `TStaticSamplerState<SF_Point,AM_Clamp,AM_Clamp,AM_Clamp>`
(`Runtime/Renderer/Private/PostProcess/SceneRenderTargets.cpp`, line 2733). This is source
evidence, not a runtime read: the newest capture (12:41, aircraft viewer) contains no draw of
this light shader, so its recorded sampler state could not be used. UE 4.27.2's
`DeferredLightingCommon.ush::ShadowRayCast` uses the same comparison with no bias, so there is
no later Epic correction to port.

Cause, restated. Each of the eight ray samples compares the ray's depth with the depth stored
at the nearest texel centre. On a surface seen at a grazing angle that centre lies in front of
the ray by up to half a texel's worth of depth change, which the comparison treats as an
occluder. The noise decides which pixels escape, which produces the regular pattern. Texels at
533x300 are three times larger per axis than at native 1600x900, which is why Ultra
Performance shows it most.

Correction. Before the march, the shader now samples the depth at the pixel and at its four
neighbours, takes per axis the smaller absolute difference (the larger one where the smaller is
exactly zero, which is a clamped edge sample), and adds half the sum to the ray's starting
depth. Half the per-texel change on each axis is the largest error point sampling can make on a
plane, so the bias is resolution dependent by construction and vanishes on depth that is flat
in screen space. Taking the smaller side keeps a silhouette's depth jump out of the estimate.
Five extra point samples per lit pixel inside the contact branch only.

Encoding. `games/ac7/src/contact_shadow.cpp`, now `rsf_ac7_contact_shadow_correct`, applies
both the 08:45 noise phase and this bias to the shader with SHA-256
`60a2ca61e5d83b853f692ee8c0bc3580ea46b8e33d72c644949e5f37c1060a12`. It additionally requires the
original tokens `05000036 00100042 0000000a 0010002a 00000008` (`mov r10.z, r8.z`, the ray start)
at token 996, inserts 169 words before it and replaces it with `add r10.z, r8.z, r15.x`.
Temporaries r15 to r17 are new; `dcl_temps` becomes 18. The block was encoded by
`.local/ac7-lighting-20261004/encode_bias.py`, whose operand encoder reproduces the shader's own
`mov r10.z, r8.z` exactly. The container grows from 10456 to 11212 bytes; the transformed
CRC-32C is 4154163049, which the capture predicates now recognise in place of 2950909598. The
creation log line reports the CRC computed from the transformed bytes.

Checks, all offscreen on this machine's RTX 4070 Laptop GPU and WARP:

- `probe_contact` (ignored local fixture): disassembly contains `dcl_temps 18`, the phase
  instructions, the centre sample, the edge fallback and `add r10.z, r8.z, r15.x`; WARP and
  hardware `CreatePixelShader` return S_OK; a trailer is retained; a one-byte change and a
  truncated input are refused.
- `bias_probe` runs the original and corrected shaders with the captured View and light
  constants, scene depth, light mask and static-shadow inputs of all six 08:22
  `contact_light_before` records, and synthetic GBuffer data as in the 09:00 probe. With the
  contact length set to zero the corrected shader's output is bit-identical to the original on
  all 160800 pixels, so nothing outside the contact branch changed. With the captured contact
  length 0.04, pixels whose light is cut by more than half fall from 3641, 7925, 17567, 38844,
  39384 and 21287 to 1542, 3670, 8030, 1314, 1440 and 2398.
- Images of the contact term (`.local/ac7-lighting-20261004/bias-all.png`) show the ground
  stipple and the false shadow bands toward the horizon gone in all six frames, and the
  aircraft's own contact and self shadows, bushes and rocks retained. A first version left one
  dotted row at the bottom edge because the clamped neighbour was compared with the
  reconstructed ray depth instead of the centre texel; comparing with a centre sample removed
  it (0 partially shadowed pixels in the last 20 rows, against about 300 per row originally).

Limits. All frames share one sun angle and one internal resolution. The GBuffer is synthetic,
so the replay shows the contact term, not final colour. The bias is a derived correction, not
an engine one; on steep, finely detailed geometry it can lift genuine sub-texel contact
shadows. Moving-game appearance at Ultra Performance and at Quality is not yet seen.

Deployed at 13:21, backup `.local/deploy-backups/ac7-pair-20261004-132153-981`.
