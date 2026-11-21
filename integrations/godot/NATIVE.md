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
to the owner (the context and submission state moved to `d3d12.rs` in the third slice). No second Runtime is loaded, and no vendor, pacing or presentation policy moved into
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
and SR context call, and decides nothing. Boot, shutdown and configure run inside the
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
  the signal; a refused signal keeps everything pending;
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
session always has one), and the ledger is lock-guarded.

This is a behaviour-preserving port from source review, with no new experiment. Checks: rustfmt
1.11.0 `--edition 2024 --check` (the pinned 1.94.1 toolchain is not installed),
`cargo metadata --locked --offline` and `git diff --check`; `platform.rs` stays hand-mirrored
and the generated `rsf_game_render_pass` binding is used for the pass. Not run: native build and
link, Cargo tests (`native/d3d12/tests.rs`), Clippy, CTest, editor, exported players, device
fixtures and visual harness.

## Remaining Godot ownership slices

1. The Vulkan SR context owner in `rsfg_render_vulkan`: context retirement and recreation (SR
   generation, device and output size), FSR4 graph handoff and continuation, the spatial fallback
   call, DLSS preset application, sharpening defaults, frame dump capture and diagnostics. Its
   decisions read the Rust session view; the contexts and the presenter owner stay C++ leaves.
   The Vulkan Auto SR and FG resolution to FSR3, ported unchanged into `vulkan.rs`, is backend
   choice the Runtime should own (Rule 3), unlike D3D12, which asks `rsf_gpu_default_sr_backend`
   and `rsf_fg_default_backend`.
2. Window ownership: the `Engine` window-class admission, the WNDPROC subclass and its restore,
   the input session and PCL ping routing in `runtime.cpp`, and the Vulkan primary window and
   present-marker admission (`rsfg_vk_window`, `rsfg_vk_present_marker`).
3. `vulkan_creation.cpp`: the creation resolver, device feature and extension requirements, and
   XeSS/interop readiness.
4. `effect.cpp`: the compositor probe snapshot and the motion capture classification
   (unwritten, moving and non-finite pixels). The GPU readback stays a godot-cpp leaf.
5. The engine module `modules/rescaleframe` (bridge, graph, motion, D3D12 and Vulkan drivers) is
   compiled into the Godot engine by SCons against engine headers and cannot link this archive. It
   still holds CPU frame identity and markers, the primary viewport, render-scale policy (the
   per-quality scale table and scaling mode), history reset heuristics, camera matrices, motion
   resolve and a second Dictionary option parser in `ReScaleFrameBridge::configure`. Sharing the
   option schema there needs a new native export, which is an ABI change.
6. The addon (`settings.gd`, `controller.gd`, `csharp/ReScaleFrame.cs`) keeps its own option
   defaults for the Inspector resource.
7. D3D12 residue in the `runtime.cpp` leaves: the fixed SR context inputs (setup ABI 1 planning,
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
