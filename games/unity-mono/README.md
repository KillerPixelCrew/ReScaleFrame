# Unity Mono

`rsf_game_unity_mono` builds `ReScaleFrame.Game.UnityMono.dll` through the existing game-plugin
ABI. It is the shared plugin shell for reflection and Harmony adapters across Unity Mono games.
Drag'n Wash is the first recognized build and URP RenderGraph validation case.

The plugin owns Unity discovery, engine hooks, camera/frame conventions and SR reinsertion.
The shared orchestrator retains vendor SDKs, GPU interoperability and presentation. URP, HDRP
and built-in rendering require separate adapters. IL2CPP is outside the initial Mono adapter.

Detection currently requires the exact x64 Drag'n Wash executable name and SHA256. A different
build remains unknown, including other Unity Mono games. Recognition only establishes identity.
The initial adapter targets Unity 6000.3 URP RenderGraph on native D3D12. Its Mono helper uses
Harmony after complete method-contract preflight; native packets retain GPU resources through
Unity completion fences. Missing runtime services or unsupported players refuse preparation.
Normal loading uses the shared RSF shim, dropped alongside original game files.
`rendering_ready` remains zero while broader scene, resize and hardware acceptance is pending.

Native contract fixtures verify identity, ABI guards and inactive cleanup in a test process.
The shipped-Mono fixture separately exercises managed packet layout, twelve URP method contracts
and Harmony patch/unpatch. Native DX12 vendor readback and live game evidence are recorded in
[the runtime notes](../../docs/research/unity-dx12-runtime-20261004.md).

Current source supplies independent DLSS-G, FSR3/4 and XeSS FG with live provider replacement,
SDK-limited multiplier controls and provider-owned Reflex/XeLL pacing. Unity's SDR pre-UI colour
and normalized depth/motion are captured separately; SR Off/FSR1 also supply FG inputs. Recorded
user acceptance covers XeSS/DLSS-G and the final FSR correction, without establishing higher MFG
counts, all scene/resize paths or Claw operation. Queried DLSS/XeSS limits determine multipliers;
the pinned AMD API provides 2x. FSR4 generation needs separate hardware evidence. See
[shared FG corrections and acceptance](../../docs/research/shared-fg-20261004.md).

```powershell
./eng/verify.ps1 -Configuration Release -VS2026
ctest --preset windows-release-vs18 -R game_unity_mono_plugin_contract
./eng/build-unity-sr.ps1 -VS2026 -UnityManagedDirectory '<game>/DragNWash_Data/Managed'
./eng/deploy-unity-sr.ps1 -GameDirectory '<game>' -FrameGeneration FSR3 -GeneratedFrames 1
./eng/package-unity-sr.ps1 -GameDirectory '<game>' -Backend Auto
```

Use the Visual Studio 2022 preset on a machine that has VS 2022. The DLL lands under
`build/windows-x64/bin/<Config>/`.

The managed build requires .NET, the researched player's Managed references, Unity native headers
and locked Harmony 2.4.2; see [dependencies](../../docs/dependencies.md). Deployment defaults to
Auto/Quality with FG Off, backs up owned RSF files and checks original-file hashes. Run deployment
only against a closed game. It copies all FG runtime families even when the default is Off.
The version carrier goes beside `DragNWash.exe`; runtime/plugin/helper/overlay live in `ReScaleFrame/`.
The packager writes a local Claw test ZIP under `.local/packages`, keeps the deployed FG defaults
and excludes preferences, logs, backups and original game files. It does not publish a release.
Unity saves FG provider selection in `ReScaleFrame/preferences.ini`; SR choices remain session
settings over `[UnitySR]` defaults. [Current status](../../docs/current-status.md) records limits.

[Plugin design](../../docs/research/unity-mono-plugin.md) records the inspected methods, proposed
bootstrap, semantic patch guards, native execution and lifecycle requirements.
[First game evidence](../drag-n-wash/engine.json) keeps build identity separate from capability.

## Source and lifecycle

| Files | Responsibility |
| --- | --- |
| [src/plugin.cpp](src/plugin.cpp) | Exact game identity and serialized native lifecycle |
| [src/mono_runtime.cpp](src/mono_runtime.cpp), [src/mono_runtime.h](src/mono_runtime.h) | Existing-player Mono export/domain discovery and owned worker attachment |
| [src/bridge.cpp](src/bridge.cpp), [src/bridge.h](src/bridge.h) | Bounded packet queue, D3D12 command/resource leases, event IDs and completion drainage |
| [unity_bridge.h](include/rescaleframe/unity_bridge.h), [managed/Native.cs](managed/Native.cs) | Matching x64 native/managed sizes, versions, resource pointers and rooted callbacks |
| [managed/Bootstrap.cs](managed/Bootstrap.cs) | Main-thread activation, producer admission and owned Harmony cleanup |
| [managed/UrpAdapter.cs](managed/UrpAdapter.cs) | Twelve-method metadata preflight, exact camera dimensions, temporal inputs, pre-postprocessing SR and completed SDR/UI boundaries |
| [managed/CpuBoundaries.cs](managed/CpuBoundaries.cs) | Removable player-loop input/simulation markers using the same frame IDs as render packets |
| [managed/AssemblyInfo.cs](managed/AssemblyInfo.cs) | Internal access for the managed contract fixture |
| [CMakeLists.txt](CMakeLists.txt), [managed project](managed/ReScaleFrame.Unity.Managed.csproj), [build properties](managed/Directory.Build.props), [package lock](managed/packages.lock.json) | Native/managed build inputs, external player references and locked Harmony dependency |

Preparation copies host services and loads the adjacent managed helper and `0Harmony.dll` into
the player's identified script domain. A fresh native worker owns its Mono attachment and
detachment. Bootstrap copies the native API, patches the render-loop boundary and defers Unity
object access until the main thread runs a supported URP/D3D12 loop. Native start enables packet
admission; active status additionally requires managed installed state. Quiesce disables new native
packets. Stop waits for pending events, native callbacks and GPU completion before removing managed
producers and releasing host services. Busy cleanup retains the plugin for retry.

Each successful enqueue copies a packet and retains its native textures in one of 32 slots.
Its opaque address must be delivered once with the reserved plugin event. Execution transfers
leases into separate three-slot SR and completed-scene command rings. Unity's returned frame-fence
identity guards allocator/resource reuse. An unknown submission completion retains ownership and
refuses teardown. Device changes require a drained restart.

The adapter admits a full-window perspective `Camera.main` base view without stereo, camera
stack, a target texture or hardware dynamic resolution. Per-camera history resets on frame gaps
and policy-generation changes. Jitter is measured after GPU projection conversion in render pixels;
FOV is radians and delta time is seconds. Resource handles resolve during graph execution. SR
records a complete spatial fallback before the native event; missing inputs or unsupported execution
retain that result. Completed-scene observation currently uses the SDR path before screen-space UI.

Shared frame generation and latency live in the runtime, using source CPU tokens and submitted
depth/motion/HUD-less copies. Reflex controls reach the selected DLSS provider; active DLSS-G
requires an effective mode of at least On.

The managed project requires `UnityManagedDirectory` (or `RSF_UNITY_MANAGED_DIRECTORY`) pointing
to the examined player's `Managed` directory. Unity reference assemblies stay external and are
not copied into the mod. Harmony resolves through the checked-in lock file. CMake's
`RSF_UNITY_NATIVE_INCLUDE` selects untracked Unity native headers; absent D3D12 headers leave the
native lifecycle buildable while graphics discovery refuses.
