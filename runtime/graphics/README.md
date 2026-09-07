# Graphics helpers

D3D11 utilities shared by the research proxy and runtime. Game-specific view interpretation lives in `games/ac7`; vendor evaluation lives in `runtime/backends`.

| Module | Purpose |
| --- | --- |
| `d3d11_observer` | Track selected allocations and invoke a Present callback |
| `resource_roles`, `frame_tap` | Classify resources, find candidate colour/depth/motion bindings, and describe the draws into a named target |
| `resource_ref` | Retain and release COM resources across the C ABI |
| `constant_buffer_read` | Stage and read a buffer with type/device checks |
| `motion_decode` | Convert biased velocity to float motion and preserve unwritten pixels |
| `scene_reinsert` | Put a reconstructed scene back into the game's frame, keeping its grade and interface |
| `d3d11_state` | Save everything the device context has bound, and put it back |
| `texture_dump` | Read supported texture formats into diagnostic TGA/JSON files |
| `present_blit` | Show reconstructed scene colour over the back buffer |
| `overlay_renderer`, `overlay_input` | Render egui meshes and collect window input |

The frame tap uses format and binding heuristics. It does not yet identify a verified AC7 shader, view, and frame. A retained texture can still be overwritten by the game; choose the consumption or copy point explicitly.

`rsf_frame_tap_watch_target` asks where the reconstruction goes back in. It names a render target and describes the next few draws into it: extent, the viewport actually in effect, the ordinal within the pass, indexed or not, and every pixel shader input with its slot. Pointed at the back buffer, the single texture that draw reads is the composite; pointed at that composite, the draw that reads scene colour is the tonemap. Neither fact is available from the captures here, because the exported action list records render-target bindings and not shader resource bindings. Each watch carries a draw budget: watching the back buffer holds a reference on it, which makes `ResizeBuffers` fail, so a permanent watch trades an answer for a later mode change that breaks.

## Scene reinsertion

The debug view draws the reconstruction over the finished frame, so the image is ungraded and everything the game composited after it, the interface included, is gone. `scene_reinsert` builds the substitution instead: it promotes the composite and the interface's own target to output resolution, points scene colour at the reconstruction, and hands the tap a plan. `rsf_frame_tap_set_plan` swaps those bindings before forwarding them, in the output merger, in the pixel stage, and in `ClearRenderTargetView`, and scales viewports and scissor rectangles while a promoted target is bound. The game then grades the reconstruction with its own shaders, draws its own interface over it at output resolution, and its final upscale into the back buffer becomes a copy. Nothing is removed from the frame.

Three parts of that plan are decisions rather than mechanics:

- Scene colour is gated on the composite being bound. The scene passes read scene colour while they are still writing it, so an ungated substitution hands a lighting pass a reconstruction of a frame it has not finished, which is a feedback loop rather than an upscale.
- The gate is also where the reconstruction runs. It is the last point before anything reads scene colour and the first where the scene is whole; evaluating at Present would leave the scene a frame behind the grade and interface drawn over it. The price is a mid-frame evaluate, where the game will not rebind what it believes is still bound, which is what `d3d11_state` is for.
- The interface target is identified by format, `R8G8B8A8` where every scene target in this tail is `B8G8R8A8`. That is one observed difference in one game, so an input that does not match leaves the interface magnified with the scene and says so rather than promoting on a guess.

Not fixed by any of it: bloom is still computed from the render-resolution scene, so the glow composited over the reconstruction is low resolution, and post-process shaders addressing texels rather than sampling normalized will address the wrong ones, because their constants still describe the buffer the engine believes it has. Both are visible only in a rendered result and neither has been looked at.

Run context work on the owning render thread. Save and restore all affected graphics state around injected work. Hook installation, rollback, and teardown must account for callbacks already in flight. Current gaps are in [the review](../../docs/review.md).

The decode and texture helpers have synthetic tests. The observer and DLSS debug path have recorded game runs. The render-target watch, the substitution plan, the state save and restore, and the plan `scene_reinsert` builds are cross-built and tested under Wine on DXVK against a real device, each checked by asking the context what actually got bound. The reconstruction input set is deliberately not tested synthetically, because it is recognized from a combination only a real engine frame produces. Whether the substitution produces a correct picture is not tested at all; that needs the game's own shaders reading the game's own constants. The egui renderer/input components compile but are not yet connected to the live runtime. See [the tracker](../../docs/implementation.md).
