# Godot native integration ownership

## GDExtension on the C interface

`ReScaleFrame.Godot.Native.dll` has no first-party C++ and no longer links godot-cpp.
`native/gdextension.rs` implements the `rsf_godot_initialize` entry and registers
`ReScaleFrameEffect` (`CompositorEffect`) and `ReScaleFrameRuntime` (`RefCounted`) through
Godot 4.7's C interface. `gdextension/effect.rs` holds the class, the `_render_callback`
virtual, the scene and RenderingDevice leaves and the motion readback objects.
`gdextension/runtime.rs` holds the option reads, renderer names, status Dictionary and
render-thread Callable. `gdextension/api.rs` loads the interface, the builtin constructors and
the method binds by their pinned 4.7 hashes. The effect and extension owners, property keys and
callback sequences are unchanged.

`native/runtime_platform.c` is the only native source. This C11 leaf asserts the private record
layouts, supplies the CMake-selected optional Vulkan symbols and the CMake version, and
registers `rsf_godot_host_unload` with `atexit` from a CRT initializer. `exports.def` exports the
entry. CMake builds the DLL when `RSF_GODOT_NATIVE=ON`, which `eng/build-godot.ps1` passes.

## Native control and D3D12 follow-up

The existing engine ABI retains its function signatures and record layouts. The DLL exports
control, status, CPU and several presentation notifications directly from Rust through
`native/exports.def`. ABI checks read the size before the version and
validate output headers without borrowing uninitialized bodies. The engine callback applies the
typed extension settings through the same root-viewport admission and scaling path as the public
Dictionary method.

D3D12 status returns ABI 3 and reads latency availability, mode and counters from the Runtime
latency owner. Godot and Unity use the same packaged vendor paths and give a nonzero provider
switch result precedence in their legacy FG result. The sharpening record derives from the public
C header for all three Rust consumers, with compile-time field offset checks.

A D3D12 binding publishes the device and queue after fence creation. Present reuses a receipt from
successful recording for its current session and source. HUD-less capture shares that source's
slot while its lists await submission, waits for completion if the slot was signalled, and
refuses it if the original SR slot wait failed. A native final-scene pass acquires its own retired
slot. Window subclass forwarding remains keyed by HWND through a refused restoration so a
replacement window can attach independently.

Rust plans the motion diagnostic's buffer size and dispatch and supplies its shader. The
GDExtension entry, class bindings, Variant/RID marshaling and RenderingDevice recording are Rust
on the C interface (see above). Rust handles the D3D12 COM, DXGI and Runtime frame capture calls.

The native integration binds one versioned Runtime CPU service at backend setup. CPU facts and
window messages use its report callback, and diagnostic pings use its ping callback. Godot admits
the window. Runtime still owns marker mapping, ordering and pacing.

Godot's pinned 4.7.2 source at `ed1daf0bf001b61586d9930840f2f1394092c079` shows the main
RenderingDevice submitting the last graph command with the frame fence in `execute_chained_cmds`.
`_execute_frame` then either presents inside that driver call or calls the separate presentation
queue with the signalled semaphore. An execution scope copies the session and source for this
specific main-device call. The Vulkan driver reports submit begin immediately before queue
submission and submit end only after success. It brackets the actual native, proxy or mixed-batch
Present call for the admitted HWND, skipping fan-out failures and Present calls without a matching
submitted frame. The Runtime carries the explicit source during Present for the XeSS provider's
existing identity lookup. No pacing or marker semantics are chosen in the engine.

The new receipt export is optional at lookup. Older modules and DLLs retain their original
entry points. The paired engine and native DLL use explicit execution facts. The pinned source
patch was generated and checked in memory. The engine checkout, game installations and
device fixtures were left untouched.

Source evidence is in the Rust entry points, the CMake export list, the engine's
`RSF_RESOLVE`/GetProcAddress calls, and recording fixtures for ABI reuse, failed fence creation,
failed SR admission and wait, and window replacement. Bindings were regenerated using bindgen
0.72.1 and formatted with the Rust 1.94.1 toolchain. Native linking, tests and device execution
remain pending.

### E02 checklist evidence

| Finding | Source evidence |
| --- | --- |
| Reusable D3D12 status header | `control.rs` writes the current ABI and `control/tests.rs` passes the same buffer twice through `rsfg_status`. |
| Shared packaged D3D12 paths | `sdk/game/src/vendor_paths.rs` supplies Godot's session storage and Unity's package identity. |
| Simultaneous FG failures | `sdk/game/src/presentation_result.rs` keeps both facts and defines provider-switch precedence. The Unity and Godot fixtures both specify simultaneous failures. |
| D3D12 latency mode and availability | The native leaf copies both from `rsf_native_latency_status_get` and `control.rs` uses that one latency snapshot for status. |
| Completion fence before binding | `host.rs` creates a candidate fence before committing COM ownership. `d3d12/tests.rs` requires repeated binding attempts while creation fails. |
| Present recording receipt | `d3d12.rs` publishes after successful recording. The same-frame capture refusal fixture expects Present to report its unknown screen and missed SR. |
| HUD-less slot admission | `d3d12.rs` matches the SR slot's session and source, waits on already-submitted work, and refuses the fixture's failed wait. |
| Typed settings handoff | The extension's typed settings enter `bridge.cpp` through the same host admission and viewport scaling callbacks as the Dictionary method. |
| Size-first ABI validation | Raw header reads in `lib.rs` reject the four-byte attach, boot and configure fixture before reading a version. |
| Generated sharpening layout | `rsf_game_sys::rsf_sharpening_options`, generated from `sharpening_settings.h`, is the type used by Godot, Unity and Runtime. |
| Runtime CPU service and explicit Present | `native_cpu_service.h`, the Runtime graphics hosts and the paired Godot driver receipt carry CPU facts, ping requests and the admitted window's actual submission and Present scope. |
| Auto policy delegation | The fake selectors return configured answers and record the arguments. The Runtime's own choice fixtures cover provider policy. |
| Refused window restore | `window.rs` retains ownership per HWND and its fixture admits a replacement while the old window continues forwarding independently. |
| Shared fixtures | `platform/fake.rs` provides versioned setup and status records and backend boot helpers. |

### E02 first-party native leaves

This slice starts at `84a8b9eb`. Eleven engine-facing functions previously defined
in `runtime.cpp` are implemented in `runtime.rs` and listed by the same names in
`exports.def`. The signatures in `api.h` are unchanged. D3D12 rendering checks the
pass size before borrowing the record and shares the existing Vulkan rejection of any
nonzero depth-domain descriptor. The engine module's D3D12 and Vulkan call sites,
packet layouts and receipt production stay as integrated.

- `host.rs` holds the D3D12 device, queue and fence behind one published binding.
  It uses the same factory/adapter LUID query, render-thread fence wait with a 15-second
  timeout, queue signal, SR setup and plan, FG setup, and Runtime status queries.
  The existing `d3d12.rs` ledger still authorizes the slot, records successful SR,
  matches real command-list submission, and carries the successful receipt to Present.
  Win32 reads and window-procedure calls report the same facts to `window.rs`.
- D3D12 capture uses the existing `rsf_core_capture12_*` Runtime owner from
  `runtime/orchestrator/src/frame_capture12.rs`. The former `FrameCapture12` C++ class
  also called that owner. The Rust leaf keeps the same event names and source identity,
  reset flag, camera, dimensions, time in milliseconds, render-target state, slot and
  default capture arguments. It flushes the slot before the HUD-less copy and calls
  Runtime FG afterwards. Shutdown flushes all capture slots before destroying SR and
  cancelling the session dump. The device binding and capture owner retain their DLL
  lifetime. The CRT destructor calls Rust for unload cleanup and skips a lock held
  during process teardown. Rust allocations follow Rust's out-of-memory behavior.
- The Vulkan wrappers still use E03's guard and capture callbacks and the shared Runtime's
  DLSS-G suppression and synchronized WSI entry points. The Present capture flag stays
  thread-local. It tracks an admitted legacy window marker or E01's exact Present
  execution receipt. A frame-role execution boundary still passes the explicit-execution
  flag to E03. Native WSI capture surrounds the actual call and sees its return value.
- `lib.rs` now iterates the existing options schema, composes typed settings and applies
  them through the registered engine service. It also reads status/requested settings
  for the script status snapshot. `runtime_control.cpp` lends the Dictionary only for
  synchronous Variant reads. Rust handles the frame-dump request once the SDK Callable
  runs on the render thread.

Residual E02 file inventory and source constraints:

| Open PORT file | Residual source |
| --- | --- |
| `runtime.cpp` | Platform wiring, C/C++ layout assertions, the CRT unload callback and the CMake-conditional FSR4 probe/Vulkan capture symbol table. The remaining validation/default leaves call shared inline helpers in `dlss_preset.h`, `sharpening_settings.h` and `frame_generation.h`. These are native build or Runtime boundaries. |
| `extension.cpp` | `GDExtensionBinding::InitObject`, scene-level registration and `GDREGISTER_CLASS`. The pinned `include/godot_cpp/godot.hpp` declares the binding object and `core/class_db.hpp` expands the macro into typed C++ registration. |
| `effect.h`, `effect.cpp` | `GDCLASS`, construction and destruction, the generated `CompositorEffect::_render_callback(int32_t, RenderData*)` virtual, `Ref`-owned scene and shader objects, Godot matrix and Variant conversions, and the RenderingDevice command and RID lifetime. The 4.7 API describes RID-based compute/list and storage-buffer calls and an owned `PackedByteArray` return for `buffer_get_data`. Rust already owns readback admission, dispatch dimensions, shader text and pixel classification. |
| `runtime_control.h`, `runtime_control.cpp` | `GDCLASS` for the `RefCounted` subclass, ClassDB script method binding, typed Dictionary and Variant conversion, RID checks, RenderingServer queries, and `callable_mp_static` plus `call_on_render_thread`. The macro delegates to the pinned C++ Callable factory. The shader probe and Runtime retain their existing script-visible classes and methods. |

The class evidence is from the pinned godot-cpp checkout
`8070599885b6df675fcad033dd032ce44be2b850`, including
`include/godot_cpp/classes/wrapped.hpp`, `include/godot_cpp/core/class_db.hpp`,
`include/godot_cpp/variant/callable_method_pointer.hpp` and
`gdextension/extension_api-4-7.json`. Removing these C++ class paths would require
another implementation of Godot's typed wrappers, class/virtual binding and resource
lifetimes. This slice uses the pinned classes for those exact operations.

Validation covers source comparison, bindgen-cli 0.72.1, Rust 1.94.1 formatting,
locked offline Windows x64 Cargo metadata, and Git whitespace checks.
Compilation, linking, fixtures and live rendering remain unverified.

### E02 extension wiring and readback continuation

Relative to `3066e6ad`, `host.rs` initializes the production platform with named Rust
fields. The 58 callbacks match the old positional table in `runtime.cpp`. SR and FG still
ask the Runtime for the default and selectable providers. `fg_choice.rs` owns FG preference,
known provider IDs and Auto fallback. The host calls `rsf_sharpening_core_defaults` and
`rsf_sharpening_core_valid` from the existing Runtime and checks size before validation.
The DLSS check uses the generated `RSF_DLSS_PRESET_MASK` from `dlss_preset.h` with the same
upper bound of 13.

`runtime.cpp` supplies the optional Vulkan link symbols and direct compile facts from
`runtime/orchestrator/CMakeLists.txt`. Its FSR4 FG probe and capture entries still exist
only under their corresponding CMake definitions. Vulkan Rust derives compiled FSR4 FG
availability from the presence of its optional probe. The CRT destructor continues to
invoke `rsf_godot_host_unload` and the two-sided layout checks cover the private symbol
record. No engine-facing signature, export name, or Present path changed.

`effect.rs` now runs the synchronous shader, input admission and dispatch sequence through
three typed leaves. It requires all four valid Godot RIDs before dispatch. `effect.cpp`
holds the Godot shader and uniform references through the sequence and frees valid RIDs in
uniform-set, buffer, sampler, pipeline, shader order as before. It also frees RIDs created
before an exception in a leaf. Refusal still reaches Rust's existing empty-readback
classification. Godot owns the PackedByteArray until the effect callback returns, and the
SDK calls keep their shader bindings, work-group counts, list order and readback point.

`extension.rs` now owns the ordered Runtime status keys and their conditional inclusion.
The 42 session status fields, five requested settings and Boolean bridge readiness retain
their previous keys and types. At pinned godot-cpp revision
`8070599885b6df675fcad033dd032ce44be2b850`, `variant.hpp` delegates 32-bit and unsigned
64-bit integer construction to `Variant(int64_t)`. The C++ leaf uses the same conversion
and constructs a Boolean Variant for bridge readiness. `runtime_control.cpp` still handles
the four renderer and driver entries through typed Godot calls, the option Variant reads,
RID validation, Callable scheduling and ClassDB method binding.

The six E02 residual files still have these boundaries. `runtime.cpp` retains optional
CMake/SDK linkage, record assertions and DLL unload. `extension.cpp` retains
`GDExtensionBinding::InitObject` and scene class registration. `effect.h` and `effect.cpp`
retain the typed `CompositorEffect` class, scene reads, resource lifetimes and probe
Dictionary conversion. `runtime_control.h` and `runtime_control.cpp` retain the typed
`RefCounted` class and Dictionary, Variant, RID and RenderingServer calls. The pinned
`extension_api-4-7.json` and generated `compositor_effect.hpp`, `rendering_device.hpp`
and `rendering_server.hpp` define the virtual callback and typed SDK boundary.

Source comparison matched the callback and status sequences, class headers and public
exports. Pinned Rust 1.94.1 formatting, targeted bindgen 0.72.1 and locked offline Windows
x64 Cargo metadata passed. MSVC `/Zs` accepted the four extension translation units with
the optional provider definitions and `runtime.cpp` with those definitions absent. The
syntax invocation needed `/Zc:__cplusplus` and system SDK include handling to match godot-cpp's
requirements. Builds, linkage, fixture execution and device verification remain open.

### E02 capture, properties and optional provider wiring

Relative to `818a5ede`, `effect.rs` owns the capture sequence from shader admission
through recording, CPU readback and releasing the five capture resources. The dispatch
requires the same four valid input RIDs. The release order remains uniform set, storage
buffer, sampler, compute pipeline, shader. If a release reports an exception, the sequence
tries the remaining resources and refuses the capture. As before, Godot borrows the velocity
texture for the callback and owns the CPU PackedByteArray until Rust finishes classifying it.
`effect.cpp` retains direct SDK shader compilation, typed resource and uniform creation,
compute-list calls, buffer readback, and per-RID validity and free calls. Shader bytes,
descriptor indices and dispatch dimensions are unchanged.

The Rust effect and extension owners now emit all 32 probe properties and 52 possible
Runtime properties in their prior order. The property sink receives copied scalar values
and synchronous C-string or matrix borrows. `script_dictionary.h` converts them through
the pinned `Dictionary` and `Variant` wrappers, including the existing rowwise Transform3D
basis and origin and columnwise Projection conversion. The pinned generated
`rendering_server.hpp` returns `String` for both renderer and driver methods. The effect
now emits empty names if the RenderingServer singleton is unavailable, matching the
Runtime status path. The build version is still the CMake-generated version supplied
by the class. The owner continues to report Vulkan admission through the production
platform's capability, alongside the same Forward+ and D3D12 checks.

`vulkan_features.h` is now the binding source for the optional provider record used
by `runtime.cpp` and the Rust Vulkan owner. It replaces both handwritten Rust structs.
The three build facts, optional FSR4 probe and four optional capture functions retain
the same layout and CMake guards. Rust still derives FSR4 FG availability from the optional
probe and consumes exactly the same capture operations. `runtime.cpp` retains the actual
conditional symbol references, native ABI assertions and CRT destructor. `api.h`,
`exports.def`, `runtime.rs`, the class headers and `extension.cpp` match the base.

Residual `effect.cpp` and `effect.h` own the godot-cpp class construction and destruction,
the POST_TRANSPARENT virtual callback, the typed compositor flag setters, and the
RenderingDevice leaves. `runtime_control.cpp` and `runtime_control.h` retain the
RefCounted class, Variant option reads, RID validation and render-thread Callable.
`extension.cpp` retains scene registration through the pinned GDExtensionBinding and
ClassDB. No engine-module or engine fixture files are in this slice.

Source comparison checked the full property-key sequences and unchanged public entry
files. Targeted bindgen 0.72.1, Rust 1.94.1 formatting, locked offline Windows x64 Cargo
metadata and Git whitespace checks passed. MSVC `/Zs` accepted all four extension
translation units with optional providers and `runtime.cpp` with those definitions absent.
Linkage, builds, tests and device behavior remain unverified.


3. The SCons engine module's C++ still holds the Godot camera matrix math and the previous
   camera transform and projection it and the camera jump distance come from, the native texture
   handles, packets and Vulkan image descriptions, motion resolve (`motion.cpp`), the Vulkan
   graph scope (`graph.cpp`), the D3D12 and Vulkan driver patches (`d3d12.cpp`, `vulkan.cpp`),
   and the direct Dictionary option parser, now a `configure` leaf. The SR and final-scene pass
   records (identity, keys, flags, screen class, driver route, camera scalars and the last
   recorded SR pass) are in `pass.rs`. Boot, configure and shutdown order, the ready, enabled,
   session and requested SR state, and the status-driven extent updates are in `host.rs`; CPU frame
   lifecycle and render frame identity are in `frame.rs`; primary viewport and target identity,
   render admission and the history reset rules are in `primary.rs` (see below). Native
   callbacks supply CPU facts to the Rust record builder before passing them to `window.rs`
   through `rsfg_cpu`. The window handle passed to `rsfg_vk_window` and each Vulkan surface's
   `rescaleframe_window` are engine module facts too.
4. The addon (`settings.gd`, `controller.gd`, `csharp/ReScaleFrame.cs`) keeps its own option
   defaults for the Inspector resource.
5. D3D12 residue in the `runtime.cpp` leaves: the fixed SR context inputs (setup ABI 1 planning,
   inverted depth, the custom DLSS engine identity and its flag) and the capture record mapping
   from the pass (view, reset flag, extents, frame time) could move into the owner's request
   records.

`extension.cpp` (the GDExtension entry) and the godot-cpp class bindings stay C++.

## Engine module viewport policy

At RSF `31a06a94`, `eng/build-godot-engine.ps1` passed `custom_modules` to Godot `4.7.2`
(`ed1daf0bf001b61586d9930840f2f1394092c079`). Godot's `modules/SCsub` collected the external
module's C++ objects into `module_rescaleframe`. At runtime the module loaded
`ReScaleFrame.Godot.Native.dll`, resolved existing `rsfg_*` functions and registered
`rsf_godot_engine_services`. The controller calls the GDExtension's `configure`, which uses
`native/extension.rs` to compose settings and hands them back through that service table.
`configure_from_extension` reconstitutes the Dictionary for the native bridge. The direct
`ReScaleFrame.configure` singleton call still uses Godot's Variant conversions, which differ
from the GDExtension's integer type and range checks. Sharing that parser would require a
separate compatibility review.

`modules/rescaleframe/SCsub` now invokes `rustc --emit=obj` from the repository root, where
rustup reads `rust-toolchain.toml`. The command uses the SCons Windows target architecture and
compiler ABI. Godot archives the resulting `no_std` object with the native module objects for
standalone editors and export templates, including .NET builds. The viewport policy uses
private C linkage functions in `viewport.h` and `viewport.rs`. The module continues
to load the native DLL through its existing ABI. This path has no Cargo or godot-cpp dependency.

Rust owns the requested engine scaling class and the initial scale. Off selects native-size
bilinear, Spatial selects Godot's FSR spatial path, and the remaining requests, including Auto,
select Godot's FSR2 temporal path. Godot's native temporal pass still runs before the RSF hook
at tonemapping. The initial per-axis scales retain the old ratios (`1`, `2/3`, `1/1.7`, `1/2`,
`1/3`, `0.77`) and their double-to-float conversion. `rsf-upscaler::Quality::scale` evaluates
Balanced in single precision and has no Ultra Quality entry. Its rounded extent helper also
differs from the native viewport's size calculation. The DLL keeps request validation and
provider selection.

The bridge still checks the root `RID`, queues the primary render target, and applies the typed
Godot mode and scale. On each temporal frame it uses a matching reported width ratio. At the
guarded viewport sizing hook it uses exact reported dimensions when both output axes match and
the render extent is nonzero.
Each status-driven update also requires the classified request to still be current. Off restores
the exact output size, and Spatial keeps Godot's dimensions. Allocation, jitter, graph execution
and recovery stay on their existing paths.

Source comparison covered the pinned Godot module/archive wiring and `RendererViewport`'s float
setter and sizing hook. Pinned Rust formatting, locked offline Cargo metadata and whitespace
checks passed. Open verification includes SCons object compilation and final linkage, symbol and
DLL export checks, and standard/.NET editor and template runs covering Off, Spatial, Auto,
temporal quality switches, resize and fallback. The documented build targets Windows x64 MSVC.
Other Windows toolchains and target architectures are unverified. This path requires a Rust
target that matches the Godot build, which is unavailable for ARM32 MinGW.

## Engine module CPU event records

Relative to RSF `1a87cbca`, the module now constructs its version 1 `rsf_game_cpu_event`
records in `cpu_facts.rs`. `bridge.cpp` samples the Windows performance counter and frequency
at the same reporting points, supplies the session and source identity, and passes the
stack-owned record to `rsfg_cpu` synchronously after the builder returns. All records are keyed.
Input reported with no open CPU frame uses source `0` and the unframed flag. Godot's keyboard,
mouse/touch and joypad mapping and emulated-input exclusion stay in the native input patch.
The frame-end expected-render count still comes from the draw's `wants_present` value. These
records carry a zero window message ID. The native DLL's window procedure supplies message facts
through its separate existing path, and Runtime decides pacing and marker semantics.

The new function is in the existing `no_std` SCons object, with an explicit source dependency
on `cpu_facts.rs`. It fills the C++ caller's record using fixed-width fields and imports no
Godot, DLL or allocator entry point. The private header checks the public record layout and
constants, and Rust checks the mirror's size, alignment and offsets. The Windows x64 MSVC target, module
archive and native DLL symbol resolution follow the existing viewport path.

Source review compared all 11 fields with the generated SDK binding and traced the patch sites
and module archive at Godot `ed1daf0bf001b61586d9930840f2f1394092c079`. The CPU lifecycle and
render fact source matched `1a87cbca`. Pinned Rust 1.94.1 formatting, locked offline Windows
Cargo metadata and whitespace checks passed. SCons compilation and final linkage, binary
exports, and editor and player callback behavior remain unverified.

## Engine module CPU frame lifecycle

Relative to RSF `fd2eb13e`, `frame.rs` in the same private SCons object owns the state that
`bridge.cpp` kept in globals: the open CPU frame, its identity, the simulation-ended and queued
flags, the CPU frame interval, and on the render thread the queued render frame identity,
gameplay flag, frame interval and the last SR-evaluated frame. `frame.h` declares the private
C linkage and checks the shared layouts and stage values. Nothing new is exported from the
Godot executable or the native DLL, and every `rsfg_*` call is unchanged.

Rust decides the order and calls three typed native callbacks: the engine clock
(`OS::get_ticks_usec`), the record report (QPC sample, `cpu_facts.rs` builder, `rsfg_cpu`) and
the render handoff (Godot pause and window focus, `call_on_render_thread`). The patch sites in
`eng/patch-godot.py` are unchanged. At Godot `ed1daf0bf001b61586d9930840f2f1394092c079` they are
`Main::iteration` after `iterating++` (begin, then simulation begin), before both
`RenderingServer::draw(wants_present, ...)` calls (queue), before `iterating--` (end), the
Windows run loop before `process_events` and the display server's key processing while not
iterating (begin), and `Input::_parse_input_event_impl` (input).

Preserved per callback, compared line by line with `fd2eb13e`:

- Begin, only when ready and enabled: no-op while a frame is open; otherwise the clock is read,
  the interval is the wrapping microsecond difference over 10^6 (1/60 s for the first frame),
  the identity increments with wraparound, the flags reset, then `FRAME_BEGIN` and
  `INPUT_SAMPLE` are reported.
- Simulation begin: `SIMULATION_BEGIN` only with a frame open.
- Queue, only when ready and enabled: `SIMULATION_END` (with no open-frame check, as before),
  then the ended and queued flags, then the handoff of the identity, or zero when the draw does
  not present, with the CPU interval. Godot's pause and focus are read after the report.
- End, with no ready check: `SIMULATION_END` unless the queue reported it since the last begin,
  then `FRAME_END` with render-expected `1` only for a presenting draw, then the frame closes.
  The frame stays open during both reports, so input delivered meanwhile stays framed.
- Input, after the native kind mask check: the open frame's identity, otherwise source `0` with
  the unframed flag.
- Lifecycle reports require a nonzero identity and are gated on ready and enabled at each
  report. The input report now re-reads both after the entry check on the same call, which
  cannot change the result.

On the render thread, the queued begin stores the identity, gameplay flag and interval before
`rsfg_render_begin`. The SR and final-scene hooks read one snapshot and refuse identity zero as
before. The final-scene hook still requires the last SR pass to carry the current identity. The
sequence break is `last_evaluated + 1 != current` with wraparound, in the same position in the
reset condition, so a consumed reset request still short-circuits it. It is computed when the
hook starts, but nothing on that thread writes either value before the old evaluation point.
A recorded SR pass stores its identity as evaluated, and a new primary target clears it, as
`last_frame` did. The screen class, the 0.25 s gap and the camera frame time use the snapshot.

The state uses relaxed atomics where the C++ used plain globals. Each value has the same
writer thread as before, and input parsed on another thread now reads the open frame without a
data race. Rust adds no ordering, pacing, marker or backend decision; Runtime still interprets
every report. This is a source port with no new experiment. Checks: rustfmt 1.11.0 (Rust 1.100,
the pinned 1.94.1 toolchain is not installed here) `--edition 2024 --check`, locked offline
Windows Cargo metadata and `git diff --check`. Not run: SCons compilation and linkage, the
`-Dwarnings` Rust object build, symbol checks, Clippy, Cargo tests, CTest, editor and exported
player runs, and callback order capture in a live session.

## Engine module primary viewport identity and render admission

Relative to RSF `f9a36d38`, `primary.rs` in the same private SCons object owns the identity and
per-frame decisions `bridge.cpp` kept in globals: the primary viewport and render target, the
resource generation, the history reset request, the last recorded SR pass's color texture,
internal extent and source frame, and the admission of the sizing, camera, SR and final-scene
hooks. `primary.h` declares the private C linkage and asserts the three record layouts, which
`primary.rs` asserts too. RIDs cross as `RID::get_id()`; `RID` equality and `is_valid` are
comparisons of that 64-bit `_id` (`core/templates/rid.h`), so storing it is exact. The C++ keeps
every typed RID, Godot read and call, `ready` and `enabled`, the thread handoff, the packet,
`driver_callback_add`, the previous camera transform and projection, and `last_pass`. The
`frame.rs` evaluation and retarget entries are now crate functions called from `primary.rs`;
nothing new is exported from the Godot executable or the native DLL, and no `rsfg_*` call,
status, backend choice, scaling mode or scale changed.

Callback triggers, in the order they run at Godot `ed1daf0bf001b61586d9930840f2f1394092c079`
with `eng/patch-godot.py` applied:

1. `ReScaleFrameBridge::configure` (the singleton call, or the GDExtension through
   `configure_from_extension`), after `rsfg_configure` accepts: `call_on_render_thread(set_primary)`
   runs `rsf_godot_primary_set` on the rendering server thread (identity stored, generation
   incremented, `frame.rs` evaluation cleared). The main thread then issues
   `viewport_set_scaling_3d_mode` and `viewport_set_scaling_3d_scale`, `FUNC2` commands queued
   behind it, or run directly after the queue flush when the caller is the server thread. A
   changed scale reaches `_configure_3d_render_buffers`, whose patched sizing hook calls
   `RSFGodot::render_size`, admitted by `rsf_godot_primary_sizing`. `enabled` is stored on the
   main thread after these calls, as before, so with a separate render thread the first sizing
   hook can still see the host inactive.
2. Each presenting or non-presenting `Main::iteration` draw: `queue_frame` queues
   `set_render_frame` before `RenderingServer::draw`. On the server thread it stores the render
   frame, calls `rsfg_render_begin`, reads `rsf_godot_primary_viewport` as a typed RID and, for a
   temporal request with a matching status, calls `viewport_set_scaling_3d_scale` directly. A
   changed clamped scale runs the sizing hook (step 1's admission) before the draw.
3. `RenderingServer::draw`, queued after step 2: `_draw_viewport` creates missing 3D render
   buffers through `_configure_3d_render_buffers` (sizing hook again), then each scene render
   reaches `RenderForwardClustered::_render_scene`. With valid scene buffer data,
   `RSFGodot::upscale` runs after Godot's temporal pass and `_debug_draw_cluster`, before the
   `Tonemap` timestamp, and `RSFGodot::hudless` right after
   `_render_buffers_post_process_and_tonemap`. Every scene render reaches both hooks, so
   admission refuses other viewports, probes and multiview by its facts.
4. `RendererViewport::viewport_attach_camera`, a server-thread command in queue order with steps
   1 to 3, when the camera differs: `RSFGodot::camera_changed` calls `rsf_godot_camera_changed`.
5. `ReScaleFrame.reset_history()` and the GDExtension's reset service on the caller's thread:
   `rsf_godot_reset_request`, a sequentially consistent store as before.

Preserved per hook, compared line by line with `f9a36d38`:

- Admission: host ready and enabled, nonzero source frame, valid render buffers, their render
  target equal to the primary target, one view, and a `d3d12` or `vulkan` rendering driver. The
  final-scene hook also requires the last recorded SR pass to carry the current source frame.
- SR hook after admission and valid color, depth, motion and output textures: a color texture
  other than the last recorded one increments the generation first, and the pass carries the
  generation after it. Reset is the consumed request, then a different color texture, a different
  internal extent, the `frame.rs` sequence break, a frame interval above 0.25 s, or a camera
  origin more than 20 units from the last recorded one. The request is consumed by every pass
  that reaches this point, including passes later refused for an invalid resolved motion or
  depth texture, which also keep their generation increment and record nothing.
- Recording, after the resolved textures are valid: internal extent, color texture, `frame.rs`
  evaluation and the recorded source frame, at the point the C++ stored them and `last_pass`.
- Sizing and camera hooks: ready and enabled and the primary target, as before. Initial state:
  generation 1, a pending reset, no primary, no recorded color, extent or source.

Intentional differences: the admission facts are read before the decision instead of in its
short-circuit order, so the driver name, buffer validity, render target and view count are read
for each scene render even while the host is inactive, and the camera distance is computed even
when an earlier reset term holds. All are pure queries. The final scene reads the slice of the
admitted buffers' render target, which admission proved equal to the primary target, and compares
the recorded source kept in Rust, assigned at the same point as `last_pass`. The pass generation
is assigned once. The rendering server thread is the only writer and reader of the primary state,
held in relaxed atomics where the C++ used plain globals.

This is a source port with no new experiment. Checks: rustfmt 1.11.0 `--edition 2024 --check`
of the object's crate root (the pinned 1.94.1 toolchain is not installed), locked offline Cargo
metadata and `git diff --check`. The SCons command still compiles `viewport.rs` from the
repository root, and `primary.rs` is listed as an explicit source dependency beside `frame.rs` and
`cpu_facts.rs`; `primary.h` is reached by the C++ scanner. Not run: SCons compilation and
linkage, the `-Dwarnings` Rust object build, symbol checks, Clippy, Cargo tests, CTest, editor and
exported player runs, and a live capture of the trigger order above.

## Engine module host lifecycle

Relative to RSF `97c2738f`, `host.rs` in the same private SCons object owns the engine module's
host lifecycle that `bridge.cpp` kept in globals and function bodies: the `ready`, `enabled`,
session and requested SR state, the boot admission and order, the `configure` sequence, the
shutdown order, and the status-driven extent updates at the queued render begin and at the
patched viewport sizing hook. `host.h` declares the private C linkage and asserts the record
layouts, the graph handoff capability value and the `rsf_godot_settings` prefix Rust reads
(`sr_backend` at 8, `quality` at 12). Nothing new is exported from the Godot executable or the
native DLL; every `rsfg_*` call, its arguments, `api.h`, the patch sites in
`eng/patch-godot.py` and the `ReScaleFrame` singleton's methods are unchanged.

The C++ keeps the typed leaves, and Rust decides when each runs:

- Godot: the project setting reads (`GLOBAL_DEF`, still in the same order and only after
  admission for the three path settings), the editor hint, the rendering driver names, the scene
  tree root check, the Dictionary parse with Godot's Variant conversions and the old defaults,
  `call_on_render_thread(set_primary)`, the scaling mode mapping (Native to bilinear, Spatial to
  FSR, others to FSR2) and both scaling setters, the render thread motion release and `sync`.
- Module loader: `LoadLibraryW`, its logged failure, the required export resolution with the
  same early refusal, the two optional capture exports, and `rsfg_attach_engine` with the
  engine services.
- Driver: `rsfg_vk_host_capabilities`, `rsfg_boot_vulkan` or `rsfg_boot` with the same setup,
  `rsfg_vk_window`, `rsfg_configure`, `rsfg_status` (session, or the five extent fields),
  `rsfg_render_begin` and `rsfg_shutdown`.

Preserved, compared line by line with `97c2738f`:

- Boot: nothing when `rescaleframe/enabled` is false, the display driver is the null driver or
  the editor runs; otherwise load and resolve, the graph handoff capability, the driver-selected
  start, `ready` from its result, the rejection log, attach (a refusal clears `ready`), then the
  session from status, zero when status refuses.
- Configure: refused while not ready, for an invalid viewport or one that is not the root
  viewport; then parse, the main window report on Vulkan, `rsfg_configure` (a refusal returns
  false with nothing else changed), the requested SR, the queued primary, the mode and initial
  scale from `viewport.rs`, and `enabled` last.
- Shutdown: `enabled` cleared, the motion release on the render thread and `sync` when the
  rendering server exists, then `rsfg_shutdown` only while ready. `ready` stays set.
- Render begin: the `frame.rs` store, `rsfg_render_begin`, then for a temporal request, a
  primary viewport, accepted status reporting the same request, a request still current and
  nonzero render and output widths, the scale `render_width / output_width` in double precision,
  narrowed by Godot's float setter.
- Sizing: the `primary.rs` sizing admission with ready and enabled; Native uses the output size,
  Spatial keeps Godot's size, others use the reported render size when status accepts, reports
  the same request, the request is still current, both output axes match and both render axes
  are nonzero.
- Every other `ready` or `ready && enabled` gate in `bridge.cpp` now reads
  `rsf_godot_host_ready` or `rsf_godot_host_active`, which short-circuits the same way.

Intentional differences: the editor hint and boot driver name are read before the decision
instead of after the setting check and the load, and the configure Vulkan fact (the OS driver
name) is read before the ready check. All are pure queries. The root check, parse and window
report now run through leaves in the old order. `ERR_PRINT` names `boot_load` or
`boot_rejected` instead of `boot` as its function. The runtime directory and project id live in
module strings between the load and start leaves, and are cleared after the start, as the old
stack locals were freed at the end of `boot`. `ready` and the session are relaxed atomics where
the C++ used plain globals written once by the main thread; `enabled` and the requested SR keep
sequentially consistent operations.

This is a source port with no new experiment. Checks: rustfmt 1.11.0 `--edition 2024 --check`
of the object's crate root (the pinned 1.94.1 toolchain is not installed), locked offline Cargo
metadata and `git diff --check`. `host.rs` is listed as an explicit SCons source dependency. Not
run: SCons compilation and linkage, the `-Dwarnings` Rust object build, symbol checks, Clippy,
Cargo tests, CTest, editor and exported player runs on D3D12 and Vulkan, and a live boot,
configure, resize and shutdown sequence.

Remaining in the engine module after this slice: the SR pass construction in `upscale` (camera
matrices, jitter, the previous camera and projection, the camera jump distance input, the
Vulkan image descriptions) and the final-scene pass in `hudless`, `motion.cpp`'s resolve pass,
`graph.cpp`'s graph scope, and the `d3d12.cpp` and `vulkan.cpp` driver patch leaves. The pass
records moved in the next slice. The direct singleton parser stays a C++ leaf until a
compatibility review decides whether it should share `extension.rs`'s checked parser.

## Engine module SR and final-scene pass records

Relative to RSF `798084c7`, `pass.rs` in the same private SCons object decides the SR and
final-scene pass records that `upscale` and `hudless` assembled field by field, and keeps the last
recorded SR pass that `last_pass` held. `pass.h` declares the private C linkage, the private
`rsf_godot_pass_record` and `rsf_godot_sr_facts` records, and asserts their layouts, every
`rsf_camera_frame` offset, and the role, flag, screen and camera ABI values. `pass.rs` mirrors
`rsf_camera_frame` (camera ABI 1, 384 bytes) and asserts the same offsets. The public pass has no
Rust mirror: the C++ copies the record's non-pointer fields into a zeroed `rsf_game_render_pass`
(`pass_from`) and sets the native resource pointers per route. The SDK's bindgen crate was not
reused because the SCons object is a standalone `no_std` rustc invocation with no crate
dependencies. `primary.rs`'s decision, recording and final-scene admission are now crate
functions called from `pass.rs`; their C linkage and `rsf_godot_sr_identity` are gone. Nothing
new is exported from the Godot executable or the native DLL; `rsf_game_render_pass`,
`rsf_godot_vk_packet`, every `rsfg_*` call, `motion.cpp`'s shader and the patch sites are
unchanged.

Rust decides: role (SR, then FINAL_SCENE for the repeat), session (`host.rs`), source and native
frame, the view, history and viewport key (the render target), `PRIMARY` plus `RESET` from the
`primary.rs` decision, the resource generation, the screen class (FLIGHT in gameplay, else
MENU), `motion_to_uv` `-1, -1`, `motion_camera_included` and `camera_valid` `1`, the camera
struct size and ABI, render extent (internal size), output extent (target size), near, far and
vertical FOV as Godot reports them, `depth_inverted` `1`, the frame time narrowed to float, and
the jitter in pixels as `jitter * render_extent * 0.5` in single precision in the old order. The
route comes from admission's driver fact: Vulkan uses the motion pass's `depth_r32` copy as SR
depth, samples the final-scene color and uses the Vulkan packet; D3D12 uses Godot's depth, a
color attachment read-write usage and the D3D12 packet.

The C++ keeps the Godot reads, the FOV conversion, the projection depth correction and the five
camera matrices plus `previous_clip_to_clip` (identity reprojection on reset, as before),
`resolve_motion`, the resolved texture checks, the previous projection and camera assignments,
native texture handles, callback resources, packets, the Vulkan image descriptions and
`driver_callback_add`.

Preserved per route, compared line by line with `798084c7`:

- SR: admission, `ensure_upscaled`, the four texture checks, the decision (generation increment
  and consumed reset request even for passes later refused), the matrices, resolve, the depth
  route and its check, then the previous camera and projection, the `primary.rs` recording
  (extent, color, `frame.rs` evaluation, recorded source) and the last pass, in that order. A
  refused pass records nothing. The packet's pass carries the same four native pointers.
- Final scene: admission with the recorded source equal to the current frame, the primary
  target's slice and its check, then the last SR pass with the final-scene role, no SR
  resources, the slice as color input, and the camera output extent in the Vulkan color image.
- `motion_jittered` stays zero and every other pass field stays zero, as before.

Intentional differences: the native texture handles for the SR pass are read after the resolve
instead of before it, and the unresolved depth and motion handles the old code stored and then
overwrote are no longer read. The final-scene plan runs before the slice read. The packet's
route and the final scene's usage use admission's driver fact, read from the same rendering
driver name, instead of reading the name again. All are pure queries. A `real_t` size assertion
makes the single-precision Godot build, which `eng/build-godot-engine.ps1` uses, a compile-time
requirement. The last pass is a Rust static behind an `UnsafeCell` that only the rendering
server thread touches, as the C++ global was.

Source evidence: field types checked against Godot `ed1daf0bf001b61586d9930840f2f1394092c079`
(`RenderSceneDataRD::z_near` and `z_far` are `float`, `taa_jitter` and `prev_taa_jitter` are
`Vector2`, `real_t` is `float` without `precision=double`) and against the SDK headers. A scratch
`rustc` probe confirmed that write-only fields of public `repr(C)` records reached through
exported functions raise no `dead_code` warning. Checks: rustfmt 1.11.0 `--edition 2024 --check`
of the object's crate root (the pinned 1.94.1 toolchain is not installed), locked offline Cargo
metadata and `git diff --check`. Not run: SCons compilation and linkage, the `-Dwarnings` Rust
object build, symbol checks, Clippy, Cargo tests, CTest, editor and exported player runs on D3D12
and Vulkan, and a capture comparing pass contents before and after.

Remaining in the engine module: the Godot camera matrix math and the previous camera transform
and projection (a Rust port would have to reproduce Godot's `Projection` and `Transform3D`
inverses bit for bit), `motion.cpp`'s resolve pass and shader, `graph.cpp`'s graph scope, the
`d3d12.cpp` and `vulkan.cpp` driver patch leaves, the native handle and packet assembly, and the
singleton's Dictionary parser.

## Build requirements

- The Rust archive is defined through `rsf_add_rust_static` in every configuration.
  `eng/build-godot.ps1` builds `--target rsf_godot`, which also runs
  `cargo build --locked -p rsf-godot`. Cargo is already required by the root build.
- The typed Vulkan creation fixture moved from CTest to the `rsf-godot` Cargo tests.
  Its Vulkan layouts are checked in and can be regenerated with
  `python tools/generate-godot-vulkan-bindings.py <godot-engine>/thirdparty/vulkan/include`
  using bindgen 0.72.1 and the pinned Godot checkout.
- `rsf_godot` now carries a second Rust archive beside the orchestrator's, the same unverified
  multi-archive link as Fallout 4 and Minecraft. It has not been linked.
- `tools-requirements.txt` (SCons, NumPy, Pillow) serves the engine build and the visual analyzer
  only; it needs no Rust entry.
- Legacy or ignored: the godot-cpp CRT cache cleanup in `CMakeLists.txt` remains a workaround;
  `rsfg_vk_capture_setup` and `rsfg_vk_capture_release` stay optional exports the engine module
  resolves without failing; `api.h` defines `RSF_GODOT_SETTINGS_ABI_VERSION` before its includes;
  `build-profile.json` and `dependencies.json` are unchanged and not touched by this slice.
