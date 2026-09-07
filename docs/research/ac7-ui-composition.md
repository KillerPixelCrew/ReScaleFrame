# Where render resolution belongs, and where the interface belongs

Two questions, asked before writing any more reinsertion code: where 4.18 wants the render
resolution cranked down without dragging the interface with it, and what the correct order of
reconstruction, interface and presentation is once frame generation is in the picture.

Reference UE4 `4.18.3-release`, commit `0a14a8d537a31ecc77488ced41dbaa0166612ef8`. The sparse
checkout was extended for this with `Engine/Private`, `SlateRHIRenderer` and `SlateCore`, since the
question is not a renderer question and could not be answered from `Renderer` alone.

## What stock 4.18 does, which is already the right thing

Screen percentage in 4.18 shrinks the 3D scene and nothing else. It is a rectangle, not a buffer
size: `FSceneView` carries both `ViewRect` and `UnscaledViewRect`, the scene renders into the first,
and the second stays at the viewport's own size.

Post-processing ends by putting the two back together. `PostProcessing.cpp:2480` sets
`FinalOutputViewRect = View.UnscaledViewRect`, and `:2544` applies the scale only when the two
rectangles differ, either inside the tonemapper via `bDoScreenPercentageInTonemapper` or as a
separate `FRCPassPostProcessUpscale`. That pass declares its own output at native,
`PostProcessUpscale.cpp:403`, `Ret.Extent = View.UnscaledViewRect.Max`. So everything downstream of
the tonemapper is at the viewport's resolution.

The interface arrives strictly after that, and against a target that never knew about the scale.
`FViewport::Draw` builds the scene canvas against the viewport itself, `UnrealClient.cpp:1206`, whose
render target is `GetSizeXY()`, the window. `UGameViewportClient::Draw` calls
`BeginRenderingViewFamily` with that canvas, `GameViewportClient.cpp:1276`, and only then reaches
`MyHUD->PostRender()` at `:1364` and the debug canvas at `:1381`. Slate widgets come later still,
through the Slate renderer, against the same window-sized target.

So the answer to the first question, for a stock 4.18 game, is that `r.ScreenPercentage` is already
the wrench and it already leaves the interface alone. There is no second place to fasten it.

## What AC7 does instead, which is why the question came up

AC7 does not present the stock tail, and the run log says so without needing a capture. The draw
that writes the back buffer at 2048x1152 has seven inputs bound:

```
slot 0  1024x576  format 90   B8G8R8A8_TYPELESS
slot 1  1024x576  format 27   R8G8B8A8_UNORM
slot 2   256x144  format 27
slot 3   128x72   format 27
slot 4    63x63   format 90
slot 5  1024x576  format 10   R16G16B16A16_FLOAT
slot 6  1024x576  format 35   R16G16_UNORM, velocity
```

A stock upscale pass reads one colour input. This one reads a tonemapped scene, a velocity target
and four more colour surfaces, and it is the pass that takes 1024x576 to 2048x1152. So AC7 composites
and upscales in one custom pass, and both the scene and whatever slot 1 is arrive at scene
resolution rather than native. That is the observed interface softness at a reduced render scale,
and it is a game deviation, not an engine behaviour.

Slot 1 is **assumed** to be the interface, not established. It was selected by format, being the only
full-size `R8G8B8A8_UNORM` among `B8G8R8A8` scene targets, and slots 2 and 3 share that format at
smaller sizes. Nothing has confirmed the game draws its interface into it. This matters because the
reinsertion path promotes it.

## The two candidate causes of the missing environment under F6

Reinsertion currently promotes both slot 0 and slot 1 to 2048x1152 and reports doing so. The briefing
then loses its 3D environment while the interface survives, which is the same signature as the
separate translucency layer at a scale above 1.0.

**Depth is never considered.** `runtime/graphics/src/scene_reinsert.cpp` scales viewports and scissor
rectangles for a promoted target, `:231`, and contains no reference to depth anywhere in the file.
Promoting a colour target to 2048x1152 while the game keeps binding a 1024x576 scene depth is the
invalid pair that this project has already diagnosed once, in
[ac7-frame-capture.md](ac7-frame-capture.md), where it made the briefing relief disappear and left
its HUD untouched. That is the same symptom and the same shape.

**Slot 1 may not be the interface.** If it is a post-process intermediate the scene passes read,
promoting it breaks the scene while leaving the real interface, drawn elsewhere, intact. That also
matches "the environment is missing and the interface is not".

The two are distinguishable in one instrumented run and should be, before either is acted on: log
the depth bound alongside each promoted target, and describe which draws write into slot 1 rather
than only which read it. The tap already answers both questions.

## The order once frame generation is in it

Frame generation is not a later addition to this design, it is the thing that fixes the shape of it,
because it wants the same separation that the interface question wants.

DLSS-FG runs at present, through a Streamline proxy swap chain, and interpolates between finished
frames. Its inputs are depth and motion vectors, the same ones super resolution takes, plus a
HUD-less colour and, separately, the interface with its alpha. It needs both: the HUD-less colour so
it never warps interface pixels as though they were scene, and the interface surface so it can put
the interface back onto a generated frame it did not render. Motion vectors must not describe
interface motion. Interface baked irreversibly into the colour is the case with no good outcome, and
it shows as smeared or half-rate interface.

That gives the order, and it is the same order for both features:

```
scene at render resolution, jittered
  -> super resolution                          -> scene at native
  -> the game's own tonemap and grade at native            (what reinsertion is for)
  -> this is HUD-less colour
  -> interface drawn at native into its own surface        (interface colour and alpha)
  -> interface composited over the scene       -> back buffer
  -> frame generation at present, given depth, motion vectors, HUD-less colour and the interface
```

Two consequences worth stating plainly. The interface must not be inside the super resolution input,
which it is not today and must stay out of. And the interface must remain a separate surface all the
way to present rather than being flattened early, which is a stronger requirement than sharpness
alone would justify: sharpness only wants it drawn at native, frame generation wants it still
separable at the end.

That makes reinsertion's structure right and its scope too small. Promoting the composite and the
interface to native is what the first three steps need. Keeping the interface addressable at present
is what the last step needs, and nothing addresses that yet.

## Status

No code changed for this note. The stock behaviour is established from source, the AC7 tail from a
game run's log, and the two candidate causes are stated as candidates because neither has been
tested. The identity of slot 1 is unverified and is the first thing to settle, since both the
reinsertion bug and the frame generation requirement depend on which surface actually carries the
interface.
