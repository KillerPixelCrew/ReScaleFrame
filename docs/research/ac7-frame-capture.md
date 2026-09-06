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

Unresolved: target #1208 at 1920x1080 takes 42 draws at the start of the captured frame while
everything else runs at 2048x1152. A capture spans present to present, so work that the game
performs at the end of its frame appears at the beginning of the capture, which fits Slate
rendering the interface. That would make it the interface layer at display resolution, separate
from the scene, which would matter a great deal for supplying a HUD-less image. It is a
hypothesis: confirming it needs the shader resource bindings that a replay provides.

## What is established and what is not

Established: the formats, sizes, bind flags, resource identities, and which scenes allocate
which targets. Those come from the capture and are not inferences.

Not established: that resource 2163 carries the encoding a super resolution backend needs. Its
format and lifetime match stock UE4.18 velocity, but sign, range, whether camera motion is
included, jitter treatment, and dilation state are properties of the contents. Reading contents
means replaying the capture, which needs Windows or a remote replay host. Until then the
identification is a strong lead, not a validated input.
