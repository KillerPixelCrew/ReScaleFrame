# Godot native integration ownership

Pinned references: Godot `4.7.2` (`ed1daf0bf001b61586d9930840f2f1394092c079`) and godot-cpp
(`8070599885b6df675fcad033dd032ce44be2b850`). Nothing here was built, linked or run in the
session that last revised it; the checks that exist are source comparison, Cargo fixtures and
pinned-toolchain formatting. See `docs/research/godot-engine-integration.md` for device evidence.

## GDExtension on the C interface

`ReScaleFrame.Godot.Native.dll` has no first-party C++ and no longer links godot-cpp.
`native/gdextension.rs` implements the `rsf_godot_initialize` entry and registers
`ReScaleFrameEffect` (`CompositorEffect`) and `ReScaleFrameRuntime` (`RefCounted`) through
Godot 4.7's C interface. `gdextension/effect.rs` holds the class, its per-instance probe state
(the pending capture request and the snapshot and capture published under one lock), the scene and
RenderingDevice reads and the motion readback objects; `effect.rs` classifies the readback.
`gdextension/runtime.rs` reads the option Dictionary into typed settings for the engine service,
and writes the renderer names and status Dictionary. `gdextension/api.rs` loads the interface,
the builtin constructors and the method binds by their pinned 4.7 hashes.

`native/runtime_platform.c` is the only native source. This C11 leaf supplies the CMake-selected
optional Vulkan symbols and the CMake version, and registers `rsf_godot_host_unload` with `atexit`
from a CRT initializer. `exports.def` exports the entry. CMake builds the DLL when
`RSF_GODOT_NATIVE=ON`, which `eng/build-godot.ps1` passes. The `api.h` records and the Runtime CPU
service are bindgen output (`build.rs`).

## Rust owners in the native DLL

`ReScaleFrame.Godot.Native.dll` links the `rsf-godot` staticlib (`native/lib.rs`) next to the
same OrchestratorCore. As in the former `runtime.cpp`, `vulkan.cpp` and `vulkan_creation.cpp`,
each owner keeps its state in one place and calls the Runtime, D3D12, DXGI, Win32 and Vulkan
directly. The existing engine ABI keeps its function signatures and record layouts. ABI checks
read the size before the version and validate output headers without borrowing uninitialized
bodies.

| File | Owns |
| --- | --- |
| `native/control.rs` | Requested settings and their generation, the `rsfg_configure` refusal rules (applied before either backend sees a request), engine-module service routing (`rsfg_attach_engine`, `rsfg_configure_engine`, `rsfg_reset_history`), the D3D12 FG install and the D3D12 status record (ABI 3) with latency availability, mode and counters read from the Runtime latency owner |
| `native/d3d12.rs` | D3D12 render path: the engine's first device and queue published only with their completion fence, the SR context plan (Auto resolved by the Runtime's `rsf_gpu_default_sr_backend`, spatial mapped to the normalising context because Godot evaluates FSR1 itself, fallback to backend 0 when a provider is not created), the command slots, the Present receipt committed only by a recorded pass, HUD-less slot admission bound to the source's SR slot, and the capture owner |
| `native/window.rs` | Input window admission (the first window of the process whose class is `Engine`, asked by the Runtime's `accept_window` callback and by `rsfg_vk_window`, kept until `rsfg_shutdown`), per-HWND subclass forwarding that survives a refused restoration, so a replacement window attaches independently, and the Runtime CPU service with the Present execution receipt |
| `native/runtime.rs` | The entries that route to the booted backend, the Vulkan boot (the Runtime host's boot plus the CPU service), the primary-window report, the Present markers and the engine's own WSI calls under the Runtime's Streamline scopes |

The Vulkan session is the Runtime's Vulkan host (`runtime/orchestrator/src/vulkan_host.rs`,
contract `rescaleframe/native_vulkan_host.h`), not Godot code: boot state and identity, requested
settings and generation (kept across sessions), provider admission and Auto resolution, the FG
presenter, the SR session with its retirement, the frame capture (`vulkan_host/capture.rs`), the
status record with the Streamline Vulkan latency overlay, and the creation resolver
(`vulkan_host/creation.rs`: the owner instance, XeSS preparation, the XeSS, Win32 interop and
latency stages with their fallbacks, and a receipt of the accepted stages and the adapter for each
successful `vkCreateDevice`). `api.h`'s setup, settings, status and Vulkan pass records are that
host's records under their Godot names; `exports.def` publishes the host's presenter and pass
entries as the `rsfg_vk_*` and `rsfg_render_vulkan*` exports the engine module resolves. Godot
prepares the inputs (the packet, the graph split, the primary window) and presents what the host
returns.

The native integration binds one versioned Runtime CPU service at backend setup: CPU facts and
window messages use its report callback, diagnostic pings its ping callback. Godot admits the window;
Runtime owns marker mapping, ordering and pacing. D3D12 capture uses the Runtime's
`rsf_core_capture12_*` owner (`frame_capture12.rs`); the leaf flushes the slot before the HUD-less
copy, calls Runtime FG afterwards, and shutdown flushes all slots before destroying SR and cancelling
the session dump. A frame that reaches SR through the sharpening record uses the type generated from
the public C header (`rsf_game_sys::rsf_sharpening_options`) shared by Godot, Unity and Runtime.

### Explicit Present execution on Vulkan

Godot's main RenderingDevice submits the last graph command with the frame fence in
`execute_chained_cmds`; `_execute_frame` then presents inside that driver call or calls the separate
presentation queue with the signalled semaphore. An execution scope copies the session and source
for this main-device call. The Vulkan driver reports submit begin immediately before queue submission
and submit end only after success, and brackets the actual native, proxy or mixed-batch Present call
for the admitted HWND, skipping fan-out failures and Present calls without a matching submitted
frame. The Runtime carries the explicit source during Present for the XeSS provider's identity
lookup. No pacing or marker semantics are chosen in the engine. The receipt export is optional at
lookup, so older modules and DLLs keep their entry points.

## Vulkan Auto through the Runtime provider policy

The Runtime Vulkan host does not pick a vendor on its own (Rule 3); it asks the same Runtime policy as the
D3D12 path, with the facts it has. Explicit SR and FG requests, their admission and refusal order and
the FSR4 lease admission are unchanged.

- SR uses `rsf_gpu_select_sr_backend(vendor, software, choices)`: no provider on a software
  adapter, otherwise NVIDIA DLSS, Intel XeSS, anything else FSR3, then FSR3, then XeSS among the
  choices. FSR4 is never chosen.
- FG takes `rsf_fg_default_backend(vendor, software)` and passes it to
  `rsf_fg_select_auto_backend(preferred, choices)` (`fg_choice.rs`): the preference when served,
  otherwise FSR3 when served, otherwise none.
- Facts: adapter vendor, device and software (`VK_PHYSICAL_DEVICE_TYPE_CPU`) read by the creation
  owner after each successful `vkCreateDevice` (without facts the Runtime sees an unknown adapter
  and yields FSR3); Streamline availability from the start leaf's `streamline_ready`; and the
  servable providers (SR FSR3 always, DLSS with Streamline, XeSS with a device created with the XeSS
  requirements, FSR4 with its build, graph handoff and interop device; FG FSR3 always, XeSS with its
  build, interop device and primary window), so Auto never picks a provider admission refuses.
- DLSS-G is not offered to FG Auto. Its presenter's retirement after native secondary-window use
  still ends in `0xc0000005` in driver queue destruction
  ([DLSS lifecycle investigation](../../docs/research/godot-vulkan-dlss-fg.md)), and the presenter for the
  resolved provider exists even with FG mode off, so offering it would make the fault reachable from
  default settings. It stays an explicit opt-in until secondary-window retirement is proven safe.
  FSR4 FG is not offered either (its admission asks a provider probe).

| Adapter and facts | SR Auto | FG Auto |
| --- | --- | --- |
| NVIDIA, Streamline session started | DLSS | FSR3 (DLSS-G not offered) |
| Intel, XeSS device | XeSS | FSR3, then XeSS once interop and the primary window exist |
| CPU device type | none: spatial fallback and a refused frame | FSR3 |
| AMD, unknown, no facts | FSR3 | FSR3 |

Intel FG caveat: `bridge.cpp` reports the primary window in `configure` just before
`native_configure`, after the startup swapchain already created an FSR3 owner, so the first configure
resolves Auto to XeSS and the next acquire recreates the WSI owner as a fixed-guide XeSS owner. No
Intel FG run was reviewed; the XeSS FG count evidence is from an NVIDIA device limited to one generated
frame ([audit](../../docs/research/godot-xess-mfg-capability-audit-20261025.md)). On a software
adapter `bridge.cpp` keeps Godot's temporal scaling mode and quality scale for Auto, so the engine
still renders at the reduced extent; the pass records `rsf_srvk_spatial_fallback` rather than
leaving the output unwritten.

## Remaining C++

The checklist below maps each earlier finding to its source evidence.

| Finding | Source evidence |
| --- | --- |
| Reusable D3D12 status header | `control.rs` writes the current ABI header (`status_header`). |
| Shared packaged D3D12 paths | `sdk/game/src/vendor_paths.rs` supplies Godot's session storage and Unity's package identity. |
| Simultaneous FG failures | `sdk/game/src/presentation_result.rs` keeps both facts and defines provider-switch precedence. |
| Completion fence before binding | `d3d12.rs` publishes the binding only with its fence, so a refused fence is retried on the next pass. |
| Present recording receipt | `d3d12.rs` commits the receipt only after a recorded pass; Present reports a refused one as a missed SR. |
| HUD-less slot admission | `d3d12.rs` matches the SR slot's session and source, waits on already-submitted work, and refuses a failed wait. |
| Size-first ABI validation | Raw header reads in `lib.rs` read the size word before the version. |
| Auto policy delegation | The Runtime Vulkan host (`vulkan_host.rs`) passes the adapter facts and the servable choices to the Runtime's selectors, whose own fixtures cover provider policy. |
| Device-bound readiness | The host's `vulkan_host/creation.rs` keeps a receipt per created device; its fixture (an `rsf-orchestrator` unit test) checks the receipt and its removal at `vkDestroyDevice`. |

The C++ had no tests of the D3D12, window and Vulkan session paths; their fake-leaf tests went with
the leaf layer in the C++-reference pass (PORT.md), and these paths need a Godot run to verify.

What still has to be C++, and why:

| File | Residual source |
| --- | --- |
| Engine module `motion.cpp`, `graph.cpp`, `d3d12.cpp`, `vulkan.cpp`, `bridge.cpp` | Godot camera matrix math (a Rust port would have to reproduce Godot's `Projection` and `Transform3D` inverses bit for bit) and the previous camera transform, projection and jump distance; native texture handles, packets and Vulkan image descriptions; the motion resolve pass and shader; the Vulkan graph scope; the D3D12 and Vulkan driver patches; and the singleton's Dictionary parser (kept separate from `gdextension/runtime.rs`'s checked parser pending a compatibility review, because Variant conversions differ in integer type and range checks). |

Residuals: DLSS-G stays out of Vulkan FG Auto (above); the Godot D3D12 leaves still pass software as
false; the motion readback's extent check against a velocity override; D3D12 fixed SR context inputs
(setup ABI 1 planning, inverted depth, the custom DLSS engine identity and its flag) and the capture
record mapping from the pass could move into the owner's request records; the addon (`settings.gd`,
`controller.gd`, `csharp/ReScaleFrame.cs`) keeps its own option defaults for the Inspector resource.

## Engine module

At RSF `31a06a94`, `eng/build-godot-engine.ps1` passed `custom_modules` to Godot, and
`modules/SCsub` collected the external module's C++ objects into `module_rescaleframe`. At runtime
the module loads `ReScaleFrame.Godot.Native.dll`, resolves the existing `rsfg_*` functions and
registers `rsf_godot_engine_services`. The controller calls the GDExtension's `configure`, which
composes settings in `gdextension/runtime.rs` and hands them back through that service table;
`configure_from_extension` reconstitutes the Dictionary for the native bridge.

`modules/rescaleframe/SCsub` invokes `rustc --emit=obj` from the repository root (rustup reads
`rust-toolchain.toml`) with the SCons Windows target architecture and compiler ABI. Godot archives
the resulting `no_std` object with the module objects for standalone editors and export templates,
including .NET builds. The Rust files in that object (`viewport.rs`, `cpu_facts.rs`, `frame.rs`,
`primary.rs`, `host.rs`, `pass.rs`) use private C linkage declared in matching `.h` files that assert
the shared layouts; they have no Cargo or godot-cpp dependency, export nothing from the Godot
executable or the native DLL, and are explicit source dependencies in SCons. The target is Windows
x64 MSVC; the Rust target must match the Godot build, which is unavailable for ARM32 MinGW.

### Viewport policy (`viewport.rs`)

Rust owns the requested engine scaling class and the initial scale. Off selects native-size
bilinear, Spatial selects Godot's FSR spatial path, and the remaining requests, including Auto,
select Godot's FSR2 temporal path. Godot's native temporal pass still runs before the RSF hook at
tonemapping. Initial per-axis scales are `1`, `2/3`, `1/1.7`, `1/2`, `1/3`, `0.77` with the
double-to-float conversion. `rsf-upscaler::Quality::scale` evaluates Balanced in single precision and
has no Ultra Quality entry, and its rounded extent helper differs from the native viewport's size
calculation. The DLL keeps request validation and provider selection.

The bridge checks the root `RID`, queues the primary render target, and applies the typed Godot mode
and scale. On each temporal frame it uses a matching reported width ratio; at the guarded viewport
sizing hook it uses exact reported dimensions when both output axes match and the render extent is
nonzero. Each status-driven update requires the classified request to still be current. Off restores
the exact output size and Spatial keeps Godot's dimensions.

### CPU event records and frame lifecycle (`cpu_facts.rs`, `frame.rs`)

`cpu_facts.rs` builds the version 1 `rsf_game_cpu_event` records. `bridge.cpp` samples the Windows
performance counter and frequency at the reporting points, supplies the session and source identity,
and passes the stack-owned record to `rsfg_cpu` synchronously. All records are keyed; input with no
open CPU frame uses source `0` and the unframed flag. Godot's keyboard, mouse/touch and joypad
mapping and emulated-input exclusion stay in the native input patch, and the frame-end
expected-render count comes from the draw's `wants_present`. These records carry a zero window
message ID; the window procedure supplies message facts through its separate path, and Runtime
decides pacing and marker semantics.

`frame.rs` owns the open CPU frame, its identity, the simulation-ended and queued flags and the CPU
frame interval, and on the render thread the queued render frame identity, gameplay flag, interval
and the last SR-evaluated frame, in relaxed atomics with the same writer thread as before. Rust
decides the order and calls three typed native callbacks: the engine clock (`OS::get_ticks_usec`),
the record report (QPC sample, builder, `rsfg_cpu`) and the render handoff (Godot pause and window
focus, `call_on_render_thread`). The patch sites in `eng/patch-godot.py` are `Main::iteration` after
`iterating++` (begin, then simulation begin), before both `RenderingServer::draw(wants_present, ...)`
calls (queue), before `iterating--` (end), the Windows run loop before `process_events` and the
display server's key processing while not iterating (begin), and `Input::_parse_input_event_impl`
(input).

- Begin, only when ready and enabled: no-op while a frame is open; otherwise the clock is read, the
  interval is the wrapping microsecond difference over 10^6 (1/60 s for the first frame), the
  identity increments with wraparound, the flags reset, then `FRAME_BEGIN` and `INPUT_SAMPLE` are
  reported.
- Simulation begin: `SIMULATION_BEGIN` only with a frame open.
- Queue, only when ready and enabled: `SIMULATION_END` (with no open-frame check), then the ended and
  queued flags, then the handoff of the identity (zero when the draw does not present) with the CPU
  interval. Godot's pause and focus are read after the report.
- End, with no ready check: `SIMULATION_END` unless the queue reported it since the last begin, then
  `FRAME_END` with render-expected `1` only for a presenting draw, then the frame closes. The frame
  stays open during both reports so input delivered meanwhile stays framed.
- Input, after the native kind mask check: the open frame's identity, otherwise source `0` with the
  unframed flag. Lifecycle reports require a nonzero identity and are gated on ready and enabled.

On the render thread the queued begin stores the identity, gameplay flag and interval before
`rsfg_render_begin`. The SR and final-scene hooks read one snapshot and refuse identity zero; the
final-scene hook requires the last SR pass to carry the current identity. The sequence break is
`last_evaluated + 1 != current` with wraparound. A recorded SR pass stores its identity as
evaluated, and a new primary target clears it. The screen class, the 0.25 s gap and the camera frame
time use the snapshot.

### Primary viewport identity and render admission (`primary.rs`)

`primary.rs` owns the primary viewport and render target (RIDs cross as `RID::get_id()`), the resource
generation, the history reset request, the last recorded SR pass's color texture, internal extent
and source frame, and the admission of the sizing, camera, SR and final-scene hooks. The C++ keeps
every typed RID, Godot read and call, `ready` and `enabled`, the thread handoff, the packet,
`driver_callback_add` and the previous camera transform and projection.

Callback triggers, in the order they run with `eng/patch-godot.py` applied:

1. `ReScaleFrameBridge::configure` (the singleton call, or the GDExtension through
   `configure_from_extension`), after `rsfg_configure` accepts: `call_on_render_thread(set_primary)`
   runs `rsf_godot_primary_set` on the rendering server thread (identity stored, generation
   incremented, `frame.rs` evaluation cleared). The main thread then issues
   `viewport_set_scaling_3d_mode` and `viewport_set_scaling_3d_scale`. A changed scale reaches
   `_configure_3d_render_buffers`, whose patched sizing hook calls `RSFGodot::render_size`, admitted
   by `rsf_godot_primary_sizing`. `enabled` is stored on the main thread after these calls, so with a
   separate render thread the first sizing hook can still see the host inactive.
2. Each presenting or non-presenting `Main::iteration` draw: `queue_frame` queues `set_render_frame`
   before `RenderingServer::draw`. On the server thread it stores the render frame, calls
   `rsfg_render_begin`, reads the primary viewport and, for a temporal request with a matching status,
   calls `viewport_set_scaling_3d_scale` directly.
3. `RenderingServer::draw`: `_draw_viewport` creates missing 3D render buffers (sizing hook again),
   then each scene render reaches `RenderForwardClustered::_render_scene`. With valid scene buffer
   data, `RSFGodot::upscale` runs after Godot's temporal pass and `_debug_draw_cluster`, before the
   `Tonemap` timestamp, and `RSFGodot::hudless` right after `_render_buffers_post_process_and_tonemap`.
   Every scene render reaches both hooks, so admission refuses other viewports, probes and multiview.
4. `RendererViewport::viewport_attach_camera`, a server-thread command in queue order, when the camera
   differs: `RSFGodot::camera_changed` calls `rsf_godot_camera_changed`.
5. `ReScaleFrame.reset_history()` and the GDExtension's reset service: `rsf_godot_reset_request`, a
   sequentially consistent store.

Admission: host ready and enabled, nonzero source frame, valid render buffers, their render target
equal to the primary target, one view, and a `d3d12` or `vulkan` rendering driver; the final-scene
hook also requires the last recorded SR pass to carry the current source frame. In the SR hook, after
valid color, depth, motion and output textures, a color texture other than the last recorded one
increments the generation first and the pass carries the generation after it. Reset is the consumed
request, then a different color texture, a different internal extent, the `frame.rs` sequence break,
a frame interval above 0.25 s, or a camera origin more than 20 units from the last recorded one. Every
pass that reaches this point consumes the request, including passes later refused for an invalid
resolved motion or depth texture, which also keep their generation increment and record nothing.
Recording stores internal extent, color texture, `frame.rs` evaluation and recorded source after the
resolved textures are valid. Initial state: generation 1, a pending reset, no primary, no recorded
color, extent or source. The rendering server thread is the only writer and reader of this state.

### Host lifecycle (`host.rs`)

`host.rs` owns `ready`, `enabled`, the session and requested SR state, the boot admission and order,
the `configure` sequence, the shutdown order, and the status-driven extent updates at the queued
render begin and at the patched viewport sizing hook. The C++ keeps the typed leaves and Rust decides
when each runs: Godot project-setting reads (`GLOBAL_DEF`, the three path settings only after
admission), editor hint, rendering driver names, scene-tree root check, Dictionary parse with Godot's
Variant conversions, `call_on_render_thread(set_primary)`, the scaling mode mapping and setters, the
render thread motion release and `sync`; the module loader (`LoadLibraryW`, required export resolution
with early refusal, the two optional capture exports, `rsfg_attach_engine`); and the driver calls
(`rsfg_vk_host_capabilities`, `rsfg_boot_vulkan` or `rsfg_boot`, `rsfg_vk_window`, `rsfg_configure`,
`rsfg_status`, `rsfg_render_begin`, `rsfg_shutdown`).

- Boot: nothing when `rescaleframe/enabled` is false, the display driver is the null driver or the
  editor runs; otherwise load and resolve, the graph handoff capability, the driver-selected start,
  `ready` from its result, the rejection log, attach (a refusal clears `ready`), then the session from
  status, zero when status refuses.
- Configure: refused while not ready, for an invalid viewport or one that is not the root viewport;
  then parse, the main window report on Vulkan, `rsfg_configure` (a refusal returns false with nothing
  else changed), the requested SR, the queued primary, the mode and initial scale, and `enabled` last.
- Shutdown: `enabled` cleared, the motion release on the render thread and `sync` when the rendering
  server exists, then `rsfg_shutdown` only while ready. `ready` stays set.
- Render begin: the `frame.rs` store, `rsfg_render_begin`, then for a temporal request with a primary
  viewport, accepted status reporting the same request, a still-current request and nonzero render and
  output widths, the scale `render_width / output_width` in double precision narrowed by Godot's float
  setter.
- Sizing: the `primary.rs` sizing admission with ready and enabled. Native uses the output size,
  Spatial keeps Godot's size, others use the reported render size when status accepts and reports the
  same request, the request is current, both output axes match and both render axes are nonzero.

### SR and final-scene pass records (`pass.rs`)

`pass.rs` decides the SR and final-scene pass records and keeps the last recorded SR pass. The
private `rsf_godot_pass_record` and `rsf_godot_sr_facts` records, every `rsf_camera_frame` offset
(camera ABI 1, 384 bytes) and the role, flag, screen and camera ABI values are asserted on both sides.
The public `rsf_game_render_pass` has no Rust mirror: the C++ copies the record's non-pointer fields
into a zeroed pass (`pass_from`) and sets the native resource pointers per route. The SDK's bindgen
crate is not used because the SCons object is a standalone `no_std` rustc invocation.

Rust decides: role (SR, then FINAL_SCENE for the repeat), session (`host.rs`), source and native
frame, the view, history and viewport key (the render target), `PRIMARY` plus `RESET` from the
`primary.rs` decision, the resource generation, the screen class (FLIGHT in gameplay, else MENU),
`motion_to_uv` `-1, -1`, `motion_camera_included` and `camera_valid` `1`, the camera struct size and
ABI, render extent (internal size), output extent (target size), near, far and vertical FOV as Godot
reports them, `depth_inverted` `1`, the frame time narrowed to float, and the jitter in pixels as
`jitter * render_extent * 0.5` in single precision. The route comes from admission's driver fact:
Vulkan uses the motion pass's `depth_r32` copy as SR depth, samples the final-scene color and uses
the Vulkan packet; D3D12 uses Godot's depth, a color attachment read-write usage and the D3D12 packet.
`motion_jittered` and every other pass field stay zero. The C++ keeps the Godot reads, the FOV
conversion, the projection depth correction, the five camera matrices plus `previous_clip_to_clip`
(identity reprojection on reset), `resolve_motion`, the resolved texture checks, the previous
projection and camera assignments, native texture handles, callback resources, packets and Vulkan
image descriptions. A refused SR pass records nothing; the final-scene pass requires the recorded
source to equal the current frame and uses the primary target's slice as color input. A `real_t`
size assertion makes the single-precision Godot build (`eng/build-godot-engine.ps1`) a compile-time
requirement. Field types were checked against Godot 4.7.2 (`RenderSceneDataRD::z_near` and `z_far`
are `float`, `taa_jitter` and `prev_taa_jitter` are `Vector2`).

## Build requirements

- The Rust archive is defined through `rsf_add_rust_static` in every configuration.
  `eng/build-godot.ps1` builds `--target rsf_godot`, which also runs
  `cargo build --locked -p rsf-godot`. Cargo is already required by the root build.
- The motion classification runs in the `rsf-godot` Cargo tests. The Vulkan creation fixture (the
  port of `tests/godot_vulkan_creation.cpp`) moved with the creation resolver into the Runtime's
  Vulkan host and runs in `cargo test -p rsf-orchestrator --lib vulkan_host`.
- `exports.def` aliases the host's entries (`rsfg_vk_images=rsf_vkhost_images` and so on); the
  linker resolves them from the orchestrator archive the DLL links.
- `rsf_godot` carries a second Rust archive beside the orchestrator's, the same unverified
  multi-archive link as Fallout 4 and Minecraft. It has not been linked.
- `tools-requirements.txt` (SCons, NumPy, Pillow) serves the engine build and the visual analyzer
  only.
- Legacy or ignored: the godot-cpp CRT cache cleanup in `CMakeLists.txt` remains a workaround;
  `rsfg_vk_capture_setup` and `rsfg_vk_capture_release` stay optional exports the engine module
  resolves without failing; `api.h` defines `RSF_GODOT_SETTINGS_ABI_VERSION` before its includes.
