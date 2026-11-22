# Godot engine module Rust ownership

This pass starts at RSF `f94f86f4` and uses Godot `ed1daf0bf001b61586d9930840f2f1394092c079`
(4.7.2). The question is which remaining sequencing can run in the SCons Rust object
while Godot continues to own commands, textures and resource retirement. The source
review followed the existing patch plan from the engine entry points to the submit and
Present callbacks, then compared the old C++ records with the Rust handoff.

| Path | Rust ownership | Godot boundary |
| --- | --- | --- |
| `graph.cpp`, `graph.h` | `graph.rs` validates the current recording and timeline, picks pool reuse or allocation, and sequences begin, label closure, source consumption and label reopening. | The C++ scope holds the live command-ID reference and label callback. Its driver leaves operate on the Godot pool. The engine frame fence owns reuse, including the binary semaphore links. |
| `bridge.cpp`, `pass.h`, `motion.cpp` | `pass.rs` assembles the public pass. `packet.rs` assembles the route payload, dispatches it and constructs execution-event passes. `motion.rs` owns shader, pipeline and sampler RIDs, chooses the output surfaces and packs half-jitter constants. | Godot reads native textures, submits resource-usage declarations, creates uniform sets, dispatches the original compute shader and owns scene-buffer textures. Camera math and its previous projection/transform retain Godot's typed operations. |
| `vulkan.cpp`, `rsf_godot_engine.h` | `present.rs` snapshots the session and render source and admits a receipt only for the exact queue and fence. `packet.rs` fills the swapchain ABI header, captures before trying a proxy and distinguishes skipped, refused and created calls. | The engine scope remains thread-local RAII. Swapchain ownership, queue locking, native Vulkan calls and Vulkan graph metadata remain on the driver. Native marker fallback remains available only when the execution callback is unsupported. |
| `frame.h`, `cpu_facts.h`, `host.h`, `primary.h`, `viewport.h` | Existing Rust owners continue handling frame and viewport state, CPU facts, boot and configuration. | These headers declare the private C ABI used by Godot. |

The Godot graph continuation is begun before ending the source. Once the source is
consumed, C++ updates the exact graph-owned command reference and Rust reopens labels
on the continuation, including when the provider later records fallback. Godot exposes
individual semaphore retirement in `RenderingDeviceDriverVulkan::semaphore_free`, while
`command_pool_free` retires created command buffers. Allocation now creates the semaphore
first and frees it if command allocation fails. A begin failure leaves the pair owned
by the engine pool for reuse. The existing graph fixture checks those refusals.

`RenderingDevice::_execute_frame` owns the Vulkan execution scope across
`execute_chained_cmds` and any separate present queue call. The final chained submit
carries the frame fence. The patch plan's submission begin precedes `vkQueueSubmit`;
the end receipt follows the success check and precedes `vk_submitted`. Rust matches
the same queue and fence for both submit boundaries before permitting Present. The
host receives the captured session and source from that scope even if another render
frame has since been queued. The patch plan and engine anchor text are unchanged.

SR uses render-sized color, depth and motion with target-sized output. Final scene
carries only the target-sized HUD-less color and repeats the recorded SR identity.
Vulkan still uses image descriptors with copy capability read from Godot and null
public resource pointers. D3D12 still uses borrowed native texture pointers and
invalidates its engine command cache after returning from the host. The decoder GLSL
and bindings are unchanged. The static-motion sentinel still selects camera
reprojection, with normalized depth produced only on Vulkan. In Godot,
`compute_pipeline_create` registers a dependency on the shader and `free_rid` retires
those dependencies. Rust now rolls back partial decoder initialization so a retry
has a complete shader, pipeline and sampler.

`eng/patch-godot.py` remains a source patch tool. `register_types.cpp` and
`register_types.h` own ClassDB registration, singleton lifetime and the Godot
initialization level. `d3d12.cpp` owns the native transition flush and command cache.
The graph fixtures retain a C++ engine seam in `godot_graph_handoff.cpp` and
`godot_graph_stubs/` so they exercise the actual lease and the Rust decision path
together. These boundaries continue to require Godot C++ or the patch language.

Evidence is source-only. The in-memory patch plan resolved all 21 files from the pinned
revision. Checks covered Vulkan submit and separate Present ordering, unchanged motion
GLSL, pin-matched formatting, bindgen 0.72.1 regeneration and locked offline Cargo
metadata. Engine compilation, fixture execution, link validation and moving-scene/device
behavior remain unverified.
