# AC7 frame capture

Captured 6 September 2026 from the installed game running under Proton, using in-application
RenderDoc driven by the research proxy. 22 captures: the aircraft selection screen where the
camera pans, level flight over ground and water, cloud interiors, and one frame with screen
droplets rendering.

## Capturing and reading a capture on Linux

RenderDoc's Windows `renderdoc.dll` does hook DXVK's `d3d11.dll` under Proton. Every trigger
returned success and produced a file, so D3D11 level captures are available on a Linux
development machine.

Reading them needs one detour. The Linux RenderDoc build contains no D3D11 support at all, not
even a chunk parser, and refuses with `Can't get structured data for driver D3D11`. The Windows
`renderdoccmd.exe` runs under Wine, and its `convert -c xml` is structured data only with no
replay device involved, so it produces the full API stream locally:

```bash
wine renderdoccmd.exe convert -f capture.rdc -o capture.xml -c xml
python3 tools/parse-capture.py capture.xml
```

A 111 MB capture converts to about 4.4 MB of XML. Replaying a capture, which is what gives pixel
contents and shader debugging, still requires Windows or a remote replay host.

## Frame structure

Consistent across every captured scene.

| Property | Value |
| --- | --- |
| Presented size | 2048x1152 |
| Main render target size | 2048x1152, so screen percentage is 100 |
| Textures created per captured frame | 144 to 153 |
| API chunks per captured frame | 4562 to 5158 |
| Render target bindings per frame | 94 in the hangar, 125 in flight |

Full resolution targets, 18 to 19 of them, include two `R11G11B10_FLOAT` scene colour, two
`R32G8X24_TYPELESS` depth and stencil, three `R16G16B16A16_FLOAT`, and seven
`B8G8R8A8_TYPELESS`. Shadow depth appears at 5120x1024 and 2048x2048 in flight only. The post
process ladder runs 1024x576, 512x288, 256x288 with `R11G11B10_FLOAT`, plus non square 512x576
and 256x288 stages that look like separable bloom.

## The two velocity targets

This is the result that matters for super resolution, and it confirms a concern the hook map
raised from source alone.

| Resource | Format | Size | Binding | Present in |
| --- | --- | --- | --- | --- |
| 2163 | `R16G16_UNORM` | 2048x1152 | SRV + RTV | every capture |
| 63083 | `R16G16_UNORM` | 1024x576 | SRV + UAV | in-flight captures only |
| 2111 | `R10G10B10A2_UNORM` | 2048x1152 | SRV + RTV | every capture |

Resource 2163 is full resolution, is rendered into as a render target, and exists even on the
aircraft selection screen. `R16G16_UNORM` is `PF_G16R16`, which is what UE4.18 allocates for
scene velocity. This is the super resolution motion input.

Resource 63083 is exactly half resolution, is written through an unordered access view rather
than as a render target, and is absent from the hangar scene. Compute written, half resolution
and velocity formatted is the shape of UE4's velocity flatten pass, which serves motion blur.
`ue418-hook-map.md` warned against reusing that output as ordinary motion because it stores a
polar representation with depth. Both textures now exist as distinguishable resources rather
than as a caution, and they are told apart by resolution and binding without reading a pixel.

Resource 2111 at `R10G10B10A2_UNORM` matches `PF_A2B10G10R10`, UE4's GBuffer world normal.

The resource identifiers are stable across captures, so these are allocated once during startup
rather than per frame.

## Frame timeline

`tools/parse-capture.py --timeline` reconstructs the frame's passes without a replay device, by
following render target bindings and counting the draws between them. The shipping build emits no
debug markers, so a pass here means a run of draws sharing one output binding. 106 such passes do
work in the sampled in-flight frame.

The recognisable structure, with chunk indices from `ac7_frame52376`:

| Chunk | Work | Output | Reading |
| --- | --- | --- | --- |
| 1317 | 42 draws | 1920x1080 `B8G8R8A8` #1208 | a target at a different resolution to everything else, role unresolved |
| 2016 | 10 draws | six targets at 2048x1152 plus depth | the GBuffer base pass: scene colour `R11G11B10` #56121, normals `R10G10B10A2` #2111, four `B8G8R8A8` |
| 2278 to 2413 | 10 + 7 draws | depth only, 1024x1024 and 2048x2048 | shadow map cascades |
| **2518** | **10 draws** | **`R16G16_UNORM` #2163 plus depth** | **the velocity pass** |
| 2651 | 39 draws | 2048x1152 `B8G8R8A8` #2178 plus depth | |
| 3939 | 25 draws | scene colour plus depth | |
| 4448 | 1 draw | 1x1 `R32G32_FLOAT` #1628 | eye adaptation, the exposure value a backend needs |
| 4460 to 4650 | 1 draw each | 1024x576 down to 16x18 and back up | the bloom ladder, descending then ascending |
| 4864 | 1 draw | #268 | the swap chain back buffer, named `Swap Chain Backbuffer` in the capture |

Two things follow directly.

**Velocity is its own pass, not GBuffer packed.** Ten draws render into #2163 with depth bound.
Ten draws is a small number against the base pass, so only moving objects write velocity, which
means camera motion has to be reconstructed from matrices rather than read from the target. That
is the question `ue418-hook-map.md` raised about object motion coverage, and it is now answered
for this frame.

**The frame ends in a single draw into the back buffer.** Everything before it composites into
intermediate targets. That single draw is the natural boundary for anything that needs the
finished image.

### Locating temporal AA

Three independent signals agree, without any debug marker being present.

Only two passes in the frame sample the velocity target #2163:

```
[ 3257] 1 draws -> 2048x1152 R16G16B16A16_FLOAT #1732
[ 3275] 1 draws -> 2048x1152 R16G16B16A16_FLOAT #2168
```

`FRCPassPostProcessTemporalAA::ComputeOutputDesc` in 4.18.3 forces `PF_FloatRGBA` regardless of
its input format, which is `R16G16B16A16_FLOAT`, and both passes match. They are single full
screen draws, they sit immediately before the post process downsample chain begins at chunk 3323,
and nothing else in the frame reads velocity. That places temporal AA at chunks 3257 and 3275.

The same function settles the reinsertion problem the hook map raised:

```cpp
FPooledRenderTargetDesc Ret = GetInput(ePId_Input0)->GetOutput()->RenderTargetDesc;
Ret.Format = PF_FloatRGBA;
```

The output descriptor is copied from the input and only its format and flags are overridden, so
the extent is inherited. A larger reconstruction result cannot be returned by swapping a texture
pointer, because the allocation the graph makes is input sized. That confirms from source what
`ue418-hook-map.md` predicted.

One limitation of the timeline worth stating: a pass boundary is a render target binding, so
compute work that binds no render target is attributed to whichever draw pass preceded it. The
motion blur velocity flatten is compute, which is why it does not appear here as its own pass.

Unresolved: target #1208 at 1920x1080 takes 42 draws at the start of the captured frame while
everything else, including the back buffer, runs at 2048x1152. A capture spans present to present,
so work the game performs at the end of its frame appears at the beginning of the capture, which
fits Slate rendering the interface.

These captures were taken in windowed mode, so 1920x1080 is not a display mode the game was
presenting at. A fixed 1920x1080 layer inside a 2048x1152 back buffer is consistent with a user
interface authored against a reference resolution and scaled to the window, which is ordinary
practice. If that is what it is, the interface is already composited from its own target rather
than drawn over the scene, which would matter a great deal for supplying a HUD-less image to
frame generation.

It remains a hypothesis. Confirming it needs the shader resource bindings a replay provides:
specifically whether #1208 is sampled by the final draw into the back buffer.

## The velocity encoding, from engine source

Read from the authorized 4.18.3-release checkout, `Engine/Shaders/Private/Common.ush`:

```hlsl
// velocity needs to support -2..2 screen space range for x and y
// texture is 16bit 0..1 range per channel
float2 EncodeVelocityToTexture(float2 In)
{
    // 0.499f is a value smaller than 0.5f to avoid using the full range to use the clear color (0,0) as special value
    return In * (0.499f * 0.5f) + 32767.0f / 65535.0f;
}
```

So the scale is `0.499 * 0.5 = 0.2495`, the bias is raw 32767 rather than 32768, and the encodable
range is -2 to 2 in screen space rather than -1 to 1. Decoding is
`(value - 32767/65535) / 0.2495`.

The detail that matters most is the reason for 0.499 rather than 0.5: it keeps the encoded range
clear of zero so that **raw zero is reserved as a sentinel meaning "nothing wrote velocity here"**.
`PostProcessTemporalCommon.ush` relies on exactly that:

```hlsl
float4 PrevClip = mul( ThisClip, View.ClipToPrevClip );
float2 PrevScreen = PrevClip.xy / PrevClip.w;
float2 BackN = PosN.xy - PrevScreen;          // camera motion, computed per pixel
...
bool DynamicN = VelocityN.x > 0.0;            // was anything written here
if (DynamicN) { BackN = DecodeVelocityFromTexture(VelocityN); }
```

**The velocity target holds object motion only. Camera motion is never stored in it.** Temporal AA
reconstructs camera motion per pixel from `View.ClipToPrevClip` and only overrides it where an
object actually drew. That is why the capture shows ten draws into a full resolution target: the
rest of the screen is left at the clear value on purpose.

### What the plugin has to build

Composing a motion vector field is ordinary work for this kind of integration, since games not
built for super resolution rarely hand over one ready to use. What matters here is the specifics,
which are now exact rather than assumed.

The plugin needs its own pass producing the combined field: decode the target where `x > 0` using
`(value - 32767/65535) / 0.2495`, compute camera motion from `ClipToPrevClip` everywhere else,
and convert to the units the selected backend wants. Two consequences follow. The pass needs
`View.ClipToPrevClip` at runtime, which is a struct layout question a frame capture cannot answer.
And the sentinel test is `x > 0` on the raw value, not a comparison against the bias, so the
decode has to run after the test rather than being folded into it.

Velocity units are screen space as the shader uses them, where `BackN * ViewportSize` gives an
offset in units of two pixels per viewport width. Backend conversion has to account for that
factor, and for the y direction convention, before anything is passed along.

Established: the formats, sizes, bind flags, resource identities, and which scenes allocate
which targets. Those come from the capture and are not inferences.

Not established: that resource 2163 carries the encoding a super resolution backend needs. Its
format and lifetime match stock UE4.18 velocity, but sign, range, whether camera motion is
included, jitter treatment, and dilation state are properties of the contents. Reading contents
means replaying the capture, which needs Windows or a remote replay host.

## Replay results

All 22 captures were replayed on Windows on 6 September 2026. Pixel contents settle what the
structured data could only suggest, and correct two claims made earlier in this document.

### The encoding is confirmed, and one constant here was wrong

Written pixels in #2163 average 0.4999928 normalised in every capture. The bias in `Common.ush`
is `32767/65535 = 0.4999924`. Those agree to seven decimal places, so the encoding is stock
Unreal and the bias is 32767 rather than 32768.

The Windows instructions carried a decode of `(value - 0.5) * 2`, inherited from an early draft of
this research before the source was read. Both terms are wrong. The correct decode is

```
velocity = (value - 32767.0/65535.0) / 0.2495
```

a multiplier of 4.008, not 2. The measurements cannot distinguish the two because every velocity
in these frames is small, which is exactly why the constant has to come from source rather than be
fitted to data. `runtime/graphics` implements the source form.

For scale: the largest raw value anywhere is 32813, in a frame with ground in view, decoding to
0.0028 in screen space or roughly 2.9 pixels. In the camera-panning menu capture the largest is
32768, which is 0.06 of a pixel. Where the camera tracks the aircraft the object is nearly static
relative to it, so almost all apparent motion lives in the reprojection term rather than here.

### Camera motion is absent, now measured rather than inferred

| Capture | Written | Scene |
| --- | --- | --- |
| `ac7_frame32137` | 16.4% | menu, camera panning |
| in-flight, clear sky | 4.7% to 8.3% | aircraft only |
| `ac7_frame36073`, `ac7_frame36392` | 52%, 58% | ground in view |

On the camera-only capture 83.6% of the frame is exactly zero while the camera pans, and in flight
the sky stays zero while sweeping across the screen. That is the source reading confirmed from the
opposite direction.

### Correction: resource 63083 is not the velocity flatten

This document earlier read #63083 as the motion blur velocity flatten, from its format, half
resolution and compute writing. The pixels disagree: mean 0.55, no zero clear anywhere, a bimodal
histogram with large populations at both ends, saturated across the sky with the aircraft
silhouette punched out. That is a mask, and its resource id sits in the same allocation run as the
cloud textures 62989 to 63019.

The operative conclusion is unchanged, since it must not be used as motion either way, but the
reasoning was wrong. Format and binding were not enough to identify it.

### The HUD is already separable

The frame ends identically in all 22 captures:

```
56115 --copy--> 2123 --full screen draw--> 64363 --> [2181 HUD] --> bloom 2253..2273 --> 268
```

| Resource | Format | Size | Holds |
| --- | --- | --- | --- |
| 2123 | `B8G8R8A8` | 2048x1152 | composed scene, byte for byte a copy of 56115 |
| 64363 | `B8G8R8A8` | 2048x1152 | the same scene after one full screen pass, still no HUD |
| 2181 | `R8G8B8A8` | 2048x1152 | the HUD alone, on a transparent background |
| 268 | `R10G10B10A2` | 2048x1152 | swap chain: 64363 graded, plus HUD and its glow |

Resource 2181 holds nothing but the HUD and is composited only in the final draw, along with a
blur chain that gives it its glow. Any stage up to and including 64363 is a finished HUD-less
image needing no masking or reconstruction. That is the best available outcome for frame
generation and removes the HUD exclusion work the validation plan budgeted for.

The single full screen pass from 2123 to 64363 is a vignette: it changes 33.6% of pixels by a mean
of 7 of 255, growing monotonically from 2.5 at the centre to 11.1 at the edge.

### Screen droplets are a shader effect

A pair of consecutive resources: 57437, a `BC3` 1024x1024 droplet mask, and 57439, the matching
`BC5_UNORM` normal. A mask plus a two channel normal is a screen space refraction pair. Nothing
resembling droplets exists as an interface target and no separate droplet pass appears in any
action list.

Both are resident in all 19 in-flight captures including frames that visibly have none, so
residency does not mean the effect is running. Which frames draw them needs the shader resource
bindings per draw, which the exported action list does not record.

A late screen space refraction is applied after velocity is written, so droplets carry no motion
of their own and would not reproject. They belong after upscaling.

### The caveat that applies to all of this

Every capture was taken at 100 screen percentage, so render resolution and output resolution are
equal throughout. Nothing here shows which targets follow render resolution and which follow
output resolution once the two diverge, which is exactly what inserting super resolution does.
