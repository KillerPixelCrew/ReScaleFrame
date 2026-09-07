# AC7 frame capture

Evidence from 6–7 September 2026: 22 RenderDoc captures under Proton, Windows replay, live buffer dumps, and subsequent reduced-scale DLSS runs. Scenes included aircraft selection, camera pans, flight over ground/water, clouds, and screen droplets. Resource numbers below identify these captures, not objects that can be looked up by that number at runtime.

## Current result

The research proxy runs DLSS with decoded sparse velocity, depth, jitter, and camera data read from the engine. A 7 September mission run recorded 7,917 evaluations without refusal at 1024×576 input and 2048×1152 output. The reported flight image included sky, clouds, terrain, and aircraft without obvious smearing. This is useful game evidence, not a controlled proof of vector units or every effect.

F7 displays the reconstructed scene over the back buffer with a rough tonemap. The output is not yet reinserted before the game's grading and HUD composite.

The briefing screen's terrain relief now reaches the reconstruction, game-tested on 7 September, once the backend was given the composed colour rather than the colour bound at the tapped pass. The relief still retains input pixelation under the continuously panning briefing camera, as reported on 7 September. It writes no depth or velocity of its own. The conditional [translucent-depth replay](ac7-translucent-depth.md) is synthetic-tested; its effect on briefing reconstruction remains game-unverified. See also [composed scene colour](ac7-composed-scene-color.md).

## Capture workflow

The Windows RenderDoc DLL captured D3D11 calls through DXVK under Proton. In the recorded setup, Linux RenderDoc could not parse D3D11 chunks. Windows `renderdoccmd.exe` converted them under Wine without a replay device:

```bash
wine renderdoccmd.exe convert -f capture.rdc -o capture.xml -c xml
python3 tools/parse-capture.py capture.xml --timeline
```

A 111 MB capture produced about 4.4 MB of XML. Pixel inspection used Windows replay. The parser groups draws by render-target bindings; its inherited-SRV and compute-pass tracking is incomplete. Use its output to locate candidates, then inspect the actual draw/shader/resource data.

## Resources and frame structure

The original captures all presented and rendered at 2048×1152. They contained 144–153 texture creations, 4,562–5,158 API chunks, and 94 render-target bindings in the hangar or 125 in flight. Full-size allocations included scene colour, depth/stencil, RGBA16F intermediates, and byte-format composites. Flight added shadow targets at 5120×1024 and 2048×2048. Smaller scene-colour targets formed the bloom/downsample chain.

| Resource | Format | Size | Observed role |
| --- | --- | --- | --- |
| 2163 | `R16G16_UNORM`, SRV + RTV | 2048×1152 | Sparse scene velocity |
| 63083 | `R16G16_UNORM`, SRV + UAV | 1024×576 | Mask; **not velocity flattening** |
| 2111 | `R10G10B10A2_UNORM`, SRV + RTV | 2048×1152 | GBuffer normal candidate |
| 2052 | Depth resource | Full scene size | Depth bound with temporal inputs |
| 62725 | `R32G32_FLOAT` | 1×1 | Exposure candidate; units still need verification |

In `ac7_frame52376`, chunk 2016 begins a ten-draw GBuffer pass; chunks 2278–2413 include shadow draws; chunk 2518 begins ten draws into velocity #2163 with depth. Chunk 4448 writes 1×1 #1628, followed by a downsample/upsample ladder. Chunk 4864 writes the swap-chain back buffer #268 in one draw. A 1920×1080 target #1208 receives 42 earlier draws; its role remains unresolved.

The parser found velocity-consuming draws at chunks 3257 and 3275, writing RGBA16F #1732 and #2168. The latter binds #1732, history #2172, velocity #2163, depth #2052, and exposure candidate #62725. This is strong evidence for temporal filtering and a useful UE4 TAA lead. Shader/function identity still needs verification in AC7's custom branch; format and bindings alone do not uniquely name a pass.

## Velocity encoding and camera motion

The inspected UE4.18 `Common.ush` defines:

```text
stored = velocity * 0.2495 + 32767/65535
velocity = (stored - 32767/65535) / 0.2495
```

Raw zero means unwritten, while encoded zero motion is near 0.5. Test the sentinel before decoding. Written pixels averaged 0.4999928 in replay, agreeing with the source bias of 0.4999924. The largest recorded raw value, 32813, decodes to about 0.0028 screen-space units, roughly 2.9 horizontal pixels at the captured width. Small observed motion cannot determine the scale reliably.

| Capture | Written coverage | Scene |
| --- | --- | --- |
| `ac7_frame32137` | 16.4% | Menu camera pan |
| Clear-sky flight | 4.7–8.3% | Mainly aircraft |
| `ac7_frame36073`, `ac7_frame36392` | 52%, 58% | Ground visible |

The remaining 83.6% of the menu-pan texture is exactly zero despite camera movement. The field therefore does not provide dense camera motion. Stock UE4's temporal consumer computes camera displacement from depth and `ClipToPrevClip`, then replaces it at written velocity pixels. Whether AC7's written vectors already contain camera movement must be checked before adding any camera contribution to them.

The current compute pass decodes into float motion and uses a separate invalid sentinel so valid zero motion remains distinguishable. The DLSS adapter submits `cameraMotionIncluded = false`, the decoded sentinel, depth, and the engine transform for Streamline's resolve. Another dense-motion pass is optional work for diagnostics or backends that need it, not a prerequisite for this DLSS path.

Remaining checks: clip versus UV/pixel units, temporal direction, vertical sign, jitter inclusion, dilation, sky depth, and independently moving geometry. The Rust conversion uses half the viewport extent and a vertical flip; the live path defaults scales to one. See [the motion todo](../implementation.md#ac7-motion-and-velocity-improvements).

## Jitter and render scale

Before patching, the sampled perspective views had zero jitter despite the temporal-filter candidate. The research proxy bypasses the AA-method gate at RVA `0x112b1f3`, allowing the engine's own jitter setup to run. Recorded horizontal offsets after patching were −0.147, +0.065, and −0.430 pixels, within half a pixel.

`TemporalAAJitter` was located by comparing pre-patch and post-patch dumps. Four floats at `0x720` hold current X/Y and previous X/Y in clip space. The current pair exactly matches row 2, columns 0/1 of `ViewToClip`; corresponding previous fields agree across captures. The dataset included thirty older buffers and twenty newer ones that overwrote the low-numbered files, so treating the directory as one run hid the change.

For a view rectangle of width W and height H:

```text
jitter_pixels = (clip_jitter_x * W/2, clip_jitter_y * -H/2)
```

The reader removes projection jitter because UE4.18 stores no `ViewToClipNoAA` copy. That does not by itself establish that `ClipToPrevClip` meets the backend's unjittered convention; test a stationary camera with changing jitter.

`r.ScreenPercentage = 50` changes actual scene allocations to 1024×576 while presentation remains 2048×1152. The researched console-manager singleton is at RVA `0x3a8b290`; `FindConsoleVariable` uses vtable byte offset `0x90`. The screen-percentage value was found at `0x68`, with game-thread and render-thread copies. These addresses are build-specific.

Mission loading restores the game's scale setting, so the current proxy reapplies the requested scale on a timer. It also sets jitter sample count: eight at full scale, 32 at half. Resizing allocations alone was rejected because engine size/UV constants would remain inconsistent.

## HUD and output reinsertion

At full scale, replay showed this sequence:

| Stage | Resource | Contents |
| --- | --- | --- |
| Scene copy | 56115 → 2123 | Identical `B8G8R8A8` scene colour |
| Fullscreen pass | 2123 → 64363 | HUD-less scene; edge-darkening pattern consistent with vignette |
| HUD layer | 2181 | `R8G8B8A8` UI on transparent background |
| Final composite | 268 | Graded scene, HUD, and HUD glow |

The fullscreen pass changed 33.6% of pixels by an average of 7/255; mean change rose from 2.5 at the centre to 11.1 at the edge. This supports the vignette interpretation, without identifying its shader.

Full-scale captures could not establish which targets follow render resolution. At 50%, chunk 6714 composites scene, HUD, and glow into 1024×576 #64091; chunk 6732 samples that single composite into 2048×1152 back buffer #306. The interface is therefore scaled with the scene in this path.

Reinsert the reconstructed scene before the composite, make downstream allocations and rectangles agree with output resolution, and preserve the game's grading/effects. Stock UE4 TAA inherits its output extent from its input, so swapping a texture pointer alone is insufficient. Trace both standalone spatial scaling and scaling merged into tone mapping before bypassing either.

The current resource match occurs before roughly 27 later sky/cloud draws into the same colour target. Early evaluation omitted the sky. Holding the selected resources and evaluating at Present produced the complete diagnostic image. Recheck input overwrite boundaries when moving evaluation earlier; an AddRef preserves allocation lifetime only.

Chunk 6714 is a run of draws sharing one output binding, not a single draw, so "before the composite" is not yet an address. Which of those draws reads scene colour decides where the substitution goes. The captures here cannot answer that, because the exported action list records render-target bindings and not shader resource bindings. The question is asked of the running game instead: `rsf_frame_tap_watch_target` names a render target and the frame tap reports the next few draws into it with their pixel shader inputs, slots, sizes, viewport, and ordinal within the pass. The loader points it at the back buffer, takes the single texture that draw reads as the composite, watches that in turn, and marks scene colour where it appears. Nothing is altered. Not yet run against the game; this records the instrument, not an answer.

The substitution that answer feeds is built: the composite and the interface's own target are replaced by output-resolution textures of the same format, scene colour is replaced by the reconstruction, and viewports and scissor rectangles are scaled while a replaced target is bound. Two parts of it follow from this document rather than from the code. Scene colour is substituted only after the composite has been bound in the frame, because the scene passes read scene colour while they are still writing it and the sky arrives roughly 27 draws after the pass that binds the reconstruction inputs. The reconstruction also runs at that gate rather than at Present: Present was right while the result was only drawn over the top, but the gate is the last point inside the frame before anything reads scene colour and the first where the scene is whole. The interface target is picked out by format, since 2181 is `R8G8B8A8` and the tail's scene targets are `B8G8R8A8`; that is one observed difference in one game, so an input that does not match leaves the interface magnified with the scene rather than promoted on a guess.

## Menu screens render two scenes, and only one is reconstructed

Captures `ac7_briefing_frame18425` and `ac7_hangar_frame51463`, 7 September 2026, taken at native scale with the proxy loaded but DLSS not started.

In the briefing screen the reconstruction showed the coastline and the unit markers, and the mission-area relief was absent. It is absent from the DLSS input, not lost in the upscale: the dumped 1024×576 input and the 2048×1152 output hold the same content, so the backend reconstructed exactly what it was given.

The frame contains two separate 3D renders:

| Events | Target | Format | Draws / indices | Contents |
| --- | --- | --- | --- | --- |
| 1092–1446 | 1723 | `R11G11B10_FLOAT` | 24 / 21,033 | Deferred base pass and lighting: coastline and markers |
| 1486–2015 | 46630 | `R16G16B16A16_FLOAT` | 59 / 540,030 | Contour relief, dotted terrain grid, markers |
| 2146 | 46633 | `R11G11B10_FLOAT` | 1 / 3 | Fullscreen composite of both |

The tap recognized three qualifying passes in this screen and holds the last, since `on_pass` replaces what it holds. All three carry the first layer. The relief pass never qualifies, so no choice among qualifying passes can reach it.

Selection by resource id or by allocation age would be wrong. The complete target is 46633 in the briefing and 1723 in the hangar, where 60094 is allocated later and stays black. Across both captures the only consistent signal is write order: the last-written full-size scene-colour-format target holds the complete image.

Consequences. A menu screen is not a smaller version of the flight frame, and reconstructing its first layer alone will always drop content. The second layer carries no velocity, so a temporal backend cannot reconstruct it from these inputs even if it were tapped. Either the composite result is what gets scaled in these screens, or reconstruction is declined there and they run at native. Nothing here says which, and neither has been tried.

### Why the reconstruction misses it, and where the whole scene already exists

Source read of 4.18.3 `PostProcessing.cpp`: separate translucency is recombined into the post-process chain at line 1453, through `FRCPassPostProcessBokehDOFRecombine`, and temporal AA is added at line 1467. **The composition happens before temporal AA, not after it.**

So a composed colour, scene and relief together, exists in the frame before the pass this integration taps. The capture identifies it: target 46633, one fullscreen draw at event 2146 with three indices and no depth bound, full size `R11G11B10_FLOAT`, holding the complete image. Its inputs are the base pass output 1723 and the separate translucency target 46630.

That is why the relief is missing from the reconstruction, and motion vectors were never the reason. The backend is handed the colour one step too early in the chain. Giving the icons velocity, which now works, does not put the layer into the input; only taking the composed target does.

The tap accepts a pass by the set it binds, colour with depth, velocity and a 1x1 exposure, and takes the colour bound there. Nothing in that rule distinguishes a colour before the recombine from one after it.

What would: the recombine is a fullscreen draw that reads the colour already identified and writes another full-size colour target. Recognising it needs the tap to answer "which draws read this texture", where today it answers "which draws write this one". The shadow it already keeps has the information. The [composed-colour change](ac7-composed-scene-color.md) adds that query and conditional backend selection; it is synthetic-tested, with game verification still open.

### Game-tested: separate translucency was rendering at half resolution, and that was the whole thing

7 September 2026. Patching the halving out of `SetSeparateTranslucencyBufferSize` fixed the briefing relief and the cannon tracers at once. The user's words were day and night, with only minimal smearing left.

The layer was rendering at 512×288 against a 1024×576 scene and being doubled into the composite before anything downstream saw it. Every reconstruction was handed a picture in which that content was already a 2× blow-up, and no motion, depth or backend recovers detail that was never drawn. Counters after the patch: candidate draws at 1024×576 rather than 512×288, and the depth replay went from 0 replayed to 167,374 of 170,319.

This retires a chain of explanations that were each true and none of which was the cause. Translucency does lack velocity, the relief is a static mesh actor that cannot enter the velocity pass, and separate translucency does bind depth read-only. All of that is correct and none of it was why the terrain looked unupscaled.

It also corrects the section below. Cannon tracers improving means flight uses this layer when translucent effects are active; the flight captures examined were taken at native scale and mostly without effects firing, so the layer was simply not allocated in them. Absence in those captures was read as absence in flight, which was too strong.

### Game-tested: the layer's resolution is a variable, and the briefing draws at native

7 September 2026. The briefing relief renders at 2048×1152 inside a 1024×576 scene and reaches the
reconstruction. The user's report was that it looks great and is upscaled. The run recorded the
whole chain: the four depth gates applied, the scale immediate carried at `0x1410BE330`, the layer
recognised as heavy at 537,168 indices against a threshold of 100,000, and `separate translucency
scale now 200% of the scene, which at a 50% render scale is 100% of native (heavy layer)`.

The depth followed it, which is the thing that had failed: `last draw 2048x1152 ... depth view ...
its texture 2048x1152`, where the previous run read `1024x576` for that last field. Replay went
from 22 of 1,497,917 candidate draws to 733,861 of 744,726. A capture of the output at 2048×1152
records `fraction_unwritten` 0.0000 and shows the terrain contours as clean single-pixel curves.

Rendering the layer at the scene's resolution was not enough. The user reports the briefing's cloud-coverage point cloud almost completely eaten, and it already looks poor at the stock 50% render scale, so the layer wants to be larger than the scene rather than equal to it. The right multiplier is not a constant either: the scale multiplies the scene buffer, so what it is worth depends on the render scale, and DLSS, XeSS and FSR each choose their own render scale per quality level. A fixed 1.0 means a different translucency resolution for every one of them.

The first patch loaded the 1.0 four bytes past the 0.5 in the same constant pool. That pool holds 0.5, 1.0, π/2, π, 2π, 90, 180 and 270 at `0x142574490` onward and has no 1.5 or 2.0, and re-targeting the displacement to a constant found elsewhere would only trade one fixed value for another.

Carrying the value inside the instruction answers both. `FSceneRenderTargets_SetSeparateTranslucencyBufferSize` at `0x1410be2b0` shows a free register across the patched window:

```
1410be329  73 0D                     jnc   0x1410be338
1410be32b  40 84 FF                  test  dil, dil
1410be32e  74 08                     jz    0x1410be338
1410be330  F3 0F 10 0D 58 61 4B 01   movss xmm1, [0x142574490]   ; 0.5
1410be338  66 0F 6E 83 08 02 00 00   movd  xmm0, [rbx+0x208]     ; does not read eax
1410be346  48 8B 83 08 02 00 00      mov   rax, [rbx+0x208]      ; overwrites rax
```

Every path out of the window reaches those two, so `eax` is dead across it. The fifteen bytes at RVA `0x10be329` become:

```
66 0F 1F 44 00 00         nop  word ptr [rax+rax*1]
B8 xx xx xx xx            mov  eax, <scale bits>
66 0F 6E C8               movd xmm1, eax
```

The six-byte nop leads deliberately, so the immediate lands at RVA `0x10be330`, four-byte aligned. That is what makes the scale adjustable while the game runs: the proxy changes it with one aligned store, which a render thread reading the instruction either sees or does not, never half of. An overridden RVA that would leave the immediate unaligned is patched but not registered, keeping its startup value.

The settings are percentages of the presented resolution rather than of the scene, and the multiplier is derived from whichever render scale is in effect. `RSF_TRANSLUCENCY_TARGET` defaults to 0, meaning match the scene, which is exactly the behaviour above. `RSF_TRANSLUCENCY_TARGET_HEAVY` defaults to 100, meaning native.

Heavy is decided by how much geometry went into the layer in one frame, because the briefing relief and a burst of cannon tracers are the same kind of surface to the engine and differ only in quantity: the captured briefing layer is 59 draws and 540,030 indices, and the flight effects are orders of magnitude below that. The threshold is `RSF_TRANSLUCENCY_HEAVY_INDICES`, default 100,000, and the count is logged on each crossing and in the counter report so it can be set from what the game draws.

Two limits, both real. The count comes from the frame tap, which is installed when the backend starts, so a briefing reached without starting one reports zero and keeps the ordinary scale. And the chosen scale reaches the next frame's buffer allocation rather than the current one, which is invisible across a briefing and is what stops a short tracer burst from causing a resize each way.

Whether a layer above the scene's resolution composites correctly is untested. `SeparateTranslucencyScale` at `[rbx+0x220]` follows the same xmm1 the width and height do, so the recombine's UV maths is given a consistent scale, but every value the engine itself produces there is at most 1.0.

### Game-tested: 4.18 cannot bind a depth for a layer above the scene, and the ceiling is five gates

7 September 2026. A heavy target of 100 gave a scale of 2.0 at a 50% render scale, and the briefing relief disappeared entirely. The HUD, the radar and the text were untouched; only the translucent layer was gone.

The instrumentation named the cause without another run. The layer was drawn: `translucent depth: last candidate 2048x1152`, and the heavy detection was exactly right, `540030 indices in 59 draws`, matching the capture. The next field is the fault: `depth view format 20 dimension 3 flags 0x1, its texture 1024x576`. A 2048×1152 colour target paired with a 1024×576 depth-stencil is an invalid pair in D3D11, so nothing drew.

Four gates in 4.18 test the same expression and take the borrow branch above 1.0:

| Source | What it decides above 1.0 |
| --- | --- |
| Source | RVA | Bytes | What it decides |
| --- | --- | --- | --- |
| `TranslucentRendering.cpp:1258` | `0x1168f6f` | `76 0E` jbe | skips `SetupDownsampledTranslucencyViewUniformBuffer`, so the sized depth is never allocated or filled and no scaled view uniform buffer is built |
| `SceneRenderTargets.cpp:1373` | `0x1097a0c` | `76 17` jbe | binds `GetSceneDepthSurface()` instead of the sized depth |
| `SceneRenderTargets.cpp:1400` | `0x109d03a` | `cmova r15d,ecx` | picks the scene depth to resolve, on the snapshot branch |
| `SceneRenderTargets.cpp:1405` | `0x109d061` | `76 17` jbe | the same, on the other branch |

`TranslucentRendering.cpp:94` and `:580` test it too but need no change here. The log proves `:94` already passes, because the layer was allocated and drawn at 2048×1152; the relief goes through the AfterDOF path, which does not consult it. `:580` only selects which view uniform buffer the draws use, so it costs accuracy in scene-texture lookups rather than the layer itself, and it lives in templated code with many instantiations.

The fix is to ask `Scale == 1.f` instead, because borrowing the scene's depth is correct only at exactly 1.0. `jbe` becomes `je` and `cmova` becomes `cmovne`, one byte each. Everything else the engine already does correctly at any scale: `DownsampleDepthSurface` takes the factor as a parameter and derives its viewport and rectangle from it, so at 2.0 it upsamples, and `SetupDownsampledTranslucencyViewUniformBuffer` rebuilds the view from `ScaledSize` and `ViewRect * scale`. No new shader, hook or resource.

These four are provable no-ops for stock behaviour. With `comiss 1.0, scale`, `je` and `jbe` agree at exactly 1.0, where both act on ZF, and agree below 1.0, where neither fires; `cmovne` and `cmova` agree the same way. They differ only above 1.0, a state only the scale patch can reach. That is also how the direction of the `:1258` gate was settled without another lookup: the stock briefing runs at 0.5 and its setup call must run, which only holds if the register being compared is 1.0.

The ceiling therefore follows the patches rather than a setting. All four apply or none do, since a partial application would leave the engine allocating a depth it does not bind, and a refusal keeps the scale at the scene's resolution.

An earlier revision of this section called the above-1.0 case impossible without patching five gates and running an upsample the engine never does. That was wrong on both counts and is recorded because the reasoning failure is worth not repeating: the obstacle was described before the options were enumerated.

### Flight captures do not show this layer, which is not the same as flight not having it

All 22 flight captures were rescanned for the same shape on 7 September 2026. None has a separate translucency layer.

In `ac7_frame52376` the heaviest colour target is 56121, full size `R11G11B10_FLOAT`, taking 44 draws and 206,216 indices: translucency is drawn straight into scene colour along with everything else. The only other full-size `R16G16B16A16_FLOAT` targets are 1732, 2168 and 2172, which the temporal-filter work above already identified as that pass's input, output and history, and none of them receives geometry. The same holds across the other 21.

So the two-layer composite belongs to the menu and briefing screens, where a holographic overlay is drawn as translucency over a nearly empty scene, and not to gameplay. Selecting the composed colour must therefore fall back to the colour bound at the pass when no recombine is present, or it would change the flight path, which works, in pursuit of a screen that does not.

It also rules out one thing for flight generally: there is no second scene render there to be left out of the reconstruction. The flight image already contains the sky, the clouds, the terrain and the aircraft, which holding the resources and evaluating at Present fixed. What remains open about clouds, contrails and glass is their motion, whether the vectors describing them are right, and that is untouched by any of this.

Unresolved: whether the relief pass writes depth or velocity at all, which needs its shader resource bindings rather than the render-target list used here.

## Masks, clouds, and droplets

Resource #63083 was initially called velocity flattening because it was half-size, two-channel, and compute-written. Replay showed a mean near 0.55, no zero clear, a bimodal distribution, saturated sky, and an aircraft cutout. It is a mask. Its allocation neighbours include cloud resources 62989–63019, but its exact producer still needs identification.

Resources #57437 (`BC3`, 1024×1024) and #57439 (`BC5_UNORM`) resemble a droplet mask/normal pair. Both were resident in all 19 flight captures, including dry frames. Residency does not prove use. Locate their consuming draw before placing refraction after SR or assigning a motion convention.

Cloud, smoke, glass, and contrail motion need separate analysis. Opaque depth cannot describe every transparent layer.

## View-buffer layout

The 4096-byte allocation contains the following verified fields. Stock UE4.18 source supplies context; offsets are validated against captured relationships, not copied from a later engine layout.

| Offset | Field | Use |
| --- | --- | --- |
| `0x180` | `ViewToClip` | Projection and FOV |
| `0x1C0` | `ClipToView` | Inverse projection |
| `0x200` | `ClipToTranslatedWorld` | Reprojection identity |
| `0x300`, `0x310`, `0x320` | `ViewForward`, `ViewUp`, `ViewRight` | Camera basis |
| `0x350` | `InvDeviceZToWorldZTransform` | Depth convention |
| `0x370` | `WorldCameraOrigin` | Camera position |
| `0x3A0`, `0x650` | Current/previous `PreViewTranslation` | Origin-shift correction |
| `0x4F0` | `PrevTranslatedWorldToClip` | Previous projection transform |
| `0x6E0` | `ClipToPrevClip` | Engine camera reprojection |
| `0x720` | `TemporalAAJitter` | Current/previous jitter |
| `0x7E0` | `ViewRectMin` | View rectangle origin |
| `0x7F0`, `0x800` | View/buffer size and reciprocals | Extents and main-view checks |

The strongest cross-check ties five fields together:

```text
ClipToPrevClip = ClipToTranslatedWorld
              * T(PrevPreViewTranslation - PreViewTranslation)
              * PrevTranslatedWorldToClip
```

The recorded identity agreed to about a millionth while the camera moved up to 43 units. Omitting the translation delta happened to agree in still captures and failed once the camera moved. Basis vectors also match rows of `ViewToTranslatedWorld` and have unit length.

Captured projections use reversed Z, near 1.0, and infinite far; `(0, 0, 1, 0)` in the depth transform agrees. Vertical FOV varied: 38.0° in flight, with 58.7° and 33.4° elsewhere.

Secondary viewports included 1016×1016, 128×93, and 128×111 within larger buffers. Do not select the camera merely by buffer size or creation order. The current main-view heuristic compares view and buffer extents; it still needs explicit frame/view association.

The Python layout check recorded 11 perspective views and no failures. The C++ reader's recorded run recognized 50 buffers, accepted ten perspective views, and marked secondary views separately. These are different checks, not interchangeable pass counts. [verify-view-layout.py](../../tools/verify-view-layout.py) reruns the offline relationships; [the review](../review.md) records validation gaps.

## Corrections retained from earlier analysis

| Earlier claim | Evidence and current conclusion |
| --- | --- |
| Decode with `(value - 0.5) * 2` | Engine source gives bias `32767/65535` and inverse scale about 4.008 |
| #63083 is velocity flattening | Pixel contents identify a mask; format/binds were insufficient |
| A separate HUD removes the reinsertion problem | Reduced-scale replay shows the HUD composite also shrinks |
| DLSS can consume the raw velocity texture | Its scale cannot remove UE4's storage bias; decode first |
| All camera motion must be implemented again | The engine already supplies `ClipToPrevClip`; use it with depth |
| A still-image success validates motion | Controlled camera/object sequences are still needed |
