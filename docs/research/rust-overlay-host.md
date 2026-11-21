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

## Integrated Runtime and Standard game run

The merge at `9e2cd87` passed 395 pinned Rust tests, formatting, Clippy
and 12 focused MSVC Release contracts. All eight Runtime and embedded
export inventories still matched by name and ordinal. The Anomaly DLL
kept its three exports and dynamic Runtime lookup. The overlay host
fixture linked. The worker branch and worktree were removed after archiving
its build, test and PE comparison evidence.

The isolated Standard foreground run used the merged Runtime, exact
Standard executable and matching manifest. The Rust host drew the
panel through a resize from 1280×720 to 1600×900. Turning SR off
saved `SR=0`, kept the native scene visible, stopped DLSS evaluations
and left DLSS-G active. Re-enabling SR saved `SR=1` and resumed
evaluations. This exercised the requested native rendering path.
The previously recorded refused-evaluation recovery is separate.

The panel then switched DLSS-G to FSR3 and back. Both transitions kept
the same engine facade, changed the physical swapchain identity and
reported a successful switch. FSR3 stabilized at about 240 vendor
presents per 120 source frames. The final complete status sample showed
DLSS-G active again and 3,436 DLSS SR evaluations with zero refusals.
The engine quit, destroyed Direct3D and returned code 0. The isolated
payload and settings were restored. The logs, artifacts and saved
intermediate settings are under
`.local/anomaly/staging/live-58fe95a-stage5/evidence/merged-overlay-host-standard-9e2cd87/`.

## Integrated AVX game run

The isolated AVX executable used the same Runtime, adapter and pack hashes
as the Standard run. The panel opened at 1280×720 and drew through the
resize to 1600×900. The overlay switched SR from DLSS to FSR3
Quality at 1066×600 and back to DLSS Quality at 1067×600. Both
selections were saved and applied. DLSS-G continued at 240 reported
vendor presents per 120 source frames. The panel drew all 2,046
visible frames and consumed all 22 reported pointer events. After
Insert closed it, a gameplay mouse drag visibly turned the camera.
The engine shut down Direct3D and returned code 0. The last SR status
sample reported 3,329 evaluations and zero refusals.

The isolated payload, settings and lifecycle script were restored.
Logs, hashes and the intermediate FSR3 INI are under
`.local/anomaly/staging/live-58fe95a-stage5/evidence/merged-overlay-host-avx-9e2cd87/`.
Held-key gameplay, D3D11-on-12 wrapping and fullscreen remain open.

## Matched SR evaluation captures

The AVX and Standard game logs from `9e2cd87` each named four Runtime
pipeline dump prefixes. The capture audit paired each prefix with its
core capture manifest, frame record and SR result. All eight matched by
session, source frame, view and effective backend. The manifests showed
successful SR and matched Present calls. The recorded resource generation
advanced on resize and on the AVX SR backend switch.

| Profile and source frame | Resource generation | Effective SR | Render to output |
| --- | --- | --- | --- |
| Standard and AVX 151 | 1 | DLSS | 853×480 to 1280×720 |
| Standard and AVX 392 and 632 | 2 | DLSS | 1067×600 to 1600×900 |
| Standard 872 | 2 | DLSS | 1067×600 to 1600×900 |
| AVX 872 | 3 | FSR3 | 1066×600 to 1600×900 |

Every pair had byte-identical raw SR input color, depth, game motion
and reconstructed output across the adapter and Runtime captures.
The packed row sizes and the core capture's reinsertion and final
color extents matched metadata. The Runtime recorded RG32F decoded
motion in all eight samples, RG16F motion submitted to DLSS and RG32F
submitted to FSR3. The checks cover the sampled indoor scene.

The verifier, transcript and 96 raw file hashes are under
`.local/rust-1.94.1-verification/merged-overlay-capture-audit-9e2cd87/`.
The source captures stay outside Git in the logged Anomaly dump directory.

## Matched camera cuts on AVX

Two isolated AVX runs at `bf12517` captured the same indoor area while
the camera responded to gameplay mouse input. The first requested 16
samples eight frames apart. Matrices changed between samples 688 and
696 and between 744 and 752. The capture burst missed the turn frames.
The second run recorded 16 consecutive frames, 1082 through 1097, with
mouse input timed from the first capture request.

The render path captured both cuts in the consecutive run. The camera
orientation changed by about 91.6° on frame 1083 and 170.1° on frame
1088. The Runtime's submitted DLSS constants set reset on both
evaluations and cleared it on the next frames. Each cut produced a
1600×900 final image of the new view with the game HUD intact. The
matched Present records reported success.

All 32 evaluations from the two runs shared exact frame, session, view
and provider identity with their matched Present records. Packed row
sizes and the raw input color, depth, game motion and reconstructed
output matched across the adapter and Runtime dumps. Both runs shut
down Direct3D and exited with code 0. The isolated settings and payload
were restored. Continuous camera pans and moving-object quality remain
open.

The verifier, transcript and 384 raw hashes are under
`.local/rust-1.94.1-verification/avx-camera-turn-audit-bf12517/`.
The raw captures and game logs are in `moving-sr-capture-avx-bf12517`
and `moving-sr-consecutive-avx-bf12517` under the isolated clone's
`evidence/` directory.

## Camera pans without history reset

The merged AVX run at `d0cc7ad` reduced mouse sensitivity from 0.12 to
0.012 in the isolated clone so mouse drags would produce smaller turns.
It captured consecutive evaluations and matched Presents from frame
1082 through 1097 after resize to 1600×900. Frame 1085 rotated 26.1°
and frame 1095 rotated 14.8° relative to their preceding frames. Both
retained DLSS history. The Runtime's clip-to-previous matrices matched
the recorded camera data.

All 640,200 submitted RG16F motion vectors were finite and nonzero
on each of the two moving frames. The adapter and Runtime dumps
matched byte for byte for raw input color, depth, game motion and
SR output. The final backbuffer images showed the updated room view
and intact HUD at both frames. DLSS-G stayed active and the final
complete SR report showed 2,489 evaluations with zero refusals.
The engine shut down Direct3D and exited with code 0. The runner
restored the isolated payload and mouse setting.

The verifier, transcript and 192 raw hashes are under
`.local/rust-1.94.1-verification/avx-continuous-pan-audit-d0cc7ad/`.
Logs and captured images are in `avx-continuous-pan-d0cc7ad` under
the clone's `evidence/` directory. This run covers the sampled room
and camera motion.

## Motion at an unchanged camera view

The foreground AVX window-view run captured 16 consecutive evaluations
and matched Presents. Frames 1086, 1087, 1088 and 1091 used the
same world-to-view matrix as their preceding frames and retained DLSS
history. The raw motion producer had 15, 15, 16 and 20 valid
nonzero vectors above 0.0001 UV in those frames. They fell within
x619–675 and y295–312 at 1067×600, over a window where vegetation
was visible. The same pixel positions had valid motion at the SR
handoff and finite nonzero RG16F motion in the actual DLSS submission.

The captured output and Present result matched each evaluation. The
game shut down Direct3D and exited with code 0. The runner restored the
isolated settings and payload. The verifier, transcript and 196 raw
hashes are under `.local/rust-1.94.1-verification/avx-stationary-view-audit-d0cc7ad/`.
The game log and captures are in `avx-stationary-view-motion-d0cc7ad`
under the isolated clone's `evidence/` directory. A matched draw
capture is still needed to identify the exact mesh and shader in
that region. Animated character and larger moving-object quality remain open.

## Vulkan Streamline-disabled verification

The merged core also built without the corrected Vulkan Streamline SDK.
All 13 focused Release contracts passed, including the overlay DLL
contracts and embedded render and FG12 clients. Runtime, AC7, Loader
and Minecraft kept their recorded export inventories. The direct DLL
probe still received `NOT_SUPPORTED` from Streamline initialization
and `NOT_COMPILED` from DLSS-G creation. The latter cleared both
outputs and made no host callbacks. Logs are under
`.local/rust-1.94.1-verification/integrated-streamline-disabled-9e2cd87/`.

## Integrated Debug verification

The enabled MSVC Debug build at `8268cb9` passed all 11 focused
contracts. The selection covers the Runtime overlay C++ and C11
clients, the no-SR client, Anomaly bootstrap, embedded render and
callback host clients, D3D11 host, Vulkan export and latency checks,
FidelityFX WSI, and the shared shim. Runtime, AC7, Loader and Minecraft
matched the recorded name and ordinal inventories. Anomaly matched its
three staged entry points and retained by-name Runtime discovery.
The GameSDK and runtime contract headers matched the baseline byte for
byte. Build and test logs are under
`.local/rust-1.94.1-verification/merged-core-debug-8268cb9-*`.

## Merged device fixture builds

The shared host and bridge now use D3D11-on-12 and native D3D12
diagnostics. The overlay ABI refusal asks for a matching panel and
RSF host. Registration and presentation follow the same typed
capabilities and caller-supplied setup.

MSVC Release linked OpenGL shared surface, D3D12 input readback,
native D3D12 overlay, D3D11 presentation, shared FG presentation,
shared surface and rendered FG device fixtures. The three overlay
DLL contracts and the embedded render frontend contract passed,
along with 395 pinned Rust tests, formatting and Clippy. The rebuilt
Runtime matched all 381 export names and ordinals. The device
executables were built and remain unrun. Logs are under
`.local/rust-1.94.1-verification/device-fixtures-merged-*`.
