# Latency integration audit

13 November 2026, baseline 1e9843a. Source audit, not game acceptance. The contract is
[input-keyed latency](input-keyed-latency.md), Game ABI 21, event version 1, D3D11 table 2.
P = present, M = missing or policy outside the core, N/A = unavailable observer capability.
References below describe the baseline, before migration. A received-input event records kind, not key values.

| Integration | One pre-input pace / real sample | Received kinds | PCL ping | XeLL | AMD owner | Owner/counters surface |
| --- | --- | --- | --- | --- | --- | --- |
| AC7/proxy | M: legacy PACING at native_renderer.cpp:576, real sample :542/:557 | M: message dequeue :393 present; controller poll :551 reported an attempt rather than received data | P: dequeue :406, core recognises ID | P: D3D11 bridge | P: native_fg.cpp:495, shared only with Anomaly so far | P: dlss_bridge.c:2989 / shared overlay |
| RDR2/D3D12 | M: scene_boundary.cpp:62/:158 reports SDK sleep/markers; native_fg_d3d12.cpp:425 interprets them separately | M: no received-kind facts | M: no input-window routing | P: provider path; distinct pacing mapper | M: no shared AL2 owner | P: render_host.cpp:732, lacks shared kind/order counters |
| Unity Mono / Drag'n Wash | M: managed/CpuBoundaries.cs:37 places PACING; bridge.cpp:264 emits legacy events before EarlyUpdate | M: no received-kind facts | M: no message forwarding | P: callback_render_host.cpp:338 -> fg12; separate mapper | M: no shared AL2 owner | P: callback host overlay; incomplete owner counters |
| Unity URP package | M: ReScaleFrameRuntime.cs:141 places PACING before EarlyUpdate; bridge.cpp:227 legacy events | P: InputSystem/InputMarkers.cs:16 and managed NotifyInput | M: native/package.cpp:373 selects/queues SDK ping itself | P: fg12 provider, separate mapper | M: no shared AL2 owner | P: package.cpp:360; direct SDK status reads |
| Godot D3D12 | M: modules/rescaleframe/bridge.cpp:104 legacy facts; native/runtime.cpp:166 routes separately | P: module input_event :360 | M: runtime.cpp:174 selects/queues ping | P: fg12 provider | M: no shared AL2 owner | P: runtime.cpp:390, direct SDK counters |
| Godot Vulkan | M: native/vulkan.cpp:247 maps markers and sleep in adapter | P: module input_event :360 | M: adapter owns routing | M: adapter owns XeLL/Reflex exclusion at :198/:253 | M: no native AMD Vulkan owner; only the managed host implements it | P: vulkan.cpp:207; adapter merges SDK states |
| Minecraft/OpenGL bridge | M: NativeRenderer.java:73 places PACING; gl_pipeline.cpp:186 uses fg12 mapper | P: Java input tracker/mixins -> JNI :96 | M: JNI :90 plus gl_pipeline.cpp:195 expose SDK message ID to adapter | P: fg12 provider | M: no shared AL2 owner | P: game settings and pipeline status, needs common counters |
| Project Wingman | N/A: observer only, no supported input/render lifecycle | N/A | N/A | N/A | N/A | P: observer capability/refusal status |

Paths are under games/ac7/src, loader/proxy/src, games/rdr2/src, games/unity-mono, integrations/unity,
integrations/godot and games/minecraft respectively; shared source references are runtime/orchestrator/src.
Observer-only Wingman must not advertise active pacing from executable recognition or observed Presents.

The migration moves owner selection, token reservation, pre-input pacing, marker mapping/order/deduplication,
registered-ping matching, fallbacks and counters into common Runtime code. Hosts retain resource/queue ownership
and forward facts. The core derives and reports missing facts; no adapter invents a measured input callback.
Vulkan retains its API-specific driver calls in the core, with the same CPU fact normalization and ownership rule.
Validation and final file:line references are appended after implementation.

## Implemented migration

13 November 2026. P means implemented, with validation scoped below, not game accepted.
Common references: C = runtime/orchestrator/src/native_fg.cpp:515 (Direct3D owner), :622 (CPU facts),
:734 (render), :755 (status); N = runtime/orchestrator/src/native_cpu.cpp:45 (normalization/order/ping);
V = runtime/orchestrator/src/native_latency_vulkan.cpp:143 (Vulkan owner), :163 (CPU facts).
All paths use N. C supplies Reflex/PCL, required XeLL and Anti-Lag 2 to both Direct3D APIs.
V uses the same priority in runtime/orchestrator/src/latency_policy.h:6 and CPU ledger.

| Integration | Single pre-input pace | Real sample / simulation start | Received kinds | PCL ping | XeLL | AMD owner | Owner/counters surface |
| --- | --- | --- | --- | --- | --- | --- | --- |
| AC7/proxy | P C; loader/proxy/src/dlss_bridge.c:74 dispatches normalized facts | P games/ac7/src/native_renderer.cpp:553/:566, native Slate/message pump | P :395 removed messages/raw; :548 successful XInput reads inside poll | P :395 -> N | P C | P C D3D11/bridge | P loader/proxy/src/dlss_bridge.c:2699 shared overlay |
| RDR2 | P C; runtime/orchestrator/src/native_fg_d3d12.cpp:430 converts observed wait | P games/rdr2/src/scene_boundary.cpp:99 wait plus native input marker, core compatibility mapping | P :65/:90 removed messages/raw and successful XInput reads | P :76 -> N | P C | P C D3D12 | P runtime/orchestrator/src/render_host.cpp:475 shared overlay |
| Unity Mono / Drag'n Wash | P C via runtime/orchestrator/src/callback_render_host.cpp:338 | P games/unity-mono/managed/CpuBoundaries.cs:48 and shared InputLoop.cs:25; absent input phase omits sample | P CpuBoundaries.cs:19 optional Input System state/delta or legacy changes; native bridge.cpp:278 messages | P bridge.cpp:278 -> N | P C | P C D3D12 | P shared callback-host overlay/core status |
| Unity URP package | P C via native/bridge.cpp CPU callback | P Runtime/ReScaleFrameRuntime.cs:139 and Engine/InputLoop.cs:25 around actual EarlyUpdate input nodes | P Runtime/InputSystem/InputMarkers.cs:16, NotifyInput, legacy collector and native messages | P native/package.cpp -> N | P C | P C D3D12 | P Runtime/ReScaleFrameStatus.cs:29; Samples~/TemporalDemo/TemporalDemo.cs:130 displays owner/kinds/pings/derived/order |
| Godot D3D12 | P C | P integrations/godot/modules/rescaleframe/bridge.cpp:334 before event/key processing; :343 simulation | P :361 real engine events; unframed outside update | P native/runtime.cpp message reporter -> N | P C | P C D3D12 | P native/runtime.cpp:383 -> runtime_control.cpp:93 / module dictionary |
| Godot Vulkan | P V | P same module facts | P same module facts | P V/N | P V, borrowed presentation context; no adapter pacing/marker mapping | P runtime/orchestrator/src/latency_amd_vulkan.cpp:59 query and :151/:165 INPUT/PRESENT | P native/runtime.cpp:351 core Vulkan status and N counters |
| Minecraft / GL bridge | P C via runtime/orchestrator/src/gl_pipeline.cpp:188 | P games/minecraft/src/main/java/org/kpc/rescaleframecraft/render/NativeRenderer.java:231 actual post-Present GLFW poll; CpuFrameFacts.java:10 next-source identity | P render/InputFrames.java and mixins -> native/jni.cpp:90 | P native/input_observer.h removed messages -> JNI -> N | P C | P C D3D12 bridge | P NativeRenderer.java:237 live settings/status; jni.cpp:111 core snapshot |
| Project Wingman | N/A observer only | N/A | N/A | N/A | N/A | N/A | P loader/proxy/src/wingman_loader.cpp:82 and games/project-wingman/src/native_observer.cpp:180 explicit refusal |

Unity paths are relative to integrations/unity/com.kpc.rescaleframe or integrations/unity/native as named.
The shared InputLoop preserves existing engine/mod delegates, inserts immediately before the first and after the
last actual input node, installs idempotently, and removes only its own nodes. The contract fixture covers those
operations and verifies that a missing phase is not reported as measured. Unity Mono observes processed STAT/DLTA
events and classifies the actual device, with legacy key/button transitions and mouse changes as fallback.

AC7 no longer counts an empty controller poll as receipt. integrations/windows/xinput_facts.h observes successful
GetState/ordinal-100 reads from every loaded XInput module without loading one or polling itself. Failed reads and
calls outside the owning input update do not become controller receipts. Native callbacks participate in draining.
The carrier no longer selects Reflex from the FG provider or reads RSF_REFLEX_ASYNC. Saved latency settings pass
through; core selects the technology independently of SR/FG provider. Deprecated generic SDK-query exports retain
ABI compatibility, but no migrated integration uses them. No new user option was added.

## Vulkan correction and evidence

Initial task wording suggested an existing native AMD Vulkan implementation. Source inspection found it only in
runtime/managed/ReScaleFrame.Vulkan/Latency/VendorLatency.cs and AmdAntiLagFunctions.cs. The coordinator confirmed
the correction and requested a native port. The baseline table now records the actual absence.
The port uses the standard Vulkan extension, with no additional AMD binary or copied vendor implementation.

Core queries AMD vendor ID, extension enumeration and PhysicalDeviceAntiLagFeaturesAMD, then returns requirements
through native_latency_vulkan_requirements.h ABI 1. Godot clones its known feature chain, appends returned extension/
feature requirements without interpreting policy, creates the device, and reports the actual successful request.
Optional feature/extension failure retries the exact caller request. Unknown feature nodes prevent modification;
missing enabled extension, feature or function refuses the AMD owner. Destruction notifies core before destroying
the device. Returned feature nodes live through synchronous creation on that thread; the engine chain is unchanged.

AMD core keeps 128 bounded source keys, pairing INPUT/PRESENT with the same uint64 source once each. Skipped sources
cancel their owed PRESENT; Off emits no pair. State is snapshotted and locks released before INPUT, which can block.
A two-thread stub proves an older PRESENT advances while the next INPUT waits. Shared mode/interval supply driver
settings, Boost maps to On, and microseconds convert to maxFPS. A named process-local event arbitrates native D3D,
native Vulkan and managed Vulkan ownership, preventing two pacing owners. Managed SDK semantics otherwise remain.

Primary references: [vkAntiLagUpdateAMD](https://docs.vulkan.org/refpages/latest/refpages/source/vkAntiLagUpdateAMD.html),
[AMD Anti-Lag extension proposal](https://docs.vulkan.org/features/latest/features/proposals/VK_AMD_anti_lag.html),
[Unity InputSystem.onEvent](https://docs.unity3d.com/Packages/com.unity.inputsystem@1.17/api/UnityEngine.InputSystem.InputSystem.html).
Tests use stub extension queries and function pointers; no AMD hardware is available.

Game ABI stays 21 / event v1 / D3D11 table 2. Native Unity appends cpu_fact and becomes API 5 (72 bytes), Unity package
status ABI becomes 2 (184 bytes), Godot native ABI becomes 2, Vulkan latency ABI becomes 2, and Vulkan device
requirements are ABI 1. Short/mismatched calls refuse. Package builders carry the existing Anti-Lag 2 MIT notice on
affected D3D12 paths. Readiness flags and prior game acceptance scopes are unchanged. No new executable hook site,
RVA or expected guard was needed for this cross-integration migration.

## Required game checks

| Integration | What a game run must show | Still unverified here |
| --- | --- | --- |
| AC7 | One core pace per input source; native Slate/pump facts; owner shown; K/M and successful controller receipt counts grow; ping grows with a tool; zero order errors through menus, flight, loading and FG switches | Actual XInput coverage/controller hardware, in-game marker order, physical latency |
| RDR2 | Observed native wait and input marker share a source; one paced update; real kinds/pings grow; zero order errors through load/pause/focus and providers | New Windows observers and revised core mapper in the actual process |
| Drag'n Wash / Unity Mono | Found EarlyUpdate input nodes; sample before engine input; kinds follow actual receipt; shared overlay owner/counters; stable pause/load/render-graph changes | Deployed Mono/Harmony loop execution and game input modes after migration |
| Unity URP package | Input System and legacy projects report real kinds and pings; sample UI owner/counters; resets drain cleanly | Actual editor/player engine execution, project input configuration and controllers |
| Godot D3D12 | Pre-event-pump sample, engine kinds/pings, core owner/status; live mode/limit changes; no duplicate pace with FG | Rebuilt deployed module, threaded frame order, AMD driver |
| Godot Vulkan | Supported AMD device actually enables extension+antiLag; same-source INPUT/PRESENT once each; absent driver reports none; required XeLL selects one owner | Real AMD Vulkan feature/device negotiation and pacing; Vulkan device fixtures need explicit SDK inputs |
| Minecraft | Post-Present poll reserves the following simulation source; settings show owner/pace/kinds/pings/derived/order; focus/world loads/skipped renders do not leak inputs | Actual Minecraft/GL bridge run, controller mod, Windows dequeue observer |
| Project Wingman | Observer/refusal status remains explicit, with no active latency claim | Supported input/render integration is unimplemented |

First game check: matching artifacts load with no hook/event/ABI refusal, then confirm the selected owner, real input
receipts, tool pings and zero marker-order violations.

Counters prove protocol activity, not lower physical input-to-display latency. No game was launched or changed for
this task. Validation below distinguishes source, synthetic, device and game evidence.


Minecraft source verification corrected two additional baseline assumptions: runTick HEAD is not the GLFW poll,
and NativeRenderer still contained a Java parkNanos frame cap. The mapped 1.21.1 engine calls pollEvents both before
and after glfwSwapBuffers in RenderSystem.flipFrame. RenderSystemMixin now reports INPUT_SAMPLE immediately before
the second, post-Present poll, reserving the next simulation source. The following runTick reports simulation begin
on that same source. Pre-Present receipts are explicitly unframed and bounded by the core; initial activation reports
no fabricated sample. MouseHandler.handleAccumulatedMovement supplies simulation end even when noRender skips the
render callback. Present/abort closes the engine source; recursive display calls cannot reserve a second source.
Java no longer waits or caps frames. Its existing setting is translated to the core interval, and Minecraft's own
independent cap still applies as its UI already says. InputBoundaryBytecodeTest verifies the actual mapped engine
call order and targeted simulation-end method without launching Minecraft; CpuFrameFactsTest verifies cross-update
identity, recursion and first-activation/reset behavior. GL Runtime refuses input tagged to a completed output and
releases its core CPU session after drained shutdown. Native GL fixture now supplies event-v1 facts without PACING.

## Validation

Windows MSVC Release, assigned build/wk-avx only. The final complete native build and full CTest run pass:
222 discovered, 210 passed, 12 optional tests skipped. The first run had one cursor-position assertion failure in
anomaly_overlay_visibility; its immediate rerun and subsequent full runs passed. No assertion was weakened.
eng/verify.ps1 hardcodes the shared build directory, which the coordinator reserved. Its equivalent version check,
configure/build/CTest, cargo fmt and clippy gates ran explicitly with the assigned directory; Rust tests ran as well.

| Check | Result and scope |
| --- | --- |
| Anomaly standard + AVX evidence | 62 passed: 8 original PE/profile checks and 54 negative mutations; no executable/profile sites changed |
| RDR2 D3D12 host, SR backends 1/3/4/5, vendor 0x10de | All passed with RSF_RDR2_HOST_ROOT at the staged ReScaleFrame folder; FSR3 FG; 140 input-only updates followed by 40 rendered sources per backend; owner Reflex, 180 paced, K/M/C 40/40/40, 40 pings, zero order violations |
| Independent Reflex/PCL SDK fixtures | Both passed on NVIDIA: Reflex 12 sleeps/96 markers, PCL 0 sleeps/96 markers, source IDs above 32 bits; live-token reset refusal and source-session restart checked |
| D3D11 bridge | Explicit plain, --switch and --borderless fixtures passed with NVIDIA and actual Streamline/FSR/XeSS runtimes |
| Native GL/D3D12 pipeline | RTX 4070 Laptop GPU; sharing/fences, separate event/render threads, native SR/FG switches, resizing and shutdown passed; 510 paced updates, K/M/C 416/416/416, 416 pings, zero order violations/substitute polls |
| Minecraft Gradle | 46 tests passed, no skips; includes actual mapped-engine bytecode and frame-fact identity contracts. Its 23 production GL shader checks used Intel UHD 770; the separate native GL/D3D12 pipeline check above used NVIDIA |
| Unity Mono helper | Builds and contract executable passes against matching 6000.3.14f1 shipped managed references read-only; actual input-loop insertion, cleanup and fallback contract, API layout, and shipped URP metadata checked in .NET, not deployed Mono/Harmony |
| Unity package | Managed/native size and all-offset checks, native settings round trip and existing settings tests pass; Release native package builds |
| Godot | Native GDExtension builds; headless settings Resource test passes; native device-creation contract covers private chain copy, absent capability, optional-feature refusal/fallback and actual enabled-request reporting. The patched engine module was not rebuilt/launched |
| Shared Vulkan latency and AMD | Stub contracts pass for owner exclusion, XeLL input dedup/no stacked Reflex sleep, absent extension/feature/function, Off/Boost/FPS limits, full-width pairing, skipped/duplicate sources and concurrent INPUT/PRESENT. Failed SDK shutdown retains its lease/session; successful shutdown permits a fresh source 1 without clearing another session |
| Managed Vulkan | Locked restore and Release build pass; two existing XML documentation warnings in LatencySession.cs. Managed/native-compatible pacing lease and optional Input System receipt observer tests pass |
| Rust | fmt check, clippy with warnings denied, 74 workspace tests and Release overlay build pass; AMD Vulkan owner has a distinct label from D3D Anti-Lag 2 |

The 12 default CTest skips are gated tests needing explicit runtime inputs or enabling variables. Independent
parameterized runs above cover latency_host, d3d11_present_bridge and gl_shared_surface. Other gated skips remain
explicit in the report. Native Vulkan GPU/device acceptance, AMD hardware, actual game/controller paths and physical
latency remain unverified. Evidence logs and the exact commands/file manifest are in
.local/anomaly/latency-migration/report.md. No game installation was modified or game launched.
