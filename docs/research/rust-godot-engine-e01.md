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

## E01 native follow-up

This follow-up starts at RSF `0251a81a` and reads Godot `ed1daf0bf001b61586d9930840f2f1394092c079`
from the local reference's Git objects. The question was which computations and fixture
scenarios could leave C++ while keeping the engine's typed operations and callback lifetimes.

| RSF paths | Pinned source and ownership |
| --- | --- |
| `integrations/godot/modules/rescaleframe/{camera.rs,pass.rs,pass.h,bridge.cpp}` | Godot `core/math/projection.h` and `projection.cpp` define the column-order product and `set_depth_correction(false)` defaults. Rust applies the same +Y-up, reverse-Z, zero-to-one correction and composes reprojection from the last recorded SR pass. The affine composition follows `Transform3D::operator*=`, `Basis::operator*=` and `Transform3D::xform`. Godot still evaluates projection inversion, affine inversion, FoV and camera distance. Its typed inverses retain the engine's pivot and singular-matrix behavior. |
| `integrations/godot/modules/rescaleframe/{motion.cpp,motion.h,motion.rs,motion.glsl,rsf_godot_engine.h}` | Forward+ clears temporal motion to `(-1,-1)` in `servers/rendering/renderer_rd/forward_clustered/render_forward_clustered.cpp:2236`. The matching scene shader writes previous minus current UV at `scene_forward_clustered.glsl:3112`. Rust supplies the GLSL source and selects the Vulkan depth-copy variant. The engine still owns shader compilation, texture RIDs, uniform sets and compute dispatch. Both variants preserve the shader source, UV units, half jitter and push-constant layout. |
| `tests/{godot_graph_handoff.rs,godot_graph_handoff.cpp,CMakeLists.txt}`, `integrations/godot/modules/rescaleframe/{viewport.rs,SCsub}` | Rust drives allocation, reuse and refusal scenarios against the actual `graph.cpp` lease. CMake enables the fixture module in the same Rust object only for the fixture. C++ keeps the typed doubles under `tests/godot_graph_stubs/`, scope lifetime and snapshots of the live command reference and pool. Godot's `rendering_device_graph.cpp:1127-1163` shows the paired pool allocation and `buffers_used` cursor. The fake driver supplies observable allocation, begin and dependency mutations without calling a driver. |
| `integrations/godot/modules/rescaleframe/{graph.cpp,graph.h,vulkan.cpp,d3d12.cpp}` | The graph and Vulkan leaves retain typed command-buffer and semaphore ownership. `RenderingDevice::_execute_frame` scopes the real Vulkan receipt, and the patch only reports submit completion after the `vkQueueSubmit` success check. D3D12's `CommandBufferInfo` owns cached bindings and `_resource_transitions_flush` owns pending transitions. These callbacks still borrow engine objects and run at the original driver sites. |
| `integrations/godot/modules/rescaleframe/{register_types.cpp,register_types.h,rsf_godot_engine.h}`, `eng/patch-godot.py` | Godot's `modules/register_module_types.h`, `main/main.cpp` and `core/object/class_db.h` define the scene initialization level, generated registration calls and ClassDB API selection. `GDCLASS`, method binding and singleton registration remain typed C++. The Python source patcher verifies the engine revision and exact source anchors, then inserts calls where private engine values are live. This source-patching boundary remains necessary for these hooks. |

The motion pass takes the same column-order reprojection values as the SDK record.
Reset uses identity. Missing resolved motion or depth returns before recording,
so a failed resolve leaves the last recorded camera intact. Retargeting invalidates
the receipt identity and clears frame continuity through the existing frame owner.
The Forward+ motion producer, scene-buffer
bindings and final scene hook retain their existing paths.

Source checks resolved the 21-file engine patch plan in memory and compared both
motion shader variants with the base revision. Rust 1.94.1 formatting, bindgen
0.72.1 regeneration and locked offline Cargo metadata passed. The patch plan was
read from Git objects because the reference worktree already contained older patches.
Engine compilation, linking, fixture execution and motion or presentation on a device
remain unverified for this follow-up.

## Driver and graph fixture continuation

At RSF `b5a7434c`, the graph fixture scenarios were in Rust, but the
synthetic Vulkan driver's state transitions and label assertions still ran in C++.
`tests/godot_graph_handoff.rs` now owns those transitions, faults and assertions.
`tests/godot_graph_stubs/drivers/vulkan/rendering_device_driver_vulkan.h` adapts
typed command and semaphore IDs to opaque Rust fixture state. The C++ fixture still
instantiates the production `GraphScope` over a live command reference and typed
pool, so the scenario can observe which command would record a provider fallback.
The Godot graph type double remains under `tests/godot_graph_stubs/servers/rendering/`.
The seam mirrors the pinned graph pool and Vulkan begin/retirement sites listed
above. Its transitions are synthetic and exercise no GPU work.

`packet.rs` also owns the order and selection of resource reads and command
recording by driver. `bridge.cpp` supplies typed reads through the private
`packet.h` callback table, with generated records in `packet_abi.rs`.
The pinned `core/templates/rid.h` defines zero as the invalid RID, and
`RenderingDevice::get_driver_resource` returns a 64-bit native value.
SR queries the Vulkan instance and physical device, along with render-sized
color, depth and resolved motion and target-sized output. Final scene queries
only its target-sized color. The D3D12 path keeps its queue and native texture
queries, transition flush and command cache invalidation. Callback declarations
remain in Godot and lease the same four resources in the same order.

`present.rs` now selects native, single-proxy and mixed Vulkan batches through
the generated `packet.h` table. A native batch still selects the first surface
whose execution receipt admits a marker. A single proxy forwards the original
request and a mixed batch preserves the extension refusal, optional wait fan-out,
chain order and last non-success result. `vulkan.cpp` still builds the exact
`VkSubmitInfo` and per-chain `VkPresentInfoKHR`, updates `pResults` and brackets
each mixed item after its typed request is ready. `VulkanExecutionScope` retains
thread-local receipt ownership from the pinned `RenderingDevice::_execute_frame`
through the final queue/fence submit and the later Present. The patch anchors and
submit event sites are the same as listed above.

Checks cover the in-memory pinned source plan, Rust 1.94.1 formatting,
bindgen 0.72.1 regeneration, locked offline Cargo metadata and a C++ syntax-only
check of the graph seam. Compilation and linking of the Godot module, fixture
execution and device behavior remain unverified.

## Engine policy follow-up from `818a5ede`

`graph.rs` now assembles the handoff and owns the pair allocation and rollback.
On a pool miss it creates the semaphore first. If command allocation fails, it
releases the semaphore. After append, Godot owns both resources even if command
begin fails. The pinned Vulkan driver exposes separate semaphore retirement
(`rendering_device_driver_vulkan.cpp:3101`) and the graph ties the paired pool
cursor to the engine frame. `graph.cpp` still verifies the live graph scope,
compares the exact typed command ID, invokes the Vulkan methods and changes the
borrowed command reference only after consuming the source. Fixture assertions
in `tests/godot_graph_handoff.rs` now check the actual generated handoff header,
including its driver identity and callback, through the C++ graph lease.

`present.rs` dispatches submit and Present reports with the session and source
captured by the existing thread-local execution scope. Submit still requires the
exact queue and fence. Present still requires a successful receipt when the
execution callback is available, and uses the legacy window marker only when
that callback is unavailable. `vulkan.cpp` keeps the typed callbacks, scope
nesting, queue locks and native Vulkan submit and Present calls. The pinned patch
still brackets real submit and reports its end only after the success check.

`packet.rs` now owns proxy admission and swapchain create and release order.
It registers capture before checking native WSI, timeline support and the
four-queue minimum. It preserves an owner returned during a refused create
for later retirement. Release still stops on capture refusal or failed proxy
destruction and clears native fields only after both owners retire. Typed
swapchain fields and the native fallback stay in `vulkan.cpp` and the pinned
driver patch.

`host.rs` sequences scene registration before singleton publication and host
shutdown before singleton removal. `register_types.cpp` keeps the generated
Godot module entry points, scene-level check, ClassDB API save and restore,
`GDREGISTER_CLASS`, and allocation and removal of the typed bridge singleton.
The pinned evidence is `main/main.cpp:3792,5296`,
`modules/register_module_types.h:38` and `core/object/class_db.h:535,568`.

The checks resolved all 21 patch-plan files in memory, regenerated the targeted
bindings without diff and passed Rust 1.94.1 formatting and locked offline
metadata. MinGW C++ syntax-only checks covered the real graph, Vulkan and
registration translation units against the pinned checkout's patched headers,
plus the graph fixture and private packet header. Engine object generation,
linking, fixture execution and device behavior were outside this source pass.

## Native host table and module entries

This pass starts at RSF `2b62be02` against the same pinned Godot revision. The question
was which logic still in the module's C++ did not need Godot's C++ API.

| RSF paths | Ownership after this pass |
| --- | --- |
| `native.rs`, `native.h`, `bridge.cpp`, `rsf_godot_engine.h` | `native.rs` calls `LoadLibraryW` and `GetProcAddress` and holds the 33 `rsfg_*` exports. The first 29 are required, matching the old resolve macro. `rsfg_vk_execution`, `rsfg_vk_capture_setup`, `rsfg_vk_capture_release` and `rsfg_attach_engine` stay optional. A missing required export publishes nothing. Rust gates each forwarder on readiness, activity or the Vulkan driver, timestamps and reports CPU events, and selects the render-begin route. bridge.cpp supplies the UTF-16 path, the `OS::get_current_rendering_driver_name` query as a leaf, and the Godot error print. The patched engine still calls `RSFGodot` names, now inline forwarders into Rust. |
| `present.rs`, `vulkan.cpp` | Receipt start, submit and Present call native.rs directly for the execution, support and legacy marker queries. `vulkan.cpp` keeps the `thread_local` scope stack and the private driver members. |
| `host.rs`, `register_types.h`, `bridge.cpp` | `initialize_rescaleframe_module` and `uninitialize_rescaleframe_module` are `extern "C"` Rust entries. Godot's generated `register_module_types.gen.cpp` passes its unscoped `ModuleInitializationLevel`, which converts to `int`. Rust checks the scene level (`GDEXTENSION_INITIALIZATION_SCENE`, 2, asserted in bridge.cpp) and keeps registration before publication and shutdown before removal. `register_types.cpp` is gone. Its `GDREGISTER_CLASS`, API save and restore and singleton leaves sit in bridge.cpp next to `_bind_methods`. The graph fixture builds the object without these entries because it has no engine. |
| `cpu_facts.rs`, `cpu_facts.h` | The CPU record builder is a plain Rust function, and the header is deleted. |

Behavior is unchanged with one exception. `vk_image_layout` used to call a null pointer
whenever the module had not loaded the host, such as with `rescaleframe/enabled` off on
Vulkan, because the patched render pass setup calls it for every swapchain. native.rs
now returns `VK_IMAGE_LAYOUT_PRESENT_SRC_KHR`, the layout the host gives a null owner.
`rsfg_vk_images`, `_acquire` and `_present` return `VK_ERROR_INITIALIZATION_FAILED`
without a host. Their callers reach them only with a proxy owner. Two fixes for rustc 1.100
also landed: closure annotations in `packet.rs` and a raw pointer capture in the graph
fixture's snapshot.

Remaining C++ follows from Godot's C++ API. The class is a `GDCLASS` `Object`. The driver
leaves are members of `RenderingDeviceDriverVulkan` and `RenderingDeviceDriverD3D12`
declared by the patch. `GraphScope` is a template constructed inside
`RenderingDeviceGraph::_run_render_commands`. Motion uses the RenderingDevice compute
API and the variadic `UniformSetCacheRD::get_cache`. The fixture's C++ instantiates
`graph.cpp` over typed doubles.

Checks: rustc 1.100 built the object with `-Dwarnings` for `x86_64-pc-windows-msvc` with
and without `rsf_graph_fixture`. The pinned 1.94.1 toolchain and the GNU target were not
installed. rustfmt passed. MSVC `/Zs` and MinGW `-fsyntax-only` accepted every module
translation unit against the pinned, patched headers. MinGW also accepted the patched
engine sources that include `rsf_godot_engine.h`, plus a probe of the generated module
registry caller. `main.cpp`, the D3D12 driver and `os_windows.cpp` were skipped because
their generated or Mesa headers are missing locally. The graph fixture was built by hand
with the CMake target's flags and passed, including the new unloaded-host cases. The
preset could not configure because of the tip parse error at `tests/CMakeLists.txt:451`.
Engine linking and device behavior remain unverified.
