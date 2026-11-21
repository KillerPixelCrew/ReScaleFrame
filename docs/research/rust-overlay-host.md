# Shared D3D11 overlay host in Rust

This source port starts at `05facc0`. The shared D3D11 overlay host in
`loader/proxy/src/overlay_host.cpp` was compiled into `rsf_orchestrator_core` and every consumer
that links its objects. Its decisions now live in the
[Rust owner](../../runtime/orchestrator/src/overlay_host.rs). The C++ file keeps the seven
`overlay_host.h` entry points and the native leaves declared in the private
[platform table](../../loader/proxy/src/overlay_host_platform.h). The Rust ABI file is generated
by `tools/generate-rust-bindings.py`.

## Callers

| Entry point | Callers at the baseline | Owner behavior kept |
| --- | --- | --- |
| `rsf_overlay_host_start` | AC7 `dlss_bridge.c` `overlay_tick` each Present; Runtime `overlay_platform.cpp` `start` for the overlay service; `tests/overlay_host.cpp` | The logger is replaced on every call. Started returns 1. A failed start stays failed until a stop. The swap chain selects the device. The order is panel, renderer, window and Insert subclass, with rollback in reverse. |
| `rsf_overlay_host_start_device` | D3D11-on-12 `overlay_d3d12.cpp` `start`, called by the Rust wrapped owner | Started returns 1 and does not replace the logger. Null arguments and an earlier failure are refused before the logger changes. The startup hint clock starts here. |
| `rsf_overlay_host_draw_target` | D3D11-on-12 `draw` | A busy host skips silently. The trace restarts when the panel opens. The hint and HUD window are unchanged. The intent is written even if the draw fails. A layout failure returns 0 without closing the panel. |
| `rsf_overlay_host_visible` | AC7 refresh sampling, D3D11-on-12 `visible`, fixture | Reads an atomic started mirror and the input module, and never waits for a drawing thread. |
| `rsf_overlay_host_toggle` | No caller in the tree | Same messages and behavior. |
| `rsf_overlay_host_present` | AC7 `overlay_tick`, Runtime service `present`, fixture | A second thread logs the skipped-thread line. A failed host refuses before taking the lock. Device, back buffer and view admission and their logs are unchanged. Targets are restored and released on every path. |
| `rsf_overlay_host_stop` | Runtime service `stop` (device reset), D3D11-on-12 `stop`, fixture | Clears the failure, closes and uninstalls input, destroys the renderer and panel, and releases the device. The panel DLL stays loaded. |

AC7 links `rsf_orchestrator_core` into `dinput8.dll`. The Runtime DLL, the Loader and its
`version.dll` alias, Minecraft, Unity Native and Godot Native link the same objects. All of them
share one host instance per module, as before.

## Split

Rust owns the host state, the panel instance and its five entry points, the renderer handle, the
selected device reference, start and stop, failure latching, the trace budget, the startup hint
clock and fade, the 0.25 s delta clamp, admission for the presenting device and back buffer,
texture batching (8 per drain), saving and restoring targets, and every log line. The leaves
handle `LoadLibraryW` and `GetProcAddress` with the `RSF_OVERLAY_DLL` lookup, swap chain
`GetDevice`, `GetBuffer` and `GetDesc`, `IUnknown` identity, the immediate context, texture
device and description, RTV creation, `OMGet`/`OMSetRenderTargets`, the `overlay_renderer` and
`overlay_input` calls, `GetTickCount64`, QPC and the thread ID. Leaves return owned COM
references. Rust wraps them in the existing `ComRef` helper, so restored targets, views, buffers,
contexts and devices are released on every path.

The Insert toggle key, the eight-second hint with a one-second fade and the four traced frames
are unchanged. No setting or overlay option was added.

## Behavior changes

- Fixed a reproduced bug. When the panel failed to lay out a frame at Present, the old host
  closed it but did not mark it failed. With the performance HUD on, or during the startup
  hint, every later Present laid out the poisoned panel again and logged the same line. The
  host now latches the failure like the other Present failures. The Rust test
  `a_layout_failure_is_reported_once_even_with_the_performance_hud` failed against the old
  behavior (four identical lines and four layouts) and passes after the fix.
- Start, draw and stop are now serialized by one host mutex. Present and draw still skip on
  contention instead of waiting. Stop now waits for an in-flight draw. Before, stop could
  destroy the renderer while another thread was drawing.
- Pointers in log lines keep MSVC's `%p` format of 16 uppercase hex digits. Other format strings
  are unchanged. Log lines are no longer cut at 511 bytes.

## Verification

All steps used the pinned Rust 1.94.1 toolchain and bindgen 0.72.1. The Windows x64 builds used
MSVC 19.52 through the Visual Studio 2026 generator, with the locally supplied Streamline, XeSS,
FidelityFX, Anti-Lag 2, Unity and godot-cpp inputs. The baseline was exported from `05facc0`.
It was built in its own tree with its own Cargo target directory.

- Rust passed 392 locked workspace tests, 249 of them in the orchestrator, plus formatting and
  Clippy with warnings denied. The 10 new owner tests use fake leaves and reference-counted
  fake COM objects. They cover start order and rollback, failure latching until stop, a panel
  DLL that stays loaded, the hint fade and hide, frame time defaults and clamping, admission of
  the device, back buffer and view, target save, bind and restore, release of every reference,
  texture batches, draw_target trace and intent rules, and the exported entry points sharing one
  host, including the busy skip line.
- Regenerating with bindgen 0.72.1 changed no other binding.
  `overlay_host.h` is unchanged.
- The Release PE comparison matched case-sensitive export names, ordinals, forwarders, imports and
  delay imports. Modules: Runtime (381 exports), AC7 `dinput8.dll` (256), Loader and its
  `version.dll` alias (273 each), thin carrier (6), Minecraft (267), Unity Native (275) and
  Godot Native (398).
- All 27 focused Release CPU and ABI contracts passed. They include the Runtime overlay DLL
  contracts (C++, C11, no-SR), the embedded render, FG12, SR12, translucency and callback host
  contracts, the shared shim exports and the presentation and FG API contracts.
- The native overlay host fixture now links the Rust owner. It, the D3D12 overlay fixture and
  the overlay DLL device fixture linked in Release. They were not run.

Device, foreground, visible-window and game tests were not run. These cover the D3D11 fixture,
the D3D12 backpressure fixture, the overlay DLL device fixture and the hotkey fixture. Real
panel drawing, input subclassing and D3D11-on-12 wrapping remain unverified on this build.
