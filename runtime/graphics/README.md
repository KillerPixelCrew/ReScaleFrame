# Graphics helpers

D3D11 utilities shared by the research proxy and runtime. Game-specific view interpretation lives in `games/ac7`; vendor evaluation lives in `runtime/backends`.

| Module | Purpose |
| --- | --- |
| `d3d11_observer` | Track selected allocations and invoke a Present callback |
| `resource_roles`, `frame_tap` | Classify resources and find candidate colour/depth/motion bindings |
| `resource_ref` | Retain and release COM resources across the C ABI |
| `constant_buffer_read` | Stage and read a buffer with type/device checks |
| `motion_decode` | Convert biased velocity to float motion and preserve unwritten pixels |
| `texture_dump` | Read supported texture formats into diagnostic TGA/JSON files |
| `present_blit` | Show reconstructed scene colour over the back buffer |
| `overlay_renderer`, `overlay_input` | Render egui meshes and collect window input |

The frame tap uses format and binding heuristics. It does not yet identify a verified AC7 shader, view, and frame. A retained texture can still be overwritten by the game; choose the consumption or copy point explicitly.

Run context work on the owning render thread. Save and restore all affected graphics state around injected work. Hook installation, rollback, and teardown must account for callbacks already in flight. Current gaps are in [the review](../../docs/review.md).

The decode and texture helpers have synthetic tests. The observer and DLSS debug path have recorded game runs. The egui renderer/input components compile but are not yet connected to the live runtime. See [the tracker](../../docs/implementation.md).
