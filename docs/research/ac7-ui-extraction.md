# AC7 UI extraction

Design record for the interface work in the [representation plan](../representation-plan.md),
written 7 September 2026 from the findings in [ac7-ui-composition.md](ac7-ui-composition.md), the
UE 4.18.3 source, the AC7 SDK, and Skyrim Community Shaders. Per-screen measurements are appended
per milestone; the status section says what has and has not run.

## What the interface is, per screen

Three producers exist in a 4.18 game, and AC7 has at least two of them:

| Producer | How it draws | Resolution it lands at | Alpha from a transparent clear |
| --- | --- | --- | --- |
| Slate window draws (`FSlateRHIRenderer::DrawWindow_RenderThread`) | `FSlateVertex` batches into the viewport target or the back buffer | native | accumulates: alpha ops `BO_Add, BF_One, BF_InverseSourceAlpha` (`SlateRHIRenderingPolicy.cpp:726-732`) |
| World-space widget quads (`UWidgetComponent`, `Widget3DPassThrough_Translucent`) | two triangles reading the widget's 1920x1080 render target, base-pass translucent blend, depth-tested against the scene | render resolution, into AC7's own `R8G8B8A8` UI layer (`ac7-ui-composition.md`, tail order) | **stays 0**: alpha ops `BO_Add, BF_Zero, BF_InverseSourceAlpha` (`BasePassRendering.h:1105`) |
| Canvas HUD (`AHUD::PostRender`, `FCanvas`) | `FSimpleElementVertex` batches after the scene | native | to be verified against `BatchedElements.cpp` |

AC7 rasterizes its front-end interface at a hardcoded 1920x1080 through
`Nimbus.WidgetToTextureConverter` (`DrawSize` at `+0x28`; constants at `0x1425f1cac` and
`0x1425f1ce4`), so the second row is the case on the briefing and the hangar, and the reason those
screens look soft at a reduced render scale: the quads are rasterized by the scene at the scene's
resolution. The SDK's `UWidgetComponent` carries AC7's additions `bEnableDownsampleRenderTarget`
(`0x0828`) and `DownsampledRenderTargetArray[2]` (`0x0830`), which are the alternating layer targets
the run log shows. Which screens use Slate directly, and whether any canvas draws exist, is what the
plan's M1 classification run answers.

## Identification

Registries filled at resource creation (allocation watch hooks on `CreateInputLayout`,
`CreateVertexShader`, `CreatePixelShader`, `CreateTexture2D`; records attached with `SetPrivateData`
so pointer reuse cannot alias):

- Slate input layouts: `R32G32B32A32_FLOAT@0, R32G32_FLOAT@16, R32G32_FLOAT@24, B8G8R8A8_UNORM@32,
  R16G16_UINT@36` (`SlateShaders.cpp:51-62`), five elements, or six with the instanced variant;
  matched by format, offset and slot, never semantic name; stride 40 checked at draw time.
- Widget render targets: `B8G8R8A8` typeless or UNORM, render target and shader resource, one mip,
  one sample, extent in the configured draw-size list (1920x1080 by default). A Slate-layout draw
  *into* one of these is the converter rasterizing a widget and is never diverted.
- Shader hashes: CRC32C of the bytecode, with force and skip lists in the settings, the SpecialK
  mechanism (`d3d11.cpp:3297-3313`) retargeting instead of skipping.

The classifier lives in `games/ac7` because which draw is interface is a game fact. Order: override
table; Slate or canvas layout into the back buffer; a six-index draw with one target, no unordered
access views, a widget render target in a low pixel-shader slot, a target that is neither a widget
render target nor the back buffer, and a translucent blend; otherwise scene. Every rule of the form
"the one that matches" failed in this frame at least once, so the classifier compares against
everything the shadow holds and never against an address.

## Divert

The tap performs the divert inside its draw hooks, before forwarding, through the original context
methods so its own shadow stays the game's: save the bound target, depth, viewports, scissors and
blend state from the shadow; bind the layer's render target view with no depth view; scale the
viewport per producer (1.0 for Slate and canvas into the back buffer, `layer / target` for a quad
into a render-resolution target); swap in the patched blend state when the rule says so; forward;
restore. The blend patch is Skyrim Community Shaders' `GetPatchedAlphaBlendState`
(`HDRDisplay.cpp:928-1006`) applied per diverted draw: for an "over" blend
(`DestBlend == INV_SRC_ALPHA`) the alpha operations become `ONE / INV_SRC_ALPHA / ADD` and the colour
operations stay; Slate needs nothing; additive blends stay (alpha 0 is a pure add under a
premultiplied composite); modulate cannot be represented and is counted.

Depth: the briefing quads bind the scene's depth view. Diverting them with no depth view disables
the test, which is right for an overlay and wrong wherever scene geometry sits in front of a panel.
"Divert only draws without depth" would extract nothing on the briefing, so the layer binds no
depth by default and a promoted depth built from `depth_replay`'s output-resolution copy is the
later option, selected per screen.

## The layer, HUD-less colour and the composite

The layer is `R8G8B8A8_UNORM` at back-buffer extent, premultiplied, cleared to transparent after
every Present, never bound with a depth view, shared with the D3D12 side once the bridge exists.
HUD-less colour is a copy of the graded back buffer taken before the composite; with the quads
diverted the game's own layer is transparent and the back buffer is HUD-less by construction. The
composite is `Final = UI.rgb + (1 - UI.a) * Scene.rgb` with `ONE / INV_SRC_ALPHA`, which is the
formula all three frame generation SDKs state ([vendor contracts](vendor-fg-contracts.md)) and the
one UE's own `CompositeUIPixelShader.usf:101-103` uses. Exactly one compositor runs per rendered
frame: ours when frame generation is off, the vendor's on generated frames. egui draws into the
layer after the game's interface so it rides both.

The engine's own `r.HDR.UI.CompositeMode` path exists in the binary and does the Slate half of this
([hook map](ue418-hook-map.md)); it is recorded and not used, because it does not touch the quads.

## Reinsertion after this

Promoting the interface target is gone. `scene_promote` keeps promoting the composite and the
scene colour, and adds the chain targets between the tonemap and the back-buffer draw
(`0x308E2770` in the log) that reinsertion had been leaving at render resolution.

## Status

Design only; nothing here has run. M1 classifies without changing anything and fills the per-screen
table below. M2 diverts and composites with frame generation off. Each run's summary lines are
appended here.

### Per-screen classification

Not yet measured under the corrected rules. The first run, 7 Sep 2026, is recorded in
[the implementation tracker](../implementation.md); its counts are superseded because two of the
rules that produced them were wrong.

### Measured: extraction works, and what it costs

7 Sep 2026, third extraction run.

The interface reaches the screen at native resolution. 59584 draws diverted with zero refusals,
3605 frames written to the layer and 3605 composited. This is the thing promotion could not do, and
it is done.

Two things were wrong on the way and both are recorded because both were instructive.

The composite read the frame tail walk's back buffer field, which that walk holds for thirty-two
frames and then deliberately releases, because a held swap chain reference makes `ResizeBuffers`
fail. So the interface was diverted out of the scene and never put back: it vanished rather than
moved, and nothing said so because the failing path returned null quietly. The composite now asks
the swap chain itself every present.

The colour was wrong twice, in opposite directions, before the measurement settled it. The widget
quads write into **view format 28**, plain `R8G8B8A8_UNORM`, so nothing encodes them on the way in;
the converter filling the widget texture they read uses **view format 91**,
`B8G8R8A8_UNORM_SRGB`, so they receive decoded colour and store it linear. AC7's interface target
therefore holds linear values that a later pass transforms for display. Encoding at the layer was
wrong; the transform belongs at the composite, where the back buffer already holds transformed
colour. With the sRGB curve applied there the picture is close.

### Open: the glow does not travel with a diverted draw

What remains is that the extracted interface has none of the game's glow. AC7's converter produces
more than one texture per widget: `UWidgetToTextureConverter` holds `RenderTarget` at `0x48`,
`DownSampleRT` at `0xC0`, `BlurXRT` at `0xC8`, `BlurYRT` at `0xD0` and `RenderTargetWithGlow` at
`0xD8`. The quads have six pixel inputs, and which of those textures they are is what decides where
the glow is applied and therefore what it would take to keep it. The trace now prints every input
for this reason; before the next run this is inference and not a finding.

### Measured: the quads are not overlays, and this is why extraction discolours the frame

7 Sep 2026, fourth and fifth extraction runs, with screenshots.

The blend was measured and it is not the problem. AC7's interface blend is already
`ONE / INV_SRC_ALPHA` on both colour and alpha with write mask `0xf`: already premultiplied,
already writing coverage. The alpha patch is a no-op on it, correctly, and the layer accumulates
what it should.

What the trace shows instead is the inputs. A diverted quad reads slot 0 at 1920x1080, which is the
widget texture, and slots 1, 2, 5 and 6 at 1024x576, which is the render resolution. Those are
scene-sized buffers: the draw samples the scene and the blur and glow chain built from it. It is not
an overlay that could be moved anywhere; it is a composite that belongs where it is.

And on the title screen the widget texture is the entire picture. Diverting it takes the whole
visible frame out of the scene, and compositing it back at present paints it in having skipped
everything AC7 does to it afterwards: its own UI composite, the glow, and the grade. The screenshots
show exactly that, a flat blue-washed frame rather than a few wrong HUD elements, and it is the same
cause as the missing effects rather than a second problem.

So compositing at present is the wrong insertion point for this game. The interface is not a layer
AC7 puts on top at the end; it is content the engine keeps processing.

### The correction: promote the interface target, and take the layer from there

Promotion gets this right for the reason extraction gets it wrong: the game's own composite runs, so
the colour, the glow and the grade are the game's, and nothing has to be reproduced. What made it
look impossible before was that the quads rasterize into a render-resolution target, so promoting
the target alone changed nothing. The divert now has viewport scaling, which is the missing piece:
with the target promoted and the viewport scaled, the quads rasterize at output resolution into it.

The earlier note here said promotion was not the route because frame generation needs the interface
as a separate premultiplied layer. That was wrong, and worth correcting rather than quietly
dropping: AC7's interface target **is** a premultiplied layer with coverage, which is exactly what
the vendors ask for. Promoted, it is that layer at output resolution. So promotion gives the correct
picture now and the frame generation input later, and the two stop being in tension.

### Superseded: the argument that promotion was not the route

An earlier version of this section argued that promotion could not be the route because frame
generation needs the interface as a separate premultiplied layer, and promotion puts the interface
back into the scene. The premise was wrong. AC7's interface target already is a premultiplied layer
with coverage, so promoting it produces that layer at output resolution rather than destroying it,
and the game composites it afterwards. Kept here because it was the stated reason for a decision,
and a decision reversed without saying why is worse than one never made.

### Measured: the menu shimmer is our own jitter

7 Sep 2026, by toggling the jitter gate on F4 while holding still on the main menu.

The front end shimmers at a reduced render scale and holds still at 100%. With the gate closed the
shimmer stops; with it open it returns. So the cause is the jitter this project forces on, and not,
as was also plausible, a reconstruction failing to resolve menu elements that carry no motion
vectors.

Both halves are needed to explain it. AC7 runs no temporal anti-aliasing, so the projection only
moves because `apply_jitter_patch` makes it; and the front end is drawn into render-resolution
targets and then spatially upscaled, so at 50% a sub-pixel offset is magnified by the upscale rather
than resolved by anything. At 100% the same offset is there and is too small to see.

This has a consequence for extraction beyond the obvious one. Drawing the interface into a layer at
output resolution takes it out of the upscaled path entirely, so the jitter stops landing on it, and
the shimmer should go without the gate being closed at all. That is a second, independent reason to
expect the front end to improve, and it is worth checking separately from sharpness: a run where the
interface is sharp but still shimmering would mean the quads are being composited at output
resolution and still being jittered, which points at the projection rather than at the layer.

The gate following the reconstruction (`RSF_ENABLE_JITTER=1`) is a mitigation and not the fix. The
fix is that nothing needing jitter should be drawn at a resolution nothing resolves it at, which is
what the screen policy decides and what extraction removes the need for on the front end.
