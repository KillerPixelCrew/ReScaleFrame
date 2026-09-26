# Graphics helpers

D3D11 utilities shared by the research proxy and runtime. Game-specific view interpretation lives in `games/ac7`; vendor evaluation lives in `runtime/backends`.

| Module | Purpose |
| --- | --- |
| `d3d11_observer` | Track selected allocations and invoke a Present callback |
| `resource_roles`, `frame_tap` | Classify resources, find candidate colour/depth/motion bindings, and describe the draws into a named target |
| `resource_ref` | Retain and release COM resources across the C ABI |
| `constant_buffer_read` | Stage and read a buffer with type/device checks |
| `motion_decode` | Convert biased velocity to float motion and preserve unwritten pixels |
| `scene_promote` | Put a reconstructed scene back into the game's frame by promoting its composite, the interface layers the classifier names, and the chain between the tonemap and the interface composite, all to output resolution |
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

So the mechanism stays and the insertion point changes: promote AC7's own interface layers and let
the game composite them, which keeps the colour and the glow, and take the frame generation layer
from a promoted layer later, since it already holds premultiplied colour with coverage. See
[the extraction note](../../docs/research/ac7-ui-extraction.md) for the measurements and for the
argument this reverses.

## Scene promotion

The debug view draws the reconstruction over the finished frame, so the image is ungraded and everything the game composited after it, the interface included, is gone. `scene_promote` builds the substitution instead: it promotes the composite, the interface layers and the chain to output resolution, points scene colour at the reconstruction, and hands the tap a plan. `rsf_frame_tap_set_plan` swaps those bindings before forwarding them, in the output merger, in the pixel stage, and in `ClearRenderTargetView`, and scales viewports and scissor rectangles while a promoted target is bound. The game then grades the reconstruction with its own shaders, rasterizes its widget quads at output resolution into its own layers, composites them itself, and its final upscale into the back buffer becomes a copy. Nothing is removed from the frame.

Four parts of that plan are decisions rather than mechanics:

- Scene colour is gated on the composite being bound. The scene passes read scene colour while they are still writing it, so an ungated substitution hands a lighting pass a reconstruction of a frame it has not finished, which is a feedback loop rather than an upscale.
- The gate is also where the reconstruction runs. It is the last point before anything reads scene colour and the first where the scene is whole; evaluating at Present would leave the scene a frame behind the grade and interface drawn over it. The price is a mid-frame evaluate, where the game will not rebind what it believes is still bound, which is what `d3d11_state` is for.
- The interface layers are named by the draw classifier in `games/ac7`, as the targets it sees widget quads drawn into, and by nothing else. Every rule of the form "the surface with this format" or "the one read by a draw of this shape" picked something else as well at least once; the classifier's answer is a draw the game actually made. The loader keeps each layer for as long as quads keep landing in it and drops it after two seconds without, because the engine's pool retires a layer on a screen change and a plan naming a retired one looks healthy while the interface goes back to being magnified.
- The chain, the eight-bit intermediate between the tonemap and the interface composite, is named by a composite draw's inputs and confirmed by watching the draw that writes it read the composite. Without it the promoted composite is downsampled back to render resolution on its way to the interface composite, which is what made the first promotion runs cleaner but not sharper.

Not fixed by any of it: bloom is still computed from the render-resolution scene, so the glow composited over the reconstruction is low resolution, and the quads read that scene-sized blur chain alongside their widget texture, so the glow around the interface is low resolution too. Post-process shaders addressing texels rather than sampling normalized will address the wrong ones, because their constants still describe the buffer the engine believes it has. The quads bind the scene's depth, and the tap's default policy drops it at a promoted binding, so a panel is never occluded by scene geometry in front of it. All of it is visible only in a rendered result and none of it has been looked at with this plan.

## Present is hooked by detour, beside the other hooks

Steam's overlay and RivaTuner Statistics Server both patch the entry of dxgi's Present and re-check it on their first present, one taking what it displaced as its original and the other dropping it. A vtable patch there recursed until the stack overflowed on the first Windows launch. The observer now follows the jump chain from the entry to the first function inside a module, the outermost hook's own function or dxgi's Present when nothing is patched, and detours that with MinHook; nobody re-asserts its own function's prologue. A trampoline over the file's bytes for the entry reaches the genuine body for a re-entered call. `overlay_input` uses the same library to detour the user32 cursor functions while the panel is open, which is how the panel takes a mouse in a game that warps and hides it. [The note](../../docs/research/windows-present-hook-coexistence.md) has the measurements.

## The Windows runtime rewrites its vtable

The stock D3D11 runtime keeps the immediate context's vtable on the heap and rewrites the whole work-submission family of entries, draws, dispatches, copies and clears, whenever a flush-class call runs and again on the next piece of work, flipping between two implementations. Each rewrite discards patched hooks. DXVK's table is static, which is why every Wine run of the tap's test passed and why no Windows run of this project could have observed a draw after the first read-back. Every hook now checks a sentinel slot on entry and re-applies the table when it is gone, the work-submission hooks check again on the way out, the flush-class calls are hooked as pass-throughs for that check, and `rsf_frame_tap_refresh` covers the flush inside Present. `vtable_refreshes` in the status counts it. [The measurement](../../docs/research/d3d11-runtime-vtable-rewrite.md) has the slot table.

Run context work on the owning render thread. Save and restore all affected graphics state around injected work. Hook installation, rollback, and teardown must account for callbacks already in flight. Current gaps are in [the review](../../docs/review.md).

The decode and texture helpers have synthetic tests. The observer and DLSS debug path have recorded game runs. The render-target watch, the substitution plan, the state save and restore, and the plan `scene_reinsert` builds are cross-built and tested under Wine on DXVK against a real device, each checked by asking the context what actually got bound. The reconstruction input set is deliberately not tested synthetically, because it is recognized from a combination only a real engine frame produces. Whether the substitution produces a correct picture is not tested at all; that needs the game's own shaders reading the game's own constants. The egui renderer/input components compile but are not yet connected to the live runtime. See [the tracker](../../docs/implementation.md).
