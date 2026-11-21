# Godot native integration ownership

## Rust control owner, first slice

`ReScaleFrame.Godot.Native.dll` now links the `rsf-godot` staticlib (`native/lib.rs`) next to the
same OrchestratorCore. It owns the Runtime control, settings and session state that
`native/runtime.cpp` and `native/runtime_control.cpp` held:

- `control.rs`: the requested settings and their generation, the `rsfg_configure` refusal rules
  (applied before either backend sees a request), the engine-module service routing
  (`rsfg_attach_engine`, `rsfg_configure_engine`, `rsfg_reset_history`, active only while a
  backend is booted), the D3D12 session (boot checks, session identity, runtime, Streamline,
  FidelityFX and XeSS paths, engine version, project id, log, boot state and the explicit FG
  presenter install values: Auto backend, unbounded generation, runtime switching, HUD-less
  backbuffer, inverted finite depth in metres, custom engine identity) and the status record with
  its assembly from Runtime facts for both backends.
- `extension.rs`: the `ReScaleFrameRuntime.configure(options, viewport)` option schema (21 keys,
  which ones must be Variant INT or BOOL within 32 bits, which convert as reals, the defaults) and
  the renderer admission that `configure` and `get_status` report (Forward+ on D3D12, or on Vulkan
  when the build carries the Vulkan SR payload).

The leaves stay C++ and decide nothing. `runtime.cpp` keeps every `rsfg_*` export in `api.h` with
its name and signature as a forwarder, and lends through `godot_platform.h` (mirrored by hand in
`platform.rs`, sizes and offsets asserted on both sides): the D3D12 frame capture owner, the
`rsf_fg12_install_explicit` call with its window admission and log trampolines, `rsf_fg12_options`,
the header-inline DLSS preset, sharpening and FG backend validators, the Vulkan leaves in
`vulkan.cpp` (second slice below), and the FG, presentation provider, latency, CPU counter and
window facts.
`runtime_control.cpp` keeps the typed godot-cpp `Dictionary`, `RID` and `RenderingServer` calls and
the status Dictionary keys. The D3D12 SR context, its fences, slots and pending submissions, DXGI
vendor lookup and the Win32 input window stay in `runtime.cpp` and report each evaluated SR pass
to the owner (the context and submission state moved to `d3d12.rs` in the third slice, the input
window to `window.rs` in the fifth). No second Runtime is loaded, and no vendor, pacing or presentation policy moved into
the integration; FG backend Auto resolution and SR Auto resolution still call the Runtime's
`rsf_fg_default_backend` and `rsf_gpu_default_sr_backend`.

Preserved behaviour, including quirks:

- The D3D12 status record carries ABI 1 and `rsfg_status` copies it to the caller, as before.
- A booted D3D12 session stays booted after `rsfg_shutdown`; a later `rsfg_boot` returns 1.
- D3D12 configuration is accepted and stored before boot, as before. Vulkan configuration,
  settings and status route to the Vulkan session only after the shared refusal rules pass.
- `rsfg_create_swapchain`, `rsfg_cpu`, `rsfg_render`, `rsfg_render_begin`, `rsfg_present` and
  `rsfg_shutdown` keep their order, refusals and return values.

Intentional differences: each boot attempt keeps its path strings for the process instead of
reassigning them, so a Runtime that retained a previous pointer never sees it freed. An exception
from the FG install leaf now records `RSF_BACKEND_ERROR_FEATURE_FAILED` before returning 0; the
return value is unchanged. The boot state is atomic.

This is a behaviour-preserving port from source review against the removed C++, with no new
experiment. Checks: rustfmt 1.11.0 (`--edition 2024`; the pinned 1.94.1 toolchain is not
installed), `cargo metadata --locked --offline` with the new lock entry, and `git diff --check`.
bindgen-cli 0.72.1 is not installed, so `platform.rs` is hand-mirrored and
`tools/generate-rust-bindings.py` has no Godot entry yet. Not run: native build and link, Cargo
tests (`native/control/tests.rs`, `native/extension/tests.rs`), Clippy, CTest, the Godot editor,
exported players, device fixtures and visual harness.

## Rust Vulkan session, second slice

`native/vulkan.rs` now owns the Vulkan session state that `vulkan.cpp` held, so no C++ settings or
status owner remains in the native DLL:

- the boot state and session identity (`rsfg_boot_vulkan` and the Vulkan branch of
  `rsfg_shutdown` forward to it), the requested settings and their generation, which keeps
  counting across sessions as before, and the status record with ABI 3 and its Streamline Vulkan
  latency overlay (`pcl_available` when the PCL message is a registered window message);
- the Vulkan configure admission, kept distinct from D3D12 and applied after the shared refusal
  rules in `control.rs`: XeSS SR device readiness, FSR4 SR build, graph handoff and interop, the
  FG provider set with Auto resolved to FSR3, FSR4 FG build, presenter, interop, primary window
  and a single generated frame before the FSR4 FG probe (still asked before the engine retires its
  working WSI owner), XeSS FG build, interop and window, FG mode needing a presenter and FSR3 a
  single frame, the live presenter's capacity for its own provider, and the latency options. A
  refusal at any step keeps the previous request; a refused latency option does too;
- the presenter currency rule in `rsfg_vk_acquire` (provider switch, or a new SR plan for an FSR4
  or XeSS FG owner whose guides are fixed), the presenter creation, Present and retirement
  accounting, the FG enable rule (FG mode, gameplay and flight screen), the fixed-guide retirement
  when a settled plan changes size, and the SR outcome accounting (evaluated, refused through the
  spatial fallback, or uncounted when the fallback could not record), with Auto SR resolved to FSR3.

`vulkan.cpp` keeps its guard and every Vulkan, Streamline, XeSS, FidelityFX, presenter, capture
and SR context call (the SR context state moved to `vulkan/sr.rs` in the fourth slice), and
decides nothing. Boot, shutdown and configure run inside the
`vulkan_locked` leaf, so the admission facts, the FSR4 FG probe, the presenter capacity, the
latency options and the commit see one guarded presenter state as before. The render, acquire,
create, Present and destroy paths read the session through `rsf_godot_vk_view_get` and report each
transition while they hold the guard. The lock order is the guard, then the Rust state; no leaf
runs while the Rust state is held. The `rsf_godot_platform` table is version 2 (184 bytes) and
drops the four Vulkan forwarders for ten Vulkan leaves. `api.h`, the engine module interface, every
`rsfg_*` export and the Runtime calls are unchanged.

Preserved behaviour: the refusal order, the lazily asked FSR4 FG probe and presenter capacity,
the settings and status read while not booted, a refused stop keeping the session booted, the SDK
shutdown deferred to presenter retirement, `RSF_BACKEND_ERROR_NOT_SUPPORTED` as the first FG
result, and the FINAL_SCENE return from the stored FG result. Intentional differences: configure
rechecks the boot state under the guard, and the "Godot Vulkan host ready" log is written by the
start leaf just before the session is marked booted rather than just after.

This is a behaviour-preserving port from source review, with no new experiment. Checks: rustfmt
`--edition 2024 --check` (see the commit for the toolchain), `cargo metadata --locked --offline`
and `git diff --check`; `platform.rs` stays hand-mirrored with size and offset assertions on both
sides. Not run: native build and link, Cargo tests (`native/vulkan/tests.rs` and the updated
`native/control/tests.rs`), Clippy, CTest, editor, exported players, device fixtures and visual
harness.

## Rust D3D12 SR context and submission owner, third slice

`native/d3d12.rs` now owns the D3D12 render path state that `runtime.cpp` held in globals:

- the engine's first device and queue identity (kept when the fence fails, as before, and any
  other pair refused);
- the SR context plan: the requested provider with Auto resolved by the Runtime's
  `rsf_gpu_default_sr_backend` through a leaf, spatial mapped to the normalising context because
  Godot evaluates FSR1 itself, the fallback to backend 0 when a provider is not created (the
  first creation result is the one reported), the `rsf_sr12_plan` extent and the context handle;
- retirement and recreation: a settings generation change, a new output extent, or, for the
  normalising context only, an input extent other than its plan drains all submitted work and
  destroys the context before a new one is made;
- the fence and slot ledger: three command slots taken in turn, each waiting for its own last
  submission, the pending command lists, one fence signal per matched `rsfg_submitted` with every
  matched slot taking that value, and the HUD-less record's submission reported to FG only after
  the signal; a refused signal keeps the submission pending for a later signal on the same queue;
- the final-scene HUD-less record on the last SR slot, the FG provider switch request (only with
  a provider owner on another backend, Auto resolved by `rsf_fg_default_backend`), screen gating
  (flight only in requested gameplay), sharpening defaults, the four-entry refusal diagnostics
  budget, the current render frame, the identity Present reuses with the borrowed texture handles
  cleared, the missed-SR report, and the D3D12 branch of `rsfg_shutdown` (drain, FG session end,
  capture flush, context destroy, frame dump cancel).

`runtime.cpp` keeps every `rsfg_*` export with its name, signature, checks and return values, and
lends eighteen typed leaves (`rsf_godot_platform` version 3, 328 bytes): the device, queue and
fence references with the 15 s wait and the Signal, the Runtime Auto provider calls behind the
DXGI vendor lookup, `rsf_d3d11_present_*`, `rsf_sr12_create`, `plan`, `set_sharpening`,
`evaluate` and `destroy`, the refusal log text, the capture events and records, and
`rsf_fg12_hudless`, `submitted`, `render_begin`, `window` and `end_session`. Leaves whose
work the export's `catch` used to abort return a completion flag, and the owner refuses the
call at the same point. The ledger lock is never held across a leaf except the fence signal,
which calls no owner. The obsolete `rsf_godot_paths_get`, `rsf_godot_settings_snapshot`,
`rsf_godot_sr_frame_report`, `rsf_godot_sr_missed` and `rsf_godot_session_id` entry points and
the `rsf_godot_sr_frame` record are gone; `api.h` and the engine module interface are unchanged.

Preserved behaviour: the capture, evaluate and Present ordering, the slot reused only after its
fence value, the stored creation result and plan surviving retirement, pending lists recorded
even for refused evaluations, and the HUD-less submission establishing both captures.
Intentional differences: the final-scene leaf checks the capture owner before using it (a booted
session always has one), and the ledger is lock-guarded. A pending recording now blocks slot reuse
and context retirement until the submission has a fence. The final scene can share its own
pending slot with SR, and refuses a slot held by another source frame. A later submission on the
same queue retries a refused signal, including when the earlier list is absent from the new batch.
If the engine never reports the pending list at all, retirement continues to refuse.

This is a behaviour-preserving port from source review, with no new experiment. Checks: rustfmt
1.11.0 `--edition 2024 --check` (the pinned 1.94.1 toolchain is not installed),
`cargo metadata --locked --offline` and `git diff --check`; `platform.rs` stays hand-mirrored
and the generated `rsf_game_render_pass` binding is used for the pass. Not run: native build and
link, Cargo tests (`native/d3d12/tests.rs`), Clippy, CTest, editor, exported players, device
fixtures and visual harness.

## Rust Vulkan SR context owner, fourth slice

`native/vulkan/sr.rs` now owns the SR context state and sequencing that `rsfg_render_vulkan` held
in `vulkan.cpp` globals:

- the `rsf_srvk` and FSR4 interop context handles, the device and `vkGetDeviceProcAddr` they were
  created on, the generation and provider they were made for and their readiness;
- retirement before a pass when the last creation failed, the settings generation changed, the
  engine device changed, or the last reported SR output differs from the pass output (presenter
  drain, `vkDeviceWaitIdle` on the creation device, FSR4 then `rsf_srvk` destroy; a refusal keeps
  what was not destroyed and takes the spatial fallback before the pass identity is counted);
- the passthrough of no SR and spatial (FSR1 is evaluated by Godot), creation with the requested
  quality, the FSR4 admission (`NOT_COMPILED` without the provider build, `NOT_READY` without a
  handoff, its `split_record` or a continuation output), the DLSS preset once after creation,
  the plan, the evaluate dispatch with the FSR4 continuation, the sharpening rule shared with
  D3D12 (`d3d12::sharpening`), the frame dump result, the spatial fallback (not counted when an
  FSR4 dispatch left no command buffer), the four-entry refusal log budget reset at boot, and the
  SR outcome reported to the session;
- the session stop sequence: presenter disable and drain, context retirement, then the
  `vulkan_stop` leaf (capture release, XeSS shutdown, creation requirements, SDK shutdown or its
  deferral), and the device borrow released with the session.

`vulkan.cpp` keeps the argument, depth-domain, frame record, frame dump and FG sections of
`rsfg_render_vulkan` unchanged and forwards its SR section with the guard held through
`rsf_godot_vk_sr_render`. It lends fourteen typed leaves (`rsf_godot_platform` version 4,
440 bytes, appended after the D3D12 leaves): the presenter drain, `vkDeviceWaitIdle`,
`rsf_srvk_destroy`, `rsf_fsr4vk_destroy`, `rsf_srvk_create` and the FSR4 interop create with the
handoff and its log, `rsf_srvk_dlss_preset`, the two plans, the two evaluates with the frame built
from the packet, `rsf_srvk_spatial_fallback`, the three log texts, and `rsf_input_dump_result`.
The pass reaches the owner as `rsf_godot_vk_sr_pass` with an opaque record of the packet,
handoff, continuation, frame record and current command buffer that only the leaves read; the
owner never keeps it past the call. `rsfg_vk_recording_begin` reads the live handles through
`rsf_godot_vk_sr_contexts` under the guard. The `rsf_godot_vk_sr_frame_report` export and its
record are gone. `api.h`, the engine module interface, every `rsfg_*` export, the statuses and
the leaf command order are unchanged.

Preserved behaviour, including quirks: the stale check compares the status output of the last
reported pass, not the context's own extent; a context returned with a failed result is kept and
retired by the next pass; FSR4 creation asks for `split_record` but evaluation does not; the FSR4
interop creation log is unbudgeted; the input refusal log uses the last plan and frame; `Auto`
still resolves to FSR3 on Vulkan. Intentional differences: an exception in the presenter drain,
device wait or destroy leaves during a render now counts as a refused retirement and takes the
spatial fallback, where the export's catch returned 0 without a report; the boot resets the log
budget just before the start leaf instead of inside it.

Lifetime review: no concrete local defect was found to fix. The creation leaf reports the handle
it received even when it ends in an exception, so the owner retires it as the globals did. The
FSR4 context keeps the handoff's `user` (the engine's rendering driver) and `split_record` from
creation; the engine compares that user against its active graph scope and a device change
retires the context. Unverified: retirement after a device change waits on and destroys through
the previous device; if the engine destroyed that device first, the wait is undefined. This is
unchanged from the C++ and was not checked against the pinned engine's device lifetime.

This is a behaviour-preserving port from source review, with no new experiment. Checks: rustfmt
1.11.0 `--edition 2024 --check` (the pinned 1.94.1 toolchain is not installed), `git diff
--check`, and `platform.rs`, `vulkan.cpp` and `runtime.cpp` size and offset assertions written for
the new records and table. Not run: native build and link, Cargo tests (`native/vulkan/sr/tests.rs`,
`native/vulkan/tests.rs`), Clippy, CTest, editor, exported players, device fixtures and visual
harness.

## Rust window and input owner, fifth slice

`native/window.rs` now owns the window and input state `runtime.cpp` held in globals, and
`vulkan.rs` the Vulkan primary window `vulkan.cpp` held:

- the input window admission: the first window of this process whose class is `Engine` (the class
  Godot's Windows display server registers), asked by the Runtime's `accept_window` trampoline
  (installed with the D3D12 FG presenter, which asks it for a swapchain's window) and by
  `rsfg_vk_window`, kept until `rsfg_shutdown`;
- the input session from the engine module's CPU events, stamped on window messages;
- the window procedure: attached at the first `FRAME_BEGIN` after admission and retried at the next
  one when `SetWindowLongPtrW` fails, reporting each received message as an `INPUT_EVENT` with
  `INPUT_KEYED | WINDOW_MESSAGE`, the QPC time and the message ID, then forwarding to the replaced
  procedure (`DefWindowProcW` before one is known). The procedure itself is the Rust
  `extern "system"` function, so the checked restore compares the window's procedure with it;
- the checked restoration in `rsfg_shutdown` (only while it is still the window's procedure and
  only cleared on success; the admission and session end either way), CPU event and PCL ping
  routing (the Vulkan latency
  owner while the Vulkan session is booted, otherwise the D3D12 FG owner and native latency ping),
  the window the status facts describe, and the Vulkan present-marker admission (booted Vulkan
  session, nonzero window equal to the admitted one);
- in `vulkan.rs`: the primary window `rsfg_vk_window` reports while booted (admitted or not),
  cleared just before each start leaf, read by the FSR4 and XeSS FG admission and, through
  `rsf_godot_vk_primary_window`, by the presenter host options in `rsfg_vk_create`.

`runtime.cpp` keeps every `rsfg_*` export with its name and signature as a forwarder and lends ten
typed leaves appended to `rsf_godot_platform` (version 5, 520 bytes, asserted on both sides):
`GetWindowThreadProcessId`/`GetClassNameW` into `rsf_godot_window_identity` (144 bytes),
`GetWindowLongPtrW`/`SetWindowLongPtrW` of `GWLP_WNDPROC`, `CallWindowProcW`/`DefWindowProcW`,
the QPC pair, `rsf_fg12_cpu`, `rsf_latencyvk_cpu`, `rsf_native_latency_ping`, `rsf_latencyvk_ping`
and `rsf_latencyvk_present`. `window_facts` now takes the admitted window. `rsfg_vk_native_present`
keeps its thread-local capture flag, set from an admitted marker as before. The Vulkan facts field
`primary_window` is now `reserved` and zero; `rsf::godot_vk::window` and `cpu` are gone. `api.h`,
the engine module interface, every `rsfg_*` export, the Runtime calls and their order are
unchanged; the window message events are built from the generated `rsf_game_cpu_event` binding.

Preserved behaviour, including quirks: a window the admission refused still becomes the Vulkan
primary window; a refused restore keeps the procedure attached and forwarding (a later subclass
forwards to it), keeps reporting messages with session zero, and nothing is attached again;
window messages route by the Vulkan boot state at delivery, not by a D3D12 boot; the D3D12
ping is asked even without a booted session. Intentional differences: a null window is refused
before the identity leaf (Win32 refused it as an invalid window before), and the identity leaf
always reads the class name, where the C++ skipped it for another process's window.

The original restore ignored a refused procedure replacement and cleared the forwarding pointer.
The merged port keeps it until replacement succeeds, and snapshots it before reporting a window
event. The regression for a refused native restore remains unrun. The attach and the procedure share
one thread: `rsfg_cpu` `FRAME_BEGIN` comes from `RSFGodot::cpu_begin`, which the engine patch
calls from the main loop and from the Windows display server's event processing
(`eng/patch-godot.py`), the thread that owns the window, and `SetWindowLongPtrW` dispatches no
message, so the procedure never runs between the swap and the stored previous procedure. The
module boots once per process (`bridge.cpp`), so the stale attached state after a refused restore
is not reachable again. Unverified: whether `RSFGodot::shutdown` runs before the display server
destroys the window (if after, `GetWindowLongPtrW` fails and the restore is skipped, which is
harmless for a destroyed window), and whether the shared overlay's own subclass sits above this
one in a Godot D3D12 session, which is the case the refused restore covers.

This is a behaviour-preserving port from source review, with no new experiment. Checks: rustfmt
1.11.0 `--edition 2024 --check` (the pinned 1.94.1 toolchain is not installed), `git diff
--check`, and size and offset assertions for the new record and table in `platform.rs` and
`runtime.cpp`. Not run: native build and link, Cargo tests (`native/window/tests.rs`, the updated
`native/vulkan/tests.rs`), Clippy, CTest, editor, exported players, device fixtures and visual
harness.

## Remaining Godot ownership slices

1. Vulkan Auto SR and FG resolution (Rule 3). `vulkan.rs` still resolves `Auto` to FSR3 for both,
   unchanged from the C++. The Runtime's `rsf_gpu_select_sr_backend(vendor, software, 1 << FSR3)`
   would return FSR3 only on a hardware adapter (NONE on a software one) and needs the Vulkan
   vendor id, which the session does not have when it resolves `Auto` at configure and status
   time; `rsf_fg_default_backend` picks by vendor and would change behaviour. Moving this needs a
   Runtime policy that takes the Vulkan host's servable providers and an adapter fact from the
   packet, which is a behaviour decision outside this behaviour-preserving slice.
2. `vulkan_creation.cpp`: the creation resolver, device feature and extension requirements, and
   XeSS/interop readiness.
3. `effect.cpp`: the compositor probe snapshot and the motion capture classification
   (unwritten, moving and non-finite pixels). The GPU readback stays a godot-cpp leaf.
4. The engine module `modules/rescaleframe` (bridge, graph, motion, D3D12 and Vulkan drivers) is
   compiled into the Godot engine by SCons against engine headers and cannot link this archive. It
   still holds CPU frame identity and markers, the primary viewport, render-scale policy (the
   per-quality scale table and scaling mode), history reset heuristics, camera matrices, motion
   resolve and a second Dictionary option parser in `ReScaleFrameBridge::configure`. Sharing the
   option schema there needs a new native export, which is an ABI change. Its input sources stay
   there and reach `window.rs` only through `rsfg_cpu`: the frame begin, input sample, simulation
   and frame end events (`cpu_begin`, `cpu_simulation_begin`, `queue_frame`, `cpu_end`), and the
   keyboard, mouse/touch and joypad kinds `Input::_parse_input_event_impl` reports through
   `RSFGodot::input_event` with `INPUT_UNFRAMED` outside a frame. The window handle passed to
   `rsfg_vk_window` and each Vulkan surface's `rescaleframe_window` for the present marker are
   engine module facts too.
5. The addon (`settings.gd`, `controller.gd`, `csharp/ReScaleFrame.cs`) keeps its own option
   defaults for the Inspector resource.
6. D3D12 residue in the `runtime.cpp` leaves: the fixed SR context inputs (setup ABI 1 planning,
   inverted depth, the custom DLSS engine identity and its flag) and the capture record mapping
   from the pass (view, reset flag, extents, frame time) could move into the owner's request
   records.

`extension.cpp` (the GDExtension entry) and the godot-cpp class bindings stay C++.

## Build requirements

- The Rust archive builds through `rsf_add_rust_static` only when `RSF_GODOT_CPP_DIR` configures
  the Godot target; `eng/build-godot.ps1` builds `--target rsf_godot`, which now also runs
  `cargo build --locked -p rsf-godot`. Cargo is already required by the root build.
- `rsf_godot` now carries a second Rust archive beside the orchestrator's, the same unverified
  multi-archive link as Fallout 4 and Minecraft. It has not been linked.
- `tools-requirements.txt` (SCons, NumPy, Pillow) serves the engine build and the visual analyzer
  only; it needs no Rust entry.
- Legacy or ignored: the godot-cpp CRT cache cleanup in `CMakeLists.txt` remains a workaround;
  `rsfg_vk_capture_setup` and `rsfg_vk_capture_release` stay optional exports the engine module
  resolves without failing; `api.h` defines `RSF_GODOT_SETTINGS_ABI_VERSION` before its includes;
  `build-profile.json` and `dependencies.json` are unchanged and not touched by this slice.
