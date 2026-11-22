# Godot native integration ownership

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
GDExtension entry, class bindings, Variant/RID marshaling and RenderingDevice recording still
use godot-cpp. COM, DXGI and C++ FrameCapture12 operations stay in typed native leaves.

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

- This slice returned ABI 1 for D3D12. The follow-up above corrected it to the current ABI.
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

## Rust Vulkan creation owner, sixth slice

`native/vulkan/creation.rs` now owns the state and policy `vulkan_creation.cpp` held in globals, as
a child of the existing Vulkan session (`Vulkan::creation`), not a second owner:

- the underlying resolver and session log the start leaf hands over, the owner instance, the XeSS
  preparation flag and the XeSS and Win32 interop readiness. The session's configure admission now
  reads the readiness from this owner directly, and `rsfg_vk_create` reads interop readiness
  through `rsf_godot_vk_interop_ready` for the presenter host options;
- the resolver routes: `vkCreateInstance`, `vkGetDeviceProcAddr` and `vkCreateDevice` (which
  records a non-null instance as the owner) are intercepted, the device-level `vkDestroyDevice`
  too, and every other name goes unchanged to the underlying resolver or, at device level, to the
  owner instance's `vkGetDeviceProcAddr`;
- instance creation: with XeSS prepared, the SDK's instance extensions merged once each and its
  minimum API version; an SDK refusal ends preparation, and `VK_ERROR_EXTENSION_NOT_PRESENT` or
  `VK_ERROR_INCOMPATIBLE_DRIVER` retries the engine's request and ends preparation;
- device creation in the same stage order and with the same retries: XeSS (fallback to the
  engine's request on an uncloneable chain, a `pEnabledFeatures` beside a `Features2`, a refused
  clone or SDK query, or a feature or extension refusal), then Win32 interop (both
  `VK_KHR_external_memory_win32` and `VK_KHR_external_semaphore_win32` listed and timeline
  semaphores enabled; retried unchanged on refusal), then the Runtime's optional core latency
  requirements (retried without them on refusal; the created device and the request that made it
  reported), then the native create;
- which feature structures the XeSS clone gets before the SDK patches it: a `Features2` (empty, or
  holding `pEnabledFeatures`), and the individual float16/int8, scalar block layout and integer
  dot product structures only where neither the structure nor its Vulkan 1.2 or 1.3 aggregate is
  present;
- every diagnostic, in the same words and order, and the destroy order (the Runtime stops pacing
  the device before `vkDestroyDevice`).

`vulkan_creation.cpp` keeps the Vulkan entry points the resolver hands out, which forward to the
owner with a private per-call record, and thirteen typed leaves in a separate table,
`rsf_godot_vk_creation_leaves` (version 1, 112 bytes, asserted on both sides): the chain facts
(the clone's admission without copying, structure presence, timeline semaphores), the XeSS clone
with its added structures and `Features2` first, request copies for the instance and for each
device stage, the XeSS instance and device requirement calls, the device extension enumeration,
the core latency requirement, created and destroyed calls, and the Vulkan create and destroy
calls. Extension lists are built by the owner and borrowed by the copies for the call. The
table is separate from `rsf_godot_platform` so the native creation contract test links it with the
Rust archive and without `runtime.cpp`; `rsf_godot_platform` is unchanged (version 5). The facts
fields `xess_device_ready` and `interop_device_ready` are now `reserved_creation` and zero. The
start leaf still picks the underlying resolver (Streamline's dispatch when it resolves
`vkCreateInstance`, otherwise the system loader's), and the stop leaf still ends preparation
between the XeSS and SDK shutdowns, both now through the owner. `api.h`, every `rsfg_*` export,
the Runtime calls, their arguments and their order are unchanged.

Preserved behaviour, including quirks: the owner instance survives a new start; the resolver keeps
forwarding after stop; a chain with an unknown structure keeps the engine's request and leaves
XeSS unavailable; the latency stage clones whatever request reaches it, so an SDK-appended
structure the clone does not know skips the latency requirements for that request.

Local fixes from source review: the C++ read the underlying resolver without a check in the
device-level resolver, the destroy path and the interop enumeration; these now refuse or skip when
none is set (not reachable with the pinned engine, which boots once per process). The underlying
resolver, owner instance and log were plain globals written by the start leaf and by the loader's
calls; they are now an atomic and a short lock never held across a leaf. A merge that cannot
allocate returns `VK_ERROR_OUT_OF_HOST_MEMORY` as the entry point's catch did, rather than
aborting.

Build: the `rsf_godot_rust` archive is now defined before the godot-cpp gate, as the other game
archives are, so `godot_vulkan_creation_contract` links it; it builds only for a target that links
it. That native test keeps its cases unchanged and now exercises the Rust owner through the C++
leaves, including the Runtime's AMD latency negotiation.

This is a behaviour-preserving port from source review, with no new experiment. Checks: rustfmt
1.11.0 `--edition 2024 --check` (the pinned 1.94.1 toolchain is not installed), `git diff
--check`, and size and offset assertions written for the new records and table in `platform.rs`
and `vulkan_creation.cpp`. Not run: native build and link, Cargo tests
(`native/vulkan/creation/tests.rs`, the updated `native/vulkan/tests.rs` and
`native/vulkan/sr/tests.rs`), Clippy, CTest (`godot_vulkan_creation_contract`), editor, exported
players, device fixtures and visual harness.

## Rust compositor effect owner, seventh slice

`native/effect.rs` now owns the state and policy `effect.cpp` held in each `ReScaleFrameEffect`, as
one owner per instance created and destroyed with the godot-cpp object (`rsf_godot_effect_create`
and `rsf_godot_effect_destroy`), not a second Runtime status:

- the pending capture request (`request_motion_capture` on any thread, taken first by the next
  callback), the callback count, and the snapshot and capture each callback publishes under one
  lock, read by `get_status`;
- the scene buffer admission: one view, a nonempty internal extent and valid layer 0 color, depth
  and velocity textures. Formats and native handle facts are read only for admitted buffers;
- the capture admission: a pending request and a velocity format of `DATA_FORMAT_R16G16_SFLOAT`
  (82, asserted against godot-cpp in `effect.cpp`). A request the callback cannot read is still
  answered with an unsucceeded capture, as before;
- the classification of the readback: it succeeds only at exactly `internal_width *
  internal_height * 8` bytes; each texel is two little-endian `f32` widened to `f64`, as
  `PackedByteArray.decode_float` does, and is non-finite when either component is, otherwise
  unwritten when both are at most -1, otherwise its larger magnitude feeds `capture_max_abs_uv` and
  it moves above 0.00001 compared in `f64`. Valid zero motion is neither moving nor unwritten. The
  -1 rule matches the pinned 4.7.2 Forward+ source, which clears the velocity-only framebuffer to
  `Color(-1, -1, 0, 0)` before its motion pass (`render_forward_clustered.cpp`);
- the published probe claims: stage `renderer_probe`, the reason text, the snapshot scope
  `last_post_transparent_callback`, and `rendering_ready`, `sr_active`, `fg_active` and
  `latency_active` false. The effect reads no Runtime settings or status, so no duplicate of the
  `control.rs` status exists; `ReScaleFrameRuntime.get_status` remains the source of SR, FG and
  latency state.

`effect.cpp` keeps the class registration (`extension.cpp`), its callback contract
(`POST_TRANSPARENT`, resolved color and depth, motion vectors) and the callback type and null data
check before forwarding, and lends three godot-cpp leaves in a separate table,
`rsf_godot_effect_leaves` (version 1, 32 bytes, asserted on both sides): the frame facts (scene
buffers, scene data and the RenderingServer's RenderingDevice in that order, the render target,
view count, internal and target extents, camera transform and jittered projection widened from
`real_t` to `double`, and the three texture validity facts), the formats and native handle facts,
and the unchanged readback compute pass, whose bytes stay in the callback record until it returns.
`get_status` rebuilds the same Dictionary keys, in the same order and with the same Variant types,
from `rsf_godot_effect_status` (416 bytes); the matrices round-trip exactly. `RSF_VERSION_STRING`
and the renderer and driver names stay C++ reads. `api.h`, the engine module, the addon and the
GDScript and C# callers are unchanged.

Preserved behaviour: the snapshot is replaced at every admitted callback, including the identity
matrices and format -1 when the scene objects or the buffers are missing; a capture reports the
callback it was taken in; the readback stalls the GPU and compiles its shader for each request.
Intentional differences: the three `texture_is_valid` queries run for every callback with scene
objects rather than short-circuiting after the view and extent checks (they are pure queries); a
leaf exception counts as missing scene objects, missing resources or an empty readback; and a
status record the owner refuses yields an empty Dictionary (not reachable: the owner exists for the
object's lifetime).

Source review found no demonstrated local defect to fix. Unverified and unchanged: the readback
buffer and the CPU stride use the internal extent while the shader strides by the velocity
texture's own extent. The pinned engine creates the velocity texture at the internal extent, but a
render target velocity override (`render_target_get_override_velocity`, used for XR) could differ;
with the single-view admission and no override set by the engine module (`bridge.cpp` reads
`get_velocity_buffer`), this was not reached. Whether Godot can destroy the effect while its
render-thread callback runs is also unchanged from the C++, whose members had the same lifetime.
`request_motion_capture` is a diagnostic action, not a setting, so Rule 2 adds no overlay control.

This is a behaviour-preserving port from source review, with no new experiment. Checks: rustfmt
1.11.0 `--edition 2024` (the pinned 1.94.1 toolchain is not installed), `git diff --check`, the
`DATA_FORMAT_R16G16_SFLOAT` value from the pinned `extension_api-4-7.json` and engine enum, and size
and offset assertions written for the new records and table in `platform.rs` and `effect.cpp`. Not
run: native build and link, Cargo tests (`native/effect/tests.rs`), Clippy, CTest, editor, exported
players, the renderer probe demo (`demo/main.gd`), device fixtures and visual harness.

## Vulkan Auto through the Runtime provider policy, eighth slice

Question: the Vulkan session resolved `Auto` to FSR3 for SR and FG in the integration
(`sr_backend` and `fg_backend` in `vulkan.rs`), a vendor policy outside the orchestrator (Rule 3).
The D3D12 path already asks the Runtime. This slice makes the Vulkan session ask the same Runtime
policy with the facts it actually has, and changes no explicit request.

Runtime policy:

- SR uses the existing `rsf_gpu_select_sr_backend(vendor, software, choices)`
  (`runtime/orchestrator/src/lib.rs`): no provider on a software adapter, otherwise NVIDIA DLSS,
  Intel XeSS, anything else FSR3, then FSR3, then XeSS among the choices. FSR4 is never chosen.
- FG takes the existing vendor preference `rsf_fg_default_backend(vendor, software)`
  (`frame_generation.h`) and passes it to `rsf_fg_select_auto_backend(preferred, choices)`, a new
  Runtime function in `fg_choice.rs` built on that module's existing `allowed` provider check. It
  returns the preference when served, otherwise FSR3 when served, otherwise none, the fallback both
  generic D3D12 hosts already apply (`render_host.rs` `poll` after `fg_default_backend(vendor, 0)`,
  and `d3d11_present.rs` Auto with the owning adapter's `default_backend` when the setup restricts
  its choices). It is declared only in the private `runtime/orchestrator/src/fg_auto_choice.h` and
  is not listed in `rust_exports.c`, so the Runtime DLL's exported names are unchanged; the Godot
  DLL links OrchestratorCore statically and calls it from `runtime.cpp`. No vendor ID is repeated
  outside `frame_generation.h`. Its cases are a Rust unit test in `fg_choice.rs`. The D3D12 hosts
  are not switched to it in this slice.

Facts the session passes, each read where it is proven:

- Adapter: the creation owner (`vulkan/creation.rs`) reads `vkGetPhysicalDeviceProperties` of the
  physical device after each successful `vkCreateDevice`, through the owner instance's resolver
  and a new creation leaf (`rsf_godot_vk_creation_leaves` version 2, 120 bytes, `adapter`):
  `vendorID`, `deviceID`, and software when `deviceType` is `VK_PHYSICAL_DEVICE_TYPE_CPU`. A failed
  read keeps no facts rather than a previous device's. The facts describe the live device and,
  like the owner instance, survive a new start. Without facts (no device yet) vendor and software
  are zero, the Runtime's unknown adapter, which yields FSR3 for both as before. The creation log
  records the facts.
- SDK availability: `rsf_slvk_initialize` with DLSS SR and FG succeeding in the start leaf, reported
  as `rsf_godot_vk_facts.streamline_ready` (the former `reserved` field, cleared at each start and
  after a completed stop). DLSS SR is offered only then; `native_sr_vulkan.h` requires that session.
- Servable providers, the admission's own facts so Auto never picks a provider `admit` refuses:
  SR FSR3 always, DLSS with Streamline, XeSS with the device created with the XeSS requirements,
  FSR4 with its build, the engine's graph handoff capability and the interop device; FG FSR3
  always, XeSS with its build, the interop device and the primary window.
- DLSS-G is not offered to FG Auto. Its presenter's retirement after native secondary window use
  still ends in an access violation (see the notes below), so it stays an explicit opt-in until
  that path is proven safe. An NVIDIA adapter's DLSS preference therefore falls back to FSR3 for
  FG, as before this slice. An explicit DLSS-G request keeps its admission and refusals unchanged.
- FSR4 FG is not offered: its admission asks a provider probe, and Auto must not trigger it. The
  per-pass FSR4 graph lease (`handoff`, `split_record`, `continuation`) stays the render-time
  admission of an explicit FSR4 request; the policy never prefers FSR4, so Auto never reaches it.

The resolution runs wherever Auto was read: the configure admission (with the facts it already
read), the view the render, presenter creation and capture leaves read, and the presenter currency
rule in `rsfg_vk_acquire`. The two exports without a table argument (`rsf_godot_vk_view_get`,
`rsf_godot_vk_presenter_current`) use the table the boot registered. The state lock is released
before the leaves run. The two Runtime calls are leaves appended to `rsf_godot_platform`
(`sr_select_backend`, `fg_select_backend`; version 6, 536 bytes, asserted on both sides). A
resolution that changes later (device creation, the primary window reported by `configure`) is
followed: an SR context made for another provider is stale and retired like a new request, and a
presenter owning another provider is no longer current, so the engine recreates its WSI owner.

Preserved: every explicit SR and FG request, its admission and refusal order, the FSR4 lease
admission and fallback, the spatial fallback on any provider failure, FG mode off still creating
the presenter for the resolved provider, the D3D12 path, and AMD, unknown and fact-less adapters
(FSR3 for both). `api.h`, the engine module, the addon and every `rsfg_*` export are unchanged.

Intentional Auto differences from the former FSR3 default, with their source:

| Adapter and facts | SR Auto | FG Auto | Source of the choice |
| --- | --- | --- | --- |
| NVIDIA, Streamline session started | DLSS (was FSR3) | FSR3 (unchanged): DLSS-G is not offered | `rsf_gpu_default_sr_backend`: `0x10de` prefers DLSS |
| Intel, XeSS device | XeSS (was FSR3) | FSR3, then XeSS once interop and the primary window exist (was FSR3) | `rsf_gpu_default_sr_backend` and `rsf_fg_default_backend`: `0x8086` prefers XeSS |
| CPU device type | none: spatial fallback (was an FSR3 attempt) | FSR3 (unchanged) | `rsf_gpu_default_sr_backend` returns none for a software adapter; the FG rule falls back to FSR3 |

Notes on these differences:

- Godot Vulkan DLSS SR on NVIDIA passed standard switching and resize with four raw frames
  (`docs/implementation.md`, "Godot developer integration in progress, 25 October 2026"). DLSS
  FG passes the strict eleven-phase lifecycle fixture but still ends with `0xc0000005` in driver
  queue destruction after native secondary window use
  ([DLSS lifecycle investigation](../../docs/research/godot-vulkan-dlss-fg.md), wrap-up section).
  Because the presenter for the resolved FG provider is created at swapchain creation, also with
  FG mode off, offering DLSS-G to Auto would have made that fault reachable from default settings.
  The first version of this slice did so; it was corrected before integration by leaving DLSS-G
  out of the FG Auto choices. Offer it again only after secondary-window retirement is proven safe.
- Intel FG: `bridge.cpp` reports the primary window in `configure` just before `native_configure`
  (`modules/rescaleframe/bridge.cpp` line 226), after the startup swapchain already created an
  FSR3 owner. The first configure then makes Auto resolve to XeSS, and the next acquire recreates
  the WSI owner as a fixed-guide XeSS owner. This slice reviewed no Intel FG run; the XeSS FG
  count evidence it found is from an NVIDIA device limited to one generated frame
  ([audit](../../docs/research/godot-xess-mfg-capability-audit-20261025.md)).
- Software adapter: `bridge.cpp` keeps Godot's temporal scaling mode and quality scale for Auto
  (lines 231 to 234), so the engine still renders at the reduced extent. Passing that through as an
  explicit Off does would leave the upscaled output unwritten. The pass instead records
  `rsf_srvk_spatial_fallback`, reports `RSF_BACKEND_ERROR_NOT_SUPPORTED` and counts a refused frame,
  as an FSR3 creation failure did before.

This is a source change from review, with no new experiment. Checks: rustfmt 1.11.0
`--edition 2024 --check` (the pinned 1.94.1 toolchain is not installed),
`cargo metadata --locked --offline`, `git diff --check`, and size and offset assertions for the
new leaves, record and table version in `platform.rs`, `runtime.cpp` and `vulkan_creation.cpp`.
The Runtime DLL export list (`rust_exports.c`), `gpu_policy.h` and `tests/gpu_policy.cpp` are
unchanged from before this slice. Not run: native build and link (including the static link of
`rsf_fg_select_auto_backend` into the Godot DLL and the export table of the Runtime DLL), Cargo
tests (`fg_choice.rs`, the updated `native/vulkan/tests.rs`, `native/vulkan/sr/tests.rs`,
`native/vulkan/creation/tests.rs`), Clippy, CTest (`godot_vulkan_creation_contract`), editor,
exported players, NVIDIA, Intel, AMD and software-device runs, and the visual harness.

## Remaining Godot ownership slices

1. Vulkan Auto SR and FG resolution moved to the Runtime policy in the eighth slice. Residual:
   DLSS-G stays out of Vulkan FG Auto until its secondary-window retirement is proven safe, and
   the Godot D3D12 leaves still pass software as false.
2. `effect.cpp` keeps no owner state after the seventh slice. Its class registration, callback
   contract, typed scene reads, readback compute pass and Dictionary keys are godot-cpp leaves.
   Residual: the readback's extent check against a velocity override (see that slice).
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

- The Rust archive is defined through `rsf_add_rust_static` in every configuration and built for a
  target that links it: `rsf_godot` when `RSF_GODOT_CPP_DIR` configures the Godot target, and the
  `godot_vulkan_creation_contract` test when the Vulkan headers are present.
  `eng/build-godot.ps1` builds `--target rsf_godot`, which also runs
  `cargo build --locked -p rsf-godot`. Cargo is already required by the root build.
- `rsf_godot` now carries a second Rust archive beside the orchestrator's, the same unverified
  multi-archive link as Fallout 4 and Minecraft. It has not been linked.
- `tools-requirements.txt` (SCons, NumPy, Pillow) serves the engine build and the visual analyzer
  only; it needs no Rust entry.
- Legacy or ignored: the godot-cpp CRT cache cleanup in `CMakeLists.txt` remains a workaround;
  `rsfg_vk_capture_setup` and `rsfg_vk_capture_release` stay optional exports the engine module
  resolves without failing; `api.h` defines `RSF_GODOT_SETTINGS_ABI_VERSION` before its includes;
  `build-profile.json` and `dependencies.json` are unchanged and not touched by this slice.
