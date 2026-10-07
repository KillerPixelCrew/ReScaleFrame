# Ace Combat 7

**DLSS/DLAA, FSR 2/3/4 and XeSS Super Resolution for AC7.**

The integration starts with the game and keeps the original lighting, post-processing and UI.
Menus, HUD, briefing terrain and aircraft icons render at full output resolution, independently
of the selected quality preset. The corrected TrueSky depth path has user acceptance for cloud
blocks and aircraft overlap; later low-resolution lighting/shadow defects remain under investigation.

[Download the AC7 v0.1.0 SR release](https://github.com/KillerPixelCrew/ReScaleFrame/releases/tag/v0.1.0)
· [Installation details](../../docs/releases/ac7-install.md)
· [Report an issue](https://github.com/KillerPixelCrew/ReScaleFrame/issues)

## Supported setup

- Ace Combat 7 on Steam, Windows x64. Tested executable: build **9855922**, app **502500**.
- Windows 10 or 11 and a GPU supporting the selected backend; DLSS requires NVIDIA RTX.
- A current GPU driver and the
  [Microsoft Visual C++ v14 Redistributable, x64](https://aka.ms/vc14/vc_redist.x64.exe).

The release includes all required SR runtime files and the overlay. The corrected build was
accepted in game on RTX 4070 Laptop. Other GPU families, game builds and Proton have not received
the same AC7 visual validation. This published ZIP is SR-only. The current source also implements
FG and latency services; those features require a separately built and configured deployment.

## Install

1. Close the game and download `ReScaleFrame-0.1.0-AC7-Windows-x64.zip`.
2. In Steam, choose **Manage > Browse local files** and find `Ace7Game.exe`.
3. Extract the archive into that folder. `dinput8.dll`, `ReScaleFrame.Game.AC7.dll`,
   `rescaleframe_overlay.dll` and `ReScaleFrame.ini` go beside the executable, along with the
   included `ReScaleFrame` folder. Replace every included file when updating.
4. Launch normally. DLSS and reinsertion start automatically when the renderer is ready.

Keep the original game executable. No Steam launch options are needed on Windows. If another mod
already uses `dinput8.dll`, do not overwrite it: this release does not chain another DirectInput proxy.

## Use

Press **Insert** to open or close the overlay. Enable upscaling, choose DLSS, FSR 2, FSR 3,
FSR 4 or XeSS, then select the quality preset. Unsupported selections keep the working backend.

| Preset | Behaviour |
| --- | --- |
| Native | Output-resolution antialiasing; DLAA with DLSS |
| Quality | Highest input resolution of the upscaling presets |
| Balanced | Lower input resolution than Quality |
| Performance | Lower input resolution than Balanced; the first-launch default |
| Ultra Performance | Lowest input resolution |

The selected SDK supplies the render sizes. UI and briefing stay at **100% output resolution**
across preset changes. Turning upscaling off restores native scene rendering. Enable and quality
choices are saved in `%LOCALAPPDATA%\ReScaleFrame\AC7.ini`; backend selection is per session.

FSR 4 INT8 compatibility requires Shader Model 6.6 and wave operations and was device-tested
on RTX 4070. Initial FSR 4 shader compilation can pause the game. See the installation guide
for hardware and first-use details.

The INI beside the game contains initial defaults and advanced options. Logs are written to
`%LOCALAPPDATA%\ReScaleFrame\AC7\rsf-dump.log`.

## Update, troubleshoot or remove

Close AC7 before updating the mod. If Windows reports a missing MSVCP140 or VCRUNTIME140 DLL,
install or repair the x64 Visual C++ runtime linked above. If Insert does nothing, check the DLL
locations and any other mod using `dinput8.dll`.

When reporting an issue, include your GPU, driver, preset, output resolution and a relevant log
excerpt. The [full installation guide](../../docs/releases/ac7-install.md) has the remaining details.

To uninstall, remove the three mod DLLs, `ReScaleFrame.ini` and the installed `ReScaleFrame` folder.
Restore any files you backed up. Delete `%LOCALAPPDATA%\ReScaleFrame\AC7.ini` if you also want
to reset your saved preferences.

## Current-source frame generation

Current `main` implements independent DLSS-G, FSR3, FSR4 and XeSS FG selection, live provider
replacement, SDK-limited multipliers, Reflex controls and rendered/presented FPS reporting.
AC7's earlier DLSS-G path is user-accepted. Additional FSR/XeSS FG paths have synthetic device
evidence, with AC7 moving-scene acceptance pending. Briefing generation is deferred. Higher MFG
counts and measured latency reductions are unproven; FSR4 SR compatibility does not imply FSR4 FG.

Use [the current-source loader guide](../../loader/README.md#ac7-frame-generation) for configuration
and runtime files. The v0.1.0 installation steps above do not install FG/Reflex. See
[current status](../../docs/current-status.md),
[DLSS-G/Reflex evidence](../../docs/research/ac7-dlss-fg-20261003.md) and
[shared FG evidence](../../docs/research/shared-fg-20261004.md) for validation limits.

## Integration notes

AC7 uses a modified UE4.18 renderer on D3D11. The proxy loads the game plugin, which owns guarded
engine hooks, per-view resources and reconstruction insertion before bloom, exposure and tonemapping.
The shared runtime owns vendor SDKs and the GPU-ordered D3D11/D3D12 SR bridge.

The view reader handles the game's 4096-byte uniform buffer. Scene motion needs UE encoding
removal, and the engine's camera transform supplies camera motion. The native integration sizes the
game's own UI targets through engine owners, preserving grading and glow; texture promotion
remains a compatibility mechanism. Enlarged separate translucency
uses matching view/depth selections and unjittered constants across VS, PS, HS, DS and GS.

- [Accepted result and debugging history](../../docs/research/ac7-consumer-session.md)
- [Executable and renderer evidence](engine.json)
- [Hook sites and expected bytes](../../docs/research/ue418-hook-map.md)
- [Capture research](../../docs/research/ac7-frame-capture.md)
- [Framework implementation tracker](../../docs/implementation.md)

## Source and ownership

| Files | Responsibility |
| --- | --- |
| [src/plugin.cpp](src/plugin.cpp), [ac7_native_renderer.h](include/rescaleframe/ac7_native_renderer.h) | Game SDK identity and serialized prepare/start/quiesce/stop/status calls |
| [src/native_renderer.cpp](src/native_renderer.cpp) | Expected-byte guarded engine hooks, primary view sizing, SR graph insertion, native UI/cloud producers and render-owner retirement |
| [src/render_scope.cpp](src/render_scope.cpp), [ac7_render_scope.h](include/rescaleframe/ac7_render_scope.h) | Copied render identity and leased resources bracketed by queued RHI begin/end markers |
| [src/view_uniforms.cpp](src/view_uniforms.cpp), [ac7_view.h](include/rescaleframe/ac7_view.h) | Captured view layout recognition, camera units, projection/history inverses and current-jitter removal |
| [src/ui_rules.cpp](src/ui_rules.cpp), [ac7_ui_rules.h](include/rescaleframe/ac7_ui_rules.h) | Allocation-free Slate/canvas/widget classification from caller-owned shadow registries |
| [src/scene_color.cpp](src/scene_color.cpp), [ac7_scene_color.h](include/rescaleframe/ac7_scene_color.h) | Captured recombine selection and exact candidate layer identities |
| [src/contact_shadow.cpp](src/contact_shadow.cpp), [src/contact_shadow.h](src/contact_shadow.h) | Exact-fingerprint DXBC noise-phase and depth-quantization correction at shader creation |
| [src/truesky_depth.cpp](src/truesky_depth.cpp), [src/truesky_depth.h](src/truesky_depth.h) | One-to-one normalized depth-bounds shader for guarded native TrueSky bindings |
| [src/truesky_motion.cpp](src/truesky_motion.cpp), [src/truesky_motion.h](src/truesky_motion.h) | Cloud distance in kilometres converted to device depth for camera reprojection |
| [src/motion_capture.cpp](src/motion_capture.cpp), [ac7_motion_capture.h](include/rescaleframe/ac7_motion_capture.h) | Bounded optional F9 observations and CPU/RHI/resource diagnostics |
| [CMakeLists.txt](CMakeLists.txt), [engine.json](engine.json) | Native targets and dated build/render evidence |

Prepare runs after executable decryption and validates researched function/helper bytes before
installing inactive hooks. Start opens production after host graphics activation. Quiesce closes
admission; stop can report busy until CPU tasks, queued RHI markers, pool/uniform references and
native callback bodies retire. A refused restoration also retains the module and host callbacks.
Callers must retry cleanup without releasing that ownership.

Render scopes copy session, source, submission, family and view identity before engine objects
retire. Native family counters do not substitute for input/simulation source IDs. Engine pooled
targets and generated uniforms release on the render owner; COM resources survive through RHI
end markers. Vendor processing and presentation remain in the shared runtime.

The view API uses row-major engine matrices, render-pixel jitter, radians for field of view and
engine world units for camera distances. Ordinary written UE motion requires the documented
encoding/axis conversion; raw zero identifies unwritten pixels that need depth-derived camera
motion. The exact encoding and coverage limits remain in `engine.json` and the motion research.

F9 capture records 60 Present intervals and samples GPU bindings at 0, 30 and 59. It writes
`engine.jsonl`, `native.jsonl`, `draws.jsonl`, deduplicated `cb_<id>.bin` blobs, shader bytecode and
`session.json` loss/timing metadata under the configured capture directory. Pointer/thunk command
matches and CPU TLS scopes are diagnostic associations, not proof of GPU completion or presentation.

ReScaleFrame is GPL-3.0-only except for its MIT Game SDK. NVIDIA, AMD and Intel runtime files
retain their separate included terms and notices.
