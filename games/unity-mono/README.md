# Unity Mono

`rsf_game_unity_mono` builds `ReScaleFrame.Game.UnityMono.dll` through the existing game-plugin
ABI. It is the shared plugin shell for reflection and Harmony adapters across Unity Mono games.
Drag'n Wash is the first recognized build and URP RenderGraph validation case.

The plugin owns Unity discovery, engine hooks, camera/frame conventions and SR reinsertion.
The shared orchestrator retains vendor SDKs, GPU interoperability and presentation. URP, HDRP
and built-in rendering require separate adapters. IL2CPP is outside the initial Mono adapter.

Build detection, the renderer lifecycle and the Mono helper bootstrap are Rust (`rsf-unity-mono`,
linked as a staticlib). `plugin.cpp` keeps the stable `rsf_get_game_plugin_api` export and lends
the native D3D12 bridge through `rsf_unity_plugin_platform`. The bridge's frame state is Rust
too (`bridge_state.rs`): packet validation and storage, the policy generation, admission into
the command ring, source-submission bookkeeping and retirement. `bridge.cpp` keeps the Unity
graphics interfaces, D3D12 commands, resource leases, CPU input events and the bridge exports.
The Unity package compiles the same `bridge.cpp` and links the same archive. Its own D3D11 and
Vulkan bridge (`integrations/unity/native/bridge.cpp`) uses the same activity, managed stage,
callback count, packet store, policy and config; a process has one Unity renderer, so only one
bridge uses that state. It applies a policy change on its next reconstruction event and clears it
by generation, so a newer change stays pending. Its D3D12 submission ring, completion counter and
shared D3D11/D3D12 fence values are Rust as well (`bridge_interop.rs`). That `bridge.cpp` keeps
the Unity interfaces, D3D11/D3D12/DXGI calls, shared surfaces, fences and waits, D3D11 texture
leases, render-pass and CPU-event calls and the package's bridge exports. D3D11 and Vulkan packets
now pass the shared admission rules: the resources each event kind reads and a finite camera with
extents up to 16384.

The Vulkan bridge's state is Rust too (`vulkan_state.rs`): the shared device's readiness and its
single creator, the permanent fault, the render events queued to Unity with their packets, the
present interceptions and surface updates in progress, the player window frame identity the
present renders under, whether its DXGI swap chain exists, and a release that first closes the
bridge to new work. Its command slots and shared fence values use the same submission ring
(`bridge_interop.rs`). The presentation is Rust as well and outlives a release: Unity's
intercepted swapchains with their images and the present-ready semaphores the bridge made for
them, the presentation window's identity, and one exclusive presentation lease. A Present holds
the lease from the swapchain lookup through native forwarding, swapchain destruction holds it
through `vkDeviceWaitIdle` and the semaphore destruction, and the window thread takes it without
waiting to create the DXGI swap chain.
`integrations/unity/native/vulkan_bridge.cpp` keeps the Unity Vulkan interfaces, the Vulkan,
D3D12 and DXGI objects and calls, shared images, command buffers, slot fences, waits, copies and
barriers, the device and swapchain interception with semaphore creation and destruction, the
presentation window and its swap chain. An admitted Present holds its entry through native
forwarding. Swapchain setup publishes only a complete chain and rolls back earlier semaphores if a
later step or the publication refuses.
Each Vulkan event and window frame keeps the host services it was admitted with.
Drain probes hold an entry while reading native slot fences. The Unity instance
belongs to the shared-device creation and release path.
The Vulkan loader's original-function slots and the Win32 interop extension
request are Rust-owned (`vulkan_resolver.rs`). Native code still invokes the typed
Vulkan functions. The extension request falls back unchanged when preflight
cannot supply both names, and readiness is attached to the created device.
Rust also tracks the shared Vulkan image plans, publishes first-use layout
only after queue acceptance and refuses retirement for a partially submitted
round trip. Lifecycle release checks the bridge fences. Native resize and
swapchain retirement keep their existing wait calls.
The Rust image records now retain the created graphics handles. Native
recording borrows them under the presentation lease. A failed import
cleans up its partial image before the existing Rust record can change.

The Editor preview uses the same Rust submission-ring logic with its own fence and slots
(`preview.rs`). Rust selects the containing editor window, tracks visibility and admits
graphics preparation and Present. Native leaves keep window messages, resource copies and
DXGI calls. Failed fence creation can be retried, and a failed backbuffer lookup leaves
the existing command slot closed. The port preserves the source pass identity.

The Runtime setup behind the loader entry `rsf_unity_start` and `rsf_unity_fg_start`,
`rsf_unity_sr_start` and `rsf_unity_sr_stop` is Rust as well (`runtime_setup.rs`): the
`[UnitySR]` keys and defaults, the start gates and result codes, the wait for Unity's Mono
runtime and the `rsf_fg12_setup` (built by `generation.rs`, which the Unity package shares) and
`rsf_callback_host_setup` descriptors. `runtime_host.cpp` keeps those exports and lends the typed
calls into the already loaded Runtime, the SDK header tables and the D3D12 debug layer through
`rsf_unity_runtime_platform`. Path resolution for
`[UnitySR]` lives in `runtime_config.rs`; the Unity package reads its identity and `Log` path
through it from the same archive.

The Unity package's own state is in the archive too (`package_state.rs`, through
`integrations/unity/native/package_state.h`): the stored settings and their validation, the
`rsfu_status` and `rsfu_frame_counters` snapshots, the render plan, temporal readiness, the
pending reconfiguration, the session lifecycle with its `rsfu_initialize` result codes, and the
D3D12 SR views of the render callback (`package_views.rs`): one Runtime SR context per Unity view
key, Auto resolution, the backend-zero fallback, replacement on new extents or reconfiguration,
and the recording slots bound to the bridge's command lists. Its presentation is there as well
(`package_state/presentation.rs`): the package identity from `[UnitySR]` or `rsfu_initialize`,
the one generation install and the window pass's provider request. Its session services are there
too (`package_state/services.rs`): each session's host services and the managed bridge API handed
to `rsfu_initialize`, the `[UnitySR] Log` and per-process log files, the input window's hook and
restore, and the routing of the bridge's CPU events, the input window's messages and `rsfu_input`.
`package.cpp` keeps the `rsfu_*` exports, the render callback's routing, the Unity, DXGI, Vulkan
and preview work and the window guard, and lends the typed Win32, SDK and Runtime calls, including
the `rsf_sr12_*` context calls, `rsf_fg12_install` and `rsf_fg12_cpu`, through
`rsf_unity_package_platform`.

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
counts, all scene/resize paths or Claw operation. See
[shared FG corrections and acceptance](../../docs/research/shared-fg-20261004.md).

```powershell
./eng/verify.ps1 -Configuration Release -VS2026
ctest --preset windows-release-vs18 -R game_unity_mono_plugin_contract
./eng/prepare-unity-references.ps1 -UnityEditorDirectory '<Unity SDK>' -UrpReferenceDirectory '<URP SDK assemblies>' -UrpReferenceVersion '<actual URP version>'
./eng/build-unity-sr.ps1 -VS2026
# Commit the source before packaging.
./eng/package-unity-sr.ps1 -Backend Auto -FrameGeneration Auto
```

Use the Visual Studio 2022 preset on a machine that has VS 2022. The DLL lands under
`build/windows-x64/bin/<Config>/`.

The managed build requires .NET, prepared Unity/URP SDK references, Unity native headers
and locked Harmony 2.4.2; see [dependencies](../../docs/dependencies.md). No game installation
is needed to build or package. Supply `-UnityReferenceDirectory` to use another provisioned cache.
References are compile-only; the helper still checks the target player's private URP contracts
before installing hooks. A successful compile against a newer SDK does not establish support
for that player. [Release build evidence](../../docs/releases/v0.1.1-drag-n-wash.md).

For an explicitly requested local installation, `eng/deploy-unity-sr.ps1` defaults to
Auto/Quality with FG Off, backs up owned RSF files and checks original-file hashes. Run deployment
only against a closed game. It copies all FG runtime families even when the default is Off.
The version carrier goes beside `DragNWash.exe`; runtime/plugin/helper/overlay live in `ReScaleFrame/`.
The packager writes a Windows ZIP under `.local/packages` from Release outputs and the pinned
vendor SDK directories. It excludes Unity SDK references, preferences, logs, backups and original
game files. `-GameDirectory` remains an optional input for a verified deployment. Publishing the
archive is a separate release step.
Unity saves FG provider selection in `ReScaleFrame/preferences.ini`; SR choices remain session
settings over `[UnitySR]` defaults. [Current status](../../docs/current-status.md) records limits.

[Plugin design](../../docs/research/unity-mono-plugin.md) records the inspected methods, proposed
bootstrap, semantic patch guards, native execution and lifecycle requirements.
[First game evidence](../drag-n-wash/engine.json) keeps build identity separate from capability.
