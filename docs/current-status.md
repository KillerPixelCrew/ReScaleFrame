# Current implementation and validation

10 October update: the user reported a Unity UI/overlay regression in the new
package. The pre-UI snapshot, overlay synchronization, touch handling and Auto
FG corrections are applied and pass the recorded automated checks. Fresh game
and Claw acceptance remain pending; the earlier acceptance below does not cover
this package. [Correction and limits](research/shared-fg-20261004.md#unity-ui-regression-correction-10-october-2026).

Documentation checked on 7 October 2026 against source commit
`fda5fd0f2730b40e5b96f419bc0a05cb0b73932b`, the recorded research and GitHub release metadata.
This is a source/documentation audit. No new game, GPU or Windows build was run for this audit.

## Downloads and source builds

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
| Project Wingman, researched UE4.27.2 build | Exact executable detection and ABI lifecycle scaffold | Contract-tested in a test process. Preparation/start refuse. No renderer, SR, FG, latency or VR integration. |

All three native plugin APIs still report `rendering_ready = 0`. That conservative code-level
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
[Project Wingman research](research/project-wingman-renderer.md).

## Runtime and controls

SR consumes pre-tonemap scene colour. FG consumes the completed display-size frame and matching
normalized depth/motion, with optional HUD-less guides. AC7 preserves native UI composition;
its raw widget target is not proof of a vendor-compatible UI layer. Unity captures SDR colour
before screen-space UI; HDR output refuses that guide path. Full UI/moving-scene quality coverage
is not established for every provider.

One physical presentation provider owns the chain at a time. Live replacement drains the old
owner and preserves engine buffers; unsupported requests retain the working provider. Insert
offers independent SR and FG choices, SDK-limited multipliers, Reflex and FPS controls.
FG provider preferences are saved separately from deployment defaults. Other controls' persistence
depends on the host; do not assume every setting survives restart.

| FG provider | Capability in current source | Evidence limit |
| --- | --- | --- |
| DLSS-G | Fixed count and capability-gated Dynamic MFG in the runtime; SDK maximum queried; active generation requires effective Reflex at least On | Recorded RTX 4070 path generates one frame per source (2x). Higher counts and Dynamic MFG lack live acceptance. |
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
| FG provider | 1 | `runtime/contract/include/rescaleframe/frame_generation.h` |
| Overlay | 9 | `ui/overlay/include/rescaleframe/overlay.h` |

`VERSION` and the Cargo workspace both specify 0.1.0. Equal component versions do not imply equal
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
