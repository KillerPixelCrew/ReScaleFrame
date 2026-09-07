# Where render resolution belongs, and where the interface belongs

Two questions, asked before writing any more reinsertion code: where 4.18 wants the render
resolution cranked down without dragging the interface with it, and what the correct order of
reconstruction, interface and presentation is once frame generation is in the picture.

Reference UE4 `4.18.3-release`, commit `0a14a8d537a31ecc77488ced41dbaa0166612ef8`. The sparse
checkout was extended for this with `Engine/Private`, `SlateRHIRenderer` and `SlateCore`, since the
question is not a renderer question and could not be answered from `Renderer` alone.

## The observation this has to explain

Pressing F8 downscales the whole composition, interface included. Two screenshots of the same
briefing, one at native and one at a 50% render scale, show the same strings, `SWITCH OPERATION
AREA`, `MISSION PREP`, `AIR(TGT)`, crisp in the first and visibly soft and chunky in the second.
The interface is being rasterized at half resolution and upscaled with the scene.

That is the fact this note exists to explain. An earlier revision led with the stock engine
behaviour instead and said screen percentage "already leaves the interface alone", which reads as a
denial of something directly visible and repeatable. The stock behaviour below is still worth
writing down, but only as the thing AC7 departs from.

## What stock 4.18 does

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

So in a stock 4.18 game the interface would not follow the render scale. AC7's does, so AC7 is not
doing this, and the mechanism is one step further back.

`r.ScreenPercentage` shrinks `ViewRect`. `ViewFamily.FamilySizeX/Y` is computed from the view rects,
and `FSceneRenderTargets::ComputeDesiredSize` at `SceneRenderTargets.cpp:281` sizes the scene buffers
from that family size under both `RequestedSize` and `Grow`. So the scene buffer itself becomes
1024x576, which is what this project measured independently when the separate translucency layer
came out at half of it.

Anything AC7 allocates against that buffer size shrinks with it. Its interface target is 1024x576
in the same frames where the buffer is 1024x576, and its final pass upscales scene and interface
together. That is the whole mechanism: the interface does not follow the *view rect*, it follows the
*buffer*, and the engine hands the game a smaller buffer.

**So there is no console variable that cranks the render resolution down without taking the
interface with it.** The wrench is the size of the target the interface is rasterized into, which
means the intervention has to be a render target substitution rather than a setting. That is exactly
what reinsertion already does when it promotes the interface target to output resolution, and it is
why the interface stopped being blown up once that path started working. The wrench is already in
the right place. What is not yet right is that promoting the composite alongside it loses the 3D
scene.

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

## The SDK names every one of those surfaces

`references/ac7-sdk` settles what the tail is, and it should have been the first thing read rather
than the last. `Nimbus.WidgetToTextureConverter` is the class:

```cpp
class UWidgetToTextureConverter : public UObject {
    FIntPoint               DrawSize;              // 0x0028  (Edit)
    UUserWidget*            Widget;                // 0x0030
    UTextureRenderTarget2D* RenderTarget;          // 0x0048
    UTextureRenderTarget2D* DownSampleRT;          // 0x00C0
    UTextureRenderTarget2D* BlurXRT;               // 0x00C8
    UTextureRenderTarget2D* BlurYRT;               // 0x00D0
    UTextureRenderTarget2D* RenderTargetWithGlow;  // 0x00D8
};
```

AC7 rasterizes a UMG widget tree into `RenderTarget` at `DrawSize`, then runs its own downsample and
two-axis blur to make a glow, and composites the result. That chain is the tail's slots 1, 2 and 3
exactly: 1024x576, then 256x144, then 128x72, all `R8G8B8A8_UNORM`. So slot 1 is the interface as a
matter of record now, not an inference from its format.

The instances say which surface belongs to which screen:

| Field | Offset | Screen |
| --- | --- | --- |
| `FrontWindowConverter` | `0x0D48` | front end, which includes the briefing |
| `HudWidgetConverter` | `0x0470` | in flight |
| `HudPostProcessConverter` | `0x0478` | in flight |
| `StereoWidgetConverter`, `OverlayTextureConverter` | `0x0370`, `0x0378` | VR |

**`DrawSize` is the wrench.** It is the resolution the interface is rasterized at, it is a plain
`FIntPoint` on a `UObject`, and it is per converter, so the briefing and the flight HUD can be
pinned to native independently of each other and of the scene. Nothing in that path is a console
variable.

The SDK also explains a problem this project has been working around rather than solving.
`EGraphicsScreenPercentageSettings` enumerates `Gameplay`, `NonGameplay`, `MPGameplay`,
`VRGameplay`, `VRNonGameplay`, `VRAirShow` and `NoChange`, and
`GraphicsSettingsWindowsBlueprintLibrary` carries `SetWindowsDrawScale` with
`EGraphicsSettingsWindowsDrawScale`. AC7 keeps its own per-context render scale table and applies it
on transitions. That is why loading a mission puts `r.ScreenPercentage` back to 100 and why
`keep_render_scale` exists to keep re-writing it. Setting the game's own value means the game asks
for our number itself, instead of being overruled once per screen change and reverting.

## The two candidate causes of the missing environment under F6

> **Retracted, 7 September.** Both candidates below were framed around "the interface target", a
> render-resolution `R8G8B8A8` surface picked by format. The run log later showed what that surface
> is: AC7's own UI layer, which the widget quads draw into with the scene's depth bound, composited
> by the game before the upscale (see "Root" below and the tail order at the end of this file). The
> depth mismatch was real and is recorded; the identification was wrong. Superseded by the
> [representation plan](../representation-plan.md).

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

## Why promoting the interface target does not sharpen it

> **Retracted, 7 September.** The `DrawSize`/`Scale` patch described here is unnecessary: the
> interface is rasterized at 1920x1080 (see "Root" below), and the surface being promoted was not
> a converter target at all. Kept as the record of a wrong turn. Superseded by the
> [representation plan](../representation-plan.md).

Game-tested 7 September. With reinsertion alive the picture is cleaner and the interface is still
well short of native. The log shows the interface target promoted, `1024x576 to 2048x1152, format
27`, so the texture is the right size and the pixels land across all of it. The glyphs are still the
ones rasterized for a 1024x576 target, magnified.

Promoting a render target changes where pixels land, not how they are generated. Slate rasterizes
text from a font atlas at a size the layout decides, and 4.18's widget renderer makes that explicit:

```cpp
FGeometry WindowGeometry = FGeometry::MakeRoot(DrawSize * (1 / Scale), FSlateLayoutTransform(Scale));
```

Absolute pixels are `LocalSize * Scale`, which is `DrawSize`, and the layout space is
`DrawSize / Scale`. So native-resolution interface needs both terms moved together: `DrawSize` to
the output resolution and `Scale` to the ratio. `DrawSize` alone doubles the layout space and halves
the apparent size of everything; `Scale` alone halves the layout space and doubles it. Neither is
usable on its own.

`FWidgetRenderer::DrawWidget` passes a literal `1` for that scale, and AC7 has inlined it.
`UWidgetToTextureConverter_SetupVirtualWindow` at `0x1404d69a0` reads the converter's `DrawSize`
straight from `this+0x28`, the offset the SDK gives, converts the `FIntPoint` to the `FVector2D` the
`SVirtualWindow` is constructed with, and resizes the window to it:

```c
uVar4 = *(undefined8 *)(param_1 + 0x28);                       // DrawSize
puVar8[0x46] = CONCAT44((float)(int)(uVar4 >> 32), (float)(int)uVar4);   // SNew(SVirtualWindow).Size(...)
...
FUN_140c921b0(*pplVar2, CONCAT44(...));                        // Window->Resize(DrawSize)
```

Its two callers, `FUN_1404d5c10` and `FUN_1404d6340`, are where the draw happens and where the scale
term lives. Identifying which and patching both terms is the outstanding work. Until then the
interface is drawn at render resolution and magnified, which is better than before only because the
composite it lands in is no longer magnified again after it.

## Status

No code changed for this note. The stock behaviour is established from source, the AC7 tail from a
game run's log, and the two candidate causes are stated as candidates because neither has been
tested. The identity of slot 1 is unverified and is the first thing to settle, since both the
reinsertion bug and the frame generation requirement depend on which surface actually carries the
interface.

## Root: the interface is rasterized at 1920x1080 and then squashed

7 September 2026, from the binary rather than from captures, after being told repeatedly that the
branch was being tapped instead of the root.

`UWidgetToTextureConverter_Setup` at `0x1404d5c10` does not compute `DrawSize`. It is handed one:

```c
local_res8 = (float)param_3;                            // the caller's FVector2D
*(int *)(param_1 + 5)              = (int)local_res8;   // +0x28  DrawSize.X
*(int *)((longlong)param_1 + 0x2c) = (int)fStackX_c;    // +0x2C  DrawSize.Y
```

The front end's caller is at `0x1406243e0`, and it is unambiguous about which converter it is
building, `mov [rdi+0xd48], rax`, the `FrontWindowConverter` offset the SDK gives:

```
1406244f8  movss xmm1, [0x1425f1ce4]      ; 0x44870000 = 1080.0f
140624500  movss xmm0, [0x1425f1cac]      ; 0x44F00000 = 1920.0f
140624508  unpcklps xmm0, xmm1
140624510  movq  r8, xmm0                 ; the FVector2D DrawSize
14062451f  call  UWidgetToTextureConverter_Setup
```

**The interface is rasterized at a fixed 1920x1080 and never sees the render scale.** So the blur is
not low resolution rasterization, and the `DrawSize`/`Scale` patch described in the section above is
unnecessary: 1920x1080 is already ample for a 2048x1152 output. A 1920x1080 interface is composited
into a 1024x576 composite, squashed to fit, and the frame's last draw blows that back up to
2048x1152. Sharp, downsampled, re-upsampled.

The root is therefore the resolution of the target the interface is composited *into*, not the
resolution it is drawn *at*. Reinsertion promoting the composite is aimed correctly; something in
between is still at render resolution.

This also retires a claim made earlier in this file. The surface the tail calls the interface target,
slot 1 at 1024x576 `R8G8B8A8_UNORM`, cannot be a `WidgetToTextureConverter` render target, because
those are 1920x1080. It was identified by format, and the glow chain that appeared to confirm it,
256x144 and 128x72, was one frame's coincidence: other frames show 1x1 and 63x63 in those slots. So
what has been promoted as the interface for several runs is a render resolution surface of unknown
role.

The measurement that closes this is small and specific: find the draw whose shader input is a
1920x1080 `R8G8B8A8` texture, and record what it writes into and with what viewport. That names the
target that has to be promoted, and it does not depend on any format heuristic.

## The briefing tail, in order, from the run log

7 September 2026. The shape hunt described above ran and reported the whole tail of one briefing
frame (`rsf-dump.log`, one frame near line 425,120). Read top to bottom:

1. Glow pass: 6 indices, no depth, reads a 1920x1080 `B8G8R8A8_TYPELESS` widget texture, writes
   another 1920x1080 texture (alternating `0x3075D000` / `0x3075CC40` per frame).
2. Scene lighting into scene colour `0x308F4390` (`R11G11B10_FLOAT`) with depth view `0x307329D0`.
3. Four world-space widget quads: 6 indices each, reading four different 1920x1080 widget textures
   in PS slot 0, writing 1024x576 `R8G8B8A8_TYPELESS` targets `0x308F6550` (two quads) and
   `0x308F6190` (two quads), viewport 1024x576 at 0,0, **with the same depth view the scene used**.
4. Tonemap: scene colour plus bloom into the composite `0x308F3FD0` (1024x576, `B8G8R8A8_TYPELESS`).
5. An unwatched step produces `0x308E2770` (1024x576, same format) from the composite.
6. The game's own UI composite pass: reads `0x308E2770` (slot 0), `0x308F6550` (slot 1, the
   `R8G8B8A8` layer) and the glow and lens surfaces, and writes the composite `0x308F3FD0` again.
7. Upscale: composite to the back buffer, 2048x1152 `R10G10B10A2_UNORM`.

So the interface on the briefing is not rasterized into scene colour. AC7 draws the widget quads,
depth-tested against the scene, into a dedicated render-resolution `R8G8B8A8` layer of its own, and
composites that layer itself before the upscale. The alternating pair of layer targets is what the
SDK's `UWidgetComponent::DownsampledRenderTargetArray[2]` names. Three consequences: diverting the
quads elsewhere makes the game's own layer transparent and the back buffer HUD-less by construction;
occlusion is real wherever scene geometry sits in front of a panel; and reinsertion has an
unpromoted intermediate (`0x308E2770`) between the tonemap and the UI composite, which is why
promoting the composite made the picture cleaner without making it sharper.

That is the last finding this note records. What follows from it is the
[representation plan](../representation-plan.md), and the work is tracked there and in
[ac7-ui-extraction.md](ac7-ui-extraction.md).
