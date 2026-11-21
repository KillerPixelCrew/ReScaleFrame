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
the header-inline DLSS preset, sharpening and FG backend validators, the Vulkan owner in
`vulkan.cpp`, and the FG, presentation provider, latency, CPU counter and window facts.
`runtime_control.cpp` keeps the typed godot-cpp `Dictionary`, `RID` and `RenderingServer` calls and
the status Dictionary keys. The D3D12 SR context, its fences, slots and pending submissions, DXGI
vendor lookup and the Win32 input window stay in `runtime.cpp` and report each evaluated SR pass
to the owner. No second Runtime is loaded, and no vendor, pacing or presentation policy moved into
the integration; FG backend Auto resolution and SR Auto resolution still call the Runtime's
`rsf_fg_default_backend` and `rsf_gpu_default_sr_backend`.

Preserved behaviour, including quirks:

- The D3D12 status record carries ABI 1 and `rsfg_status` copies it to the caller, as before.
- A booted D3D12 session stays booted after `rsfg_shutdown`; a later `rsfg_boot` returns 1.
- D3D12 configuration is accepted and stored before boot, as before. Vulkan configuration,
  settings and status route to `vulkan.cpp` only after the shared refusal rules pass.
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

## Remaining Godot ownership slices

1. Vulkan session state in `vulkan.cpp`: its own requested settings, generation and status record
   (`requested`, `generation`, `current`), configure admission (XeSS device readiness, FSR4 graph
   handoff and interop, the FSR4 FG probe before retiring the WSI owner, XeSS FG, presenter
   capacity), boot (directories, Streamline and XeSS initialization, resolver choice), the FG
   enable rule (FG mode, gameplay and flight screen), fixed-guide retirement and SR fallback
   accounting. Folding its requested settings, generation and status into `control.rs` removes the
   last second settings owner; this is the next slice.
2. The D3D12 SR context owner in `rsfg_render`: SR context retirement and recreation rules, the
   FG provider switch request, spatial-to-native mapping and the fallback to backend 0, screen
   gating, sharpening defaults, the three-slot fence ledger in `rsfg_submitted`, the Present pass
   identity in `rsfg_present`, capture and diagnostics.
3. Input window ownership: the `Engine` window-class admission, the WNDPROC subclass and its
   restore, the input session and PCL ping routing.
4. `vulkan_creation.cpp`: the creation resolver, device feature and extension requirements, and
   XeSS/interop readiness.
5. `effect.cpp`: the compositor probe snapshot and the motion capture classification
   (unwritten, moving and non-finite pixels). The GPU readback stays a godot-cpp leaf.
6. The engine module `modules/rescaleframe` (bridge, graph, motion, D3D12 and Vulkan drivers) is
   compiled into the Godot engine by SCons against engine headers and cannot link this archive. It
   still holds CPU frame identity and markers, the primary viewport, render-scale policy (the
   per-quality scale table and scaling mode), history reset heuristics, camera matrices, motion
   resolve and a second Dictionary option parser in `ReScaleFrameBridge::configure`. Sharing the
   option schema there needs a new native export, which is an ABI change.
7. The addon (`settings.gd`, `controller.gd`, `csharp/ReScaleFrame.cs`) keeps its own option
   defaults for the Inspector resource.

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
