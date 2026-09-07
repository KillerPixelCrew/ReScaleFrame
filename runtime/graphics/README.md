# Graphics helpers

D3D11 utilities shared by the research proxy and runtime. Game-specific view interpretation lives in `games/ac7`; vendor evaluation lives in `runtime/backends`.

| Module | Purpose |
| --- | --- |
| `d3d11_observer` | Track selected allocations and invoke a Present callback |
| `resource_roles`, `frame_tap` | Classify resources, find candidate colour/depth/motion bindings, and describe the draws into a named target |
| `resource_ref` | Retain and release COM resources across the C ABI |
| `constant_buffer_read` | Stage and read a buffer with type/device checks |
| `motion_decode` | Convert biased velocity to float motion and preserve unwritten pixels |
| `scene_reinsert` | Put a reconstructed scene back into the game's frame by promoting its composite; becomes `scene_promote` in the representation plan, with the interface-target promotion removed |
| `depth_replay` | Replay the separate translucency layer's draws depth-only into an output-resolution copy of scene depth, for the backend's camera-motion resolve |
| `d3d11_state` | Save everything the device context has bound, and put it back |
| `texture_dump` | Read supported texture formats into diagnostic TGA/JSON files |
| `present_blit` | Show reconstructed scene colour over the back buffer. Superseded by `fullscreen_pass` and kept only until the debug view moves across |
| `fullscreen_pass` | One triangle, four modes: copy, tonemap, premultiplied composite, and coverage as grey. The composite is `ui.rgb + (1 - ui.a) * dst`, which is what all three frame generation SDKs specify, so the picture we make and the picture a vendor makes cannot drift |
| `ui_layer` | A double-buffered `R8G8B8A8_UNORM` surface at back-buffer extent, cleared to zero and never bound with a depth view, for interface draws to be diverted into |
| `ui_identify` | Membership sets of pipeline objects held by address, with eviction before an address is reused, and Castagnoli hashes for naming a shader in a settings file |
| `overlay_renderer`, `overlay_input` | Render egui meshes and collect window input |

The frame tap uses format and binding heuristics. It does not yet identify a verified AC7 shader, view, and frame. A retained texture can still be overwritten by the game; choose the consumption or copy point explicitly.

`rsf_frame_tap_watch_target` asks where the reconstruction goes back in. It names a render target and describes the next few draws into it: extent, the viewport actually in effect, the ordinal within the pass, indexed or not, and every pixel shader input with its slot. Pointed at the back buffer, the single texture that draw reads is the composite; pointed at that composite, the draw that reads scene colour is the tonemap. Neither fact is available from the captures here, because the exported action list records render-target bindings and not shader resource bindings. Each watch carries a draw budget: watching the back buffer holds a reference on it, which makes `ResizeBuffers` fail, so a permanent watch trades an answer for a later mode change that breaks.

## Interface extraction, and why it is not the route it looked like

The frame tap can divert a classified draw: retarget it to the UI layer through the original vtable
entries, scale its viewport by the fraction of its own target it covered, patch its blend's alpha
operations, forward it, and put everything back. `rsf_frame_tap_set_divert` arms it and a game-side
callback decides each draw, because which draws are the interface is a game fact and lives in
`games/ac7`.

It works, and against AC7 it produces the wrong picture. Measured on 7 September 2026: the interface
does reach the screen at native resolution, which is what the whole route existed for, and the frame
comes out flat and discoloured. The diverted quads read the scene and its blur and glow chain as
inputs, so they are composites rather than overlays, and on the title screen the widget texture is
the entire visible image. Diverting them takes content out of the frame that AC7 is still going to
process, and compositing it back at present skips that processing.

So the mechanism stays and the insertion point changes: promote AC7's own interface target and let
the game composite it, which keeps the colour and the glow, and take the frame generation layer from
that promoted target, which already holds premultiplied colour with coverage. See
[the extraction note](../../docs/research/ac7-ui-extraction.md) for the measurements and for the
argument this reverses.

## Scene reinsertion

The debug view draws the reconstruction over the finished frame, so the image is ungraded and everything the game composited after it, the interface included, is gone. `scene_reinsert` builds the substitution instead: it promotes the composite and the interface's own target to output resolution, points scene colour at the reconstruction, and hands the tap a plan. `rsf_frame_tap_set_plan` swaps those bindings before forwarding them, in the output merger, in the pixel stage, and in `ClearRenderTargetView`, and scales viewports and scissor rectangles while a promoted target is bound. The game then grades the reconstruction with its own shaders, draws its own interface over it at output resolution, and its final upscale into the back buffer becomes a copy. Nothing is removed from the frame.

Three parts of that plan are decisions rather than mechanics:

- Scene colour is gated on the composite being bound. The scene passes read scene colour while they are still writing it, so an ungated substitution hands a lighting pass a reconstruction of a frame it has not finished, which is a feedback loop rather than an upscale.
- The gate is also where the reconstruction runs. It is the last point before anything reads scene colour and the first where the scene is whole; evaluating at Present would leave the scene a frame behind the grade and interface drawn over it. The price is a mid-frame evaluate, where the game will not rebind what it believes is still bound, which is what `d3d11_state` is for.
- The interface target used to be identified by format, `R8G8B8A8` against `B8G8R8A8` scene targets. That rule is retired: the surface it picked is AC7's own render-resolution UI layer, which the widget quads draw into and the game composites itself, and promoting it cannot sharpen an interface that is rasterized as scene geometry. The interface is extracted instead; see the [representation plan](../../docs/representation-plan.md) and [AC7 UI extraction](../../docs/research/ac7-ui-extraction.md). The code still carries the set until M2 replaces it.

Not fixed by any of it: bloom is still computed from the render-resolution scene, so the glow composited over the reconstruction is low resolution, and post-process shaders addressing texels rather than sampling normalized will address the wrong ones, because their constants still describe the buffer the engine believes it has. Both are visible only in a rendered result and neither has been looked at. The plan also promotes the chain targets between the tonemap and the back-buffer draw, which reinsertion has been leaving at render resolution.

Run context work on the owning render thread. Save and restore all affected graphics state around injected work. Hook installation, rollback, and teardown must account for callbacks already in flight. Current gaps are in [the review](../../docs/review.md).

The decode and texture helpers have synthetic tests. The observer and DLSS debug path have recorded game runs. The render-target watch, the substitution plan, the state save and restore, and the plan `scene_reinsert` builds are cross-built and tested under Wine on DXVK against a real device, each checked by asking the context what actually got bound. The reconstruction input set is deliberately not tested synthetically, because it is recognized from a combination only a real engine frame produces. Whether the substitution produces a correct picture is not tested at all; that needs the game's own shaders reading the game's own constants. The egui renderer/input components compile but are not yet connected to the live runtime. See [the tracker](../../docs/implementation.md).
