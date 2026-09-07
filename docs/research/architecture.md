# Architecture

Design from the source inspection begun on 5 September 2026. [The source map](source-map.md) pins the inspected revisions. [The tracker](../implementation.md) records implementation; the current AC7 DLSS research proxy does not yet implement this full lifecycle.

## Ownership and loading

The launcher or WSGM arranges early loading. A small bootstrap loads the orchestrator, which recognizes and prepares the game plugin before activating graphics work. Heavy initialization and waits belong outside `DllMain`.

| Component | Owns |
| --- | --- |
| Launcher / WSGM | Session, profile identity, loading, requested settings |
| Bootstrap | Entry into the process and orchestrator loading |
| Orchestrator | Plugins, vendor SDKs, shared resources, commands/status, latency, presentation |
| Game plugin | Build detection, engine hooks/conventions, frame data, SR reinsertion |
| egui | Player settings, effective status, development inspection and capture |

Prefer a reusable launcher/session loader. An early hook-host route must prove ordering through Steam handoff; a successful late DLL load is insufficient. A DXGI shim remains a fallback for renderer experiments. The current `dinput8` proxy is a research carrier, not the completed product loader. Special K/SKIF provide frontend/payload references without requiring their full feature set.

Game plugins must not load a second runtime or own competing vendor contexts. AC7 signatures, structures, camera resets, and cloud/HUD rules stay in the plugin. Frontends exchange bounded configuration/status over IPC, not GPU textures.

## Game SDK and lifecycle

Use a C ABI with opaque handles, fixed-width fields, size/version checks, and function tables. Keep C++/Rust types, exceptions, and allocator ownership within their DLLs. Proposed package metadata includes ID, name, version, SDK ABI, architecture, relative entry DLL, and export.

Select one plugin per target process through read-only build detection. A discovery index may shortlist candidates; do not load every installed plugin into the game. Native plugins are not sandboxed by this ABI.

| Phase | Required behaviour |
| --- | --- |
| Detect | Validate fingerprint; report supported, unsupported, ambiguous, or incomplete |
| Prepare | Resolve hook sites and negotiate available inputs/capabilities |
| Start | Install hooks; roll back every acquired hook/resource on failure |
| Active | Deliver frame events and apply commands at defined boundaries |
| Quiesce | Stop new callbacks and retire in-flight CPU/GPU work |
| Stop | Restore owned changes and release resources safely |

Initially, disabling effects need not unload DLLs mid-game. Host services provide logging, settings, capabilities, resource allocation, and hook-lifetime helpers. Game callbacks cover input/frame start, simulation/render handoff, SR, post-processing/HUD capture, presentation, resize/reset, and shutdown.

SR must return a usable texture synchronously at the game's reconstruction boundary. Queueing it to a general worker and returning early cannot satisfy downstream rendering.

Each frame record needs:

- Session/render/view identity and resource generation, carried through game/render/RHI handoffs.
- API, adapter LUID, device/context or command-list identity, and execution phase.
- Allocation dimensions, input/output sizes, and valid rectangles.
- Colour, depth, motion, jitter, camera/exposure state, resets, and optional masks.
- Motion direction, units, camera/jitter inclusion, and dilation; depth and colour-space conventions.
- Readiness, ownership, valid-until boundary, and output insertion requirements.
- FG eligibility and the available HUD-less/UI-only inputs.

Negotiate backend requirements explicitly. Missing data disables the affected feature with a reason. A shared Unreal helper can emerge when a second game establishes reusable behaviour.

## SR and engine integration

The planned Claw route keeps native DX11 XeSS-SR on supported Intel hardware, then uses a same-adapter DX12 presentation bridge for XeSS MFG/XeLL. An optional DX12 SR round trip should earn its cost in measurement. Vulkan/DXVK remains a replaceable interop/presentation path; see [the comparison](presentation-backends.md).

Stock NVIDIA Streamline provides NVIDIA integrations. Community Shaders' inspected fork adds other vendor features, including Vulkan XeSS-SR; it does not provide the complete AC7 engine integration or native Vulkan XeSS MFG. Keep the public Game SDK independent of that choice.

The game plugin supplies engine data and returns reconstructed output to post-processing. AC7's UE4.18 graph predates later temporal-upsample machinery: output allocation, rectangles, shader constants, and downstream scaling must agree. Luma's inspected generic Unreal path is native-resolution AA, so its AC7 listing does not establish lower-resolution SR or MFG.

Use engine camera transforms with depth for camera displacement. Decode sparse velocity before submission, and verify written-vector semantics before combining motion. Analyse clouds, glass, rain, and other transparent layers separately. Preserve engine temporal setup where it supplies valid jitter/history; bypass only the filtering/scaling being replaced.

SR colour and FG HUD-less colour are different inputs. FG needs the completed output-resolution scene in the presentation colour space. Full-scale AC7 captures show a separate HUD texture, but the reduced-scale capture shows the composite shrinking too. HUD composition, glow, grading, and screen-space effects therefore remain integration work.

## DX11/DX12 presentation bridge

The design below is carried forward, with the intercept site, the facade contract, the surface set
and the per-Present protocol specified in [presentation-bridge.md](presentation-bridge.md) and the
[representation plan](../representation-plan.md). Reference implementations studied for it are
recorded in [vendor-fg-contracts.md](vendor-fg-contracts.md).

Expose a DX11-facing swap-chain facade to the game while a same-adapter DX12 queue owns the real presentation chain. Preserve the outward COM contract: `QueryInterface`, `GetDevice`, `GetBuffer`, reference counts, descriptions, resize, flags, fullscreen/output behaviour, and window association. Do not create a competing HWND swap chain.

Shared textures need compatible handles/formats and explicit GPU ordering:

1. Finish and submit the required DX11 writes, then signal the producer fence.
2. Make DX12 wait; copy/convert and transition resources before tagging them.
3. Set the matching present ID and call the XeSS proxy once per rendered frame.
4. Reuse shared slots only after SDK lifetime requirements and all relevant fences permit it.

A CPU signal does not establish GPU completion. A bounded resource ring needs fenced reuse; 4× MFG does not imply four application slots.

Start with `RV_ONLY_NOW` where SDK-recorded copies simplify ownership. The producer still must not overwrite its source before those copies finish. `RV_UNTIL_NEXT_PRESENT` has a longer lifetime. Neither Present returning nor a fence on one application queue proves completion of every SDK queue or physical scanout.

Handle resize, minimize/restore, focus, device removal, `DXGI_PRESENT_TEST`, loading, and cuts. Preserve tearing/VRR choices. SDK validation and the graphics debug layers belong in the synthetic harness; collect performance separately.

## MFG and XeLL

Query `maxSupportedInterpolations`, or initialize with `XEFG_SWAPCHAIN_USE_MAX_SUPPORTED_INTERPOLATED_FRAMES` and query the result. Reserve the intended maximum, then change the active count only when requested.

| Setting | Generated frames per rendered frame |
| --- | --- |
| Off | Disable interpolation through the SDK |
| 2× | 1 |
| 3× | 2 |
| 4× | 3 |

The inspected count setter accepts one through the configured maximum. Off is not a zero-count call; changing the allocation maximum requires reinitialization. Expose only supported choices and report effective state, including external overrides. `xefgSwapChainGetLastPresentStatus` helps distinguish requested from effective interpolation but is not a physical scanout measurement.

XeLL sleep belongs before input sampling, followed by simulation, render-submit, and present marker pairs with consistent IDs. AC7's outer `FEngineLoop::Tick` is a source lead; confirm the shipped input order. Carry IDs with the real handoff instead of sampling the latest global counter at Present. Ambiguous frames should skip interpolation.

The official XeLL path inspected is DX12. Whether it provides useful pacing for AC7's DX11 rendering through this bridge remains a Claw experiment. Measure end-to-end latency, not just successful initialization. Use one FG owner and one latency/limiter policy. Treat swap-chain-changing backend switches as restart-required initially.

## WSGM and standalone

Use the same runtime packages and settings schema in both modes. Standalone supplies its own profile store; WSGM supplies canonical application identity and profile persistence. Each session has one persistence authority. If its controller disconnects, keep the last applied snapshot and report the disconnect without blocking rendering.

WSGM snapshot inspected: `2bfe58f09a3457a8372a1d75a739a986bbbd26a6`. Revalidate these points against the delivery branch:

| File | Integration concern |
| --- | --- |
| `src/WSGM/Core/LaunchWrapperCommand.cs` | Steam `%command%` and non-Steam argument routes |
| `src/WSGM.Launch/Program.cs` | Environment, de-elevation, process tree, exit semantics |
| `native/SteamInput/crates/steam-input-lease/src/lib.rs` | Suspended child, containment, resume; separate from managed launch |
| `src/WSGM/Core/AppConfig.cs` | Canonical identity and `UsePerGameProfile` |
| `src/WSGM/Shell/RunningApplicationCoordinator.cs` | Running-process association; not early loading |
| `src/WSGM/Shell/AutoTdpService.cs` | RTSS mean frame time and rendered target |

Upscaling must work with WSGM Device Integration disabled. Coordinate and restore RTSS/game/XeLL pacing policy. AutoTDP must consume rendered timing, so a 30 FPS simulation presented at 120 FPS does not look like 120 rendered FPS.

The existing egui DLL should become both settings UI and development interface. Use a bounded command queue, apply changes on the owning thread, and show effective settings/refusal reasons. Restore graphics state, handle input/focus/DPI/controller navigation, and keep diagnostics out of scene captures and temporal history. The detailed views are in [the todo](../implementation.md#egui-development-and-validation-workflow).

## Reference reuse

The source comparison supports using small pieces and documented approaches rather than forking an entire modding framework.

| Reference | Use | Boundary |
| --- | --- | --- |
| OptiScaler | Vendor adapters, input/output separation, native DX11 XeSS, DX12 FG, capability handling | Existing upscaler inputs do not supply AC7's missing engine integration |
| Streamline / AMD FSR SDK | Official NVIDIA and AMD backend contracts | Neither replaces the AC7 plugin or the direct Intel backend |
| Skyrim Community Shaders | DX11/DX12 resources, swap-chain facade, engine-aware upscaling | Skyrim engine types and assumptions remain outside the orchestrator |
| Fallout 4 Community Shaders | Another concrete presentation bridge, resource lifetime, game anchors | Its README explicitly describes WIP paths and limited validation |
| Luma | TAA detection, constant buffers, UE velocity decoding | Generic path is native-resolution AA; license is custom, not plain MIT |
| UEVR / UESDK | Engine and render-target discovery, camera/view/Slate boundaries | UEVR's inspected license says all rights reserved; no general copying permission established |
| UE4SS / patternsleuth | Object/signature discovery and frame-loop anchors | Research tools/helpers, not required runtime frameworks |
| ac7-ultrawide / AC7 UEVR plugin | Known shader/camera/viewport behavior | Validate all signatures against the current game build |
| Special K / SKIF | Early-loader design and frontend/payload separation | Do not adopt the entire system-wide feature surface |
| ReShade / 3Dmigoto | DX11 interception, COM handling, shader inspection | Optional diagnostic tools; no dependency on their mod runtime |
| RTX Remix | Renderer interception and scene reconstruction ideas | Its D3D9 fixed-function renderer replacement is outside the AC7 DX11 plan |
| Intel SDK / Inspector | Supported APIs, data contracts, MFG controls, diagnostics | Actual DLLs and Claw validation still needed for execution |
| Intel UE plugin | Supported engine-integrated ordering and settings reference | Editor/project plugin is not directly loadable into a cooked UE4.18 game |

Licensing observations are from the inspected files, not a complete legal compatibility review. Luma adds a commercial-use permission condition. UEVR provides no general reuse grant in its current LICENSE, and no separate permissive grant was established for UESDK. OptiScaler and Special K have GPL terms; Community Shaders projects add their own exceptions. SKIF has MIT terms. ReShade uses a BSD-style license. UE4SS is MIT; patternsleuth declares MIT OR Apache-2.0. The egui components offer MIT/Apache licensing. Intel runtimes have separate binary redistribution terms and are not open-source implementations.

ReScaleFrame uses MIT for the Game SDK and GPL-3.0-only for its runtime and game plugins. Check the actual terms of any incorporated reference code or vendor dependency. A linking exception in one repository does not automatically cover another dependency. Preserve vendor DLLs unchanged and ship applicable notices.

Keep the first scope Windows x64, AC7, Intel SR/MFG/XeLL, and SDR. The orchestrator can define backend boundaries for DLSS and FSR now without requiring every vendor/API combination to ship with the first AC7 version. MFG remains part of the first complete AC7 target.

## Validation status

Source inspection supports the design. Later AC7 captures and DLSS runs establish parts of the input path, not XeSS/MFG, the DX11/DX12 bridge, or Claw performance. Build the synthetic bridge and test the target device early; complete AC7 reinsertion alongside explicit frame/view capture and egui diagnostics. [The validation plan](validation-plan.md) defines acceptance.

Additional primary references: [Microsoft Windows hooks](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-setwindowshookexw), [DLL initialization rules](https://learn.microsoft.com/en-us/windows/win32/dlls/dynamic-link-library-best-practices), [DX12 swap-chain creation](https://learn.microsoft.com/en-us/windows/win32/api/dxgi1_2/nf-dxgi1_2-idxgifactory2-createswapchainforhwnd), [shared fences](https://learn.microsoft.com/en-us/windows/win32/api/d3d11_4/nf-d3d11_4-id3d11device5-opensharedfence), [Epic's screen-percentage history](https://dev.epicgames.com/documentation/unreal-engine/screen-percentage-with-temporal-upsample?application_version=4.27), and [Intel's XeSS 3 overview](https://www.intel.com/content/www/us/en/developer/topic-technology/gamedev/xess.html).
