# Current implementation and validation

End-of-day checkpoint, 25 October 2026: work stopped at the user's request.
The latest native Release gate passes 75 discovered tests, with 64 executed and
11 opt-in skips, plus formatting and Clippy. Standard/.NET editors also build
with a new FSR1 storage-format correction, but its GPU retake and matching
template rebuilds have not run. The new aggregate Vulkan matrix runner preserves
actual images and failures; its initial FSR1 case exposed the format mismatch,
and a full matrix has not run. DLSS secondary-window teardown still crashes;
the primary-only latency-chain experiment failed and was reverted. FSR2 stage
captures narrow the remaining edge displacement without delivering a production
fix. XeSS MFG capability/admission follow-up remains unimplemented.
See the [progress tracker](implementation.md) for the restart checkpoint.

25 October 2026: the developer-facing Godot integration is in progress under
`integrations/godot`. Custom standard/.NET 4.7.2 Forward+ DX12 engines passed
SR/FG/MFG provider switching and PCL/input-marker fixtures. Actual FSR-generated
frame dumps exposed and helped correct a zero-motion handoff and camera matrix
conventions. Six standard/.NET visual cases now compare generated images with
independently rendered midpoints. SR-enabled testing also corrected Godot's
unwritten-motion sentinel. The expanded matrix passed vector/camera checks on
120 generated frames, but four of 30 end-to-end image cases still fail for
FSR2/FSR3 object motion. Standard/.NET Debug and Release
exports passed runtime/controller fixtures, with C# on .NET and a separate
GDScript-only .NET export. Native Vulkan FSR2/FSR3 SR now runs on standard/.NET
editors with resize, C# calls, raw GPU readbacks and validation-layer checks.
Vulkan FSR3 FG now passes standard/.NET functional fixtures with SR Off,
Off/resume, resize and a secondary native window. Captures include actual SDK
generated images and independent midpoint references. FSR2 image displacement
and a synchronization-validation present hazard remain open. The corrected
SR Off/FSR3 Vulkan image suite passes 12 cases and 48 actual generated frames.
Vulkan PCL/Reflex now runs with camera-free native presentation and alongside
FSR3 FG on standard/.NET editors. Vulkan DLSS SR also runs on both editions;
six DLSS SR + FSR3 FG + PCL image cases pass against independent midpoint
renders, covering 24 actual generated frames. XeSS Vulkan SR now passes six standard/.NET object/camera/combined image cases
with FSR3 FG and PCL, covering 24 actual generated images. Vulkan DLSS FG passes
strict standard/.NET device checks and independent midpoint image tests, including
a shared DLSS SR + FG case. Higher-count MFG3 passes strict GPU checks and independent quarter-step image
comparisons, preserving all three generated ordinals. XeSS FG uses a Vulkan/DX12 presenter: standard/.NET
lifecycle, C# and PCL checks pass. All twelve standard/.NET object/camera/combined
image cases with SR Off or XeSS SR pass, covering 48 actual SDK-generated images.
Optional foreground maintenance records scoped activation and preserved failed controls.
FSR4 SR uses a graph-owned
Vulkan/DX12 handoff; both editors pass switching/resize checks, with C# on .NET. One combined-motion
image case and four raw SR frames pass per edition. Rebuilt standard/.NET Release exports
also pass FSR4 switching, resize and raw-output checks. Debug exports pass XeSS FG/PCL,
Off/resume and resize on both editions. The strict DLSS lifecycle fixture now passes
all eleven phases and API/synchronization checks after the options correction, but
still crashes during SDK shutdown. FSR4 FG routing builds and correctly refuses
the current NVIDIA provider list; AMD execution remains untested. These remain bounded fixture results. Cold
startup pauses, FSR2 displacement, DLSS shutdown, new-provider export coverage and broader
scene-quality acceptance remain open. This does
not change game-plugin readiness flags.
Rebuilt Vulkan Debug/Release exports pass 12 standard/.NET SR, FG and
camera-free PCL fixtures, with C# in SR/FG and actual PNG/raw input checks in SR.
[Evidence and limits](research/godot-engine-integration.md).

10 October update: the user reported a Unity UI/overlay regression in the new
package. The pre-UI snapshot, overlay synchronization, touch handling and Auto
FG corrections are applied and pass the recorded automated checks. Fresh game
and Claw acceptance remain pending; the earlier acceptance below does not cover
this package. [Correction and limits](research/shared-fg-20261004.md#unity-ui-regression-correction-10-october-2026).

Later local live testing replaced the managed snapshot with a native copy on
Unity's current pre-UI recording list. Menu/gameplay and provider replacements
remained responsive, and the user confirmed the correction works. This local
acceptance supersedes the pending snapshot correction above. FSR4 ML-FG on
NVIDIA/Intel and physical Claw touch remain unestablished; the investigated INT8
guides provide FSR4 SR compatibility.

Documentation checked on 7 October 2026 against source commit
`fda5fd0f2730b40e5b96f419bc0a05cb0b73932b`, the recorded research and GitHub release metadata.
This is a source/documentation audit. No new game, GPU or Windows build was run for this audit.

## Downloads and source builds

18 October 2026: the [AC7 0.1.1 prerelease](https://github.com/KillerPixelCrew/ReScaleFrame/releases/tag/ac7-v0.1.1)
packages the current Windows source with FG/latency dependencies. It has build
and fixture checks, not fresh game acceptance. The Project Wingman observer
preview was withdrawn; Wingman remains research only, without a published
package. [Release evidence](releases/v0.1.1-windows.md).

The published [v0.1.0 AC7 release](https://github.com/KillerPixelCrew/ReScaleFrame/releases/tag/v0.1.0)
contains DLSS/DLAA, FSR 2/3/4 and XeSS SR, the AC7 plugin and Insert overlay. Its replacement
assets were uploaded on 2 October. FG and Reflex are absent from that package. Updating `main`
does not update a downloaded ZIP. Its included manifest and corresponding-source archive identify
the packaged code. There is no published Unity or Project Wingman release in the release listing
checked for this audit.

Current source implements independent SR/FG selection and live FG provider replacement for AC7
and the Unity Mono adapter. Build/deployment scripts and local test ZIPs are separate from a
published release. The Unity packager uses local Release artifacts and a deployed vendor payload;
it does not prove that a particular ZIP received game or Claw acceptance.

## Game integrations

| Integration | Implemented | Recorded validation and limits |
| --- | --- | --- |
| AC7, Steam build 9855922, D3D11 | Native renderer hooks and pre-tonemap SR; output-resolution UI/briefing; TrueSky correction; DLSS, FSR2/3/4, XeSS; D3D12 presentation facade and independent FG | User accepted corrected SR/UI/cloud rendering and DLSS-G. FSR3/XeSS FG have synthetic device activity; new AC7 vendor FG paths lack live acceptance. Briefing FG is deferred. Later low-resolution lighting defects and missing-object motion coverage remain open. |
| Drag'n Wash, Steam build 25286774, Unity 6000.3.14f1 Mono/URP RenderGraph, D3D12 | Shared shim, Mono/Harmony hooks, pre-DoF SR, overlay, DLSS/FSR/XeSS FG switching, pre-UI colour and temporal inputs independent of SR | Live SR execution and improved post-processing order recorded; user accepted XeSS/DLSS-G and the final FSR correction. General scene/resize/teardown coverage, other Unity players and MSI Claw acceptance remain unproven. |
| Project Wingman, researched UE4.27.2 build | Exact-build XInput carrier and guarded native temporal graph observer | Release-built, contract-tested and deployed; user-run desktop mission verifies readable inputs/history/rectangles. GPU leases, SR, FG, latency and VR processing remain unimplemented. VR hardware was unavailable. |
| Red Dead Redemption 2, Steam build 13773296, RAGE | Native common-SR adapter, scene/pacing/marker, shared FG presentation and Rust overlay; forced-DX12 ASI and Social Club correction | Four SR providers and recorded DLSS/XeSS FG intervals accepted. Shared overlay and all five runtime performance presets are user-confirmed, with actual input dimensions captured in logs. FG-Off selector mapping corrected, regression-tested and deployed. Normal SDR pixels match outside the captured HUD region. Complete HUD modes, HDR and non-NVIDIA hardware remain unproven; broad readiness stays zero. |
| GTA V Enhanced, Steam app 3240220 | Opt-in ASI entry and inactive game-plugin scaffold | Release-built and contract-tested in stand-in processes. The download is incomplete; no executable fingerprint, renderer hooks, SR/FG, latency, UI or in-game acceptance yet. |

All native plugin APIs still report `rendering_ready = 0`. That conservative code-level
flag is distinct from recorded user acceptance of individual deployed paths; it must not be
rewritten in prose as either universal support or absence of all working rendering. Detection
checks executable basename, x64 machine and exact SHA256. Other builds remain unknown.

Evidence: [AC7 consumer session](research/ac7-consumer-session.md),
[native renderer](research/ac7-native-renderer-refactor-20261001.md),
[SR/FSR4 acceptance](research/sr-interop-performance-20261002.md),
[AC7 FG and pacing](research/ac7-dlss-fg-20261003.md),
[shared FG corrections and final Unity acceptance](research/shared-fg-20261004.md),
[Unity SR](research/unity-dx12-runtime-20261004.md),
[lighting investigation](research/ac7-lighting-shadow-20261004.md) and
[Project Wingman research](research/project-wingman-renderer.md) and
[RDR2 research](research/rdr2-renderer.md).

## Runtime and controls

SR consumes pre-tonemap scene colour. FG consumes the completed display-size frame and matching
normalized depth/motion, with optional HUD-less guides. AC7 preserves native UI composition;
its raw widget target is not proof of a vendor-compatible UI layer. Unity captures SDR colour
before screen-space UI; HDR output refuses that guide path. Full UI/moving-scene quality coverage
is not established for every provider.

One physical presentation provider owns the chain at a time. Live replacement drains the old
owner and preserves engine buffers; unsupported requests retain the working provider. Insert
offers independent SR and FG choices, SDK-limited multipliers, Reflex and FPS controls.
AC7 and Unity also offer a persistent DLSS model request: Default, E, F, J, K, L or M.
Minecraft exposes the same selection in Sodium/Reese's options and its F8 screen. These controls
apply through existing runtime transitions. All seven requests passed a headless RTX 4070
device fixture; live game switching and visual acceptance remain pending. Driver overrides may
replace the requested model.
FG provider preferences are saved separately from deployment defaults. Other controls' persistence
depends on the host; do not assume every setting survives restart.

| FG provider | Capability in current source | Evidence limit |
| --- | --- | --- |
| DLSS-G | Fixed count, guarded RTX 40 MFG unlock and capability-gated Dynamic MFG; SDK/verified allocation limits; effective Reflex at least On | RTX 4070 Laptop synthetic device run confirmed 2x/3x/4x/6x, count changes and provider switching. Higher-count game image quality and Dynamic MFG remain unaccepted. [Evidence](research/rtx40-mfg-unlock.md). |
| FSR3 / FSR4 | Exact provider family requested; pinned swapchain API exposes one generated frame (2x) | FSR3 device activity and Unity user acceptance recorded. FSR4 FG refused the tested NVIDIA adapter; FSR4 SR INT8 success does not establish FSR4 FG support. |
| XeSS FG | XeLL pacing and SDK-queried interpolation maximum; count changes through the SDK | Device activity and Unity user acceptance recorded at 2x. Higher Intel MFG counts and Claw operation remain untested. |

FSR2 and FSR1 are SR choices, not FG providers. Unity Off/FSR1 can still produce normalized FG
inputs. Pacing follows the FG provider; NVIDIA SR does not impose Reflex sleep on FSR/XeSS.
Rendered FPS and SDK/DXGI aggregate presented FPS are separate counters. Neither establishes
physical scanout, a latency reduction, or an in-game performance gain.

## ABI, build and frontends

| Contract | Current version | Source |
| --- | --- | --- |
| Game plugin lifecycle and renderer callbacks | 13 | `sdk/game/include/rescaleframe/game_api.h`, `game_renderer.h` |
| Frame/camera record | 1 | `sdk/game/include/rescaleframe/game_frame.h` |
| Vendor-neutral backend | 3 | `runtime/contract/include/rescaleframe/backend.h` |
| FG provider | 2 | `runtime/contract/include/rescaleframe/frame_generation.h` |
| Overlay | 12 | `ui/overlay/include/rescaleframe/overlay.h` |

`VERSION` and the Cargo workspace both specify 0.1.1. Equal component versions do not imply equal
builds or ABI compatibility; deploy matched artifacts. SDK headers are optional, and absent headers
produce explicit unavailable stubs. Unity managed builds additionally require .NET, the researched
player's Managed assemblies, Harmony and Unity native headers. Those game assemblies are build
references and are not distributed.

Windows/MSVC is the reference build. `eng/verify.ps1` builds/tests native code and runs Rust format
and Clippy. Rust tests need `cargo test --workspace --locked`; managed/device fixtures are separate.
Recorded test counts describe their dated runs and include opt-in skips. They are not fresh audit
results or proof that clean CI exercises SDK-enabled GPU code.

The bootstrap DLL exports only a version; the launcher supports help/version without launching
or injection. Current game entry points are the AC7 DirectInput carrier and Unity version shim.
WSGM integration and frontend IPC remain planned. AC7 links shared runtime objects into its proxy;
Unity loads `ReScaleFrame.Runtime.dll`. See the [loader guide](../loader/README.md).

## Reading the history

[The implementation tracker](implementation.md) and research notes preserve dated failures,
experiments and later corrections. A pending statement in an earlier increment applies to that
increment. Follow its later correction and this status page for current claims. Plans describe
intended behavior; they are not feature or acceptance lists. Evidence JSON snapshots retain their
original measurements and hashes. Game `engine.json` summaries carry a separate current validation
summary so earlier observations remain attributable.
