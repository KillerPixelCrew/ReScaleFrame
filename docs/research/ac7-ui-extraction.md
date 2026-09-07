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

Not yet measured.
