# Unity Mono

`rsf_game_unity_mono` builds `ReScaleFrame.Game.UnityMono.dll` through the existing game-plugin
ABI. It is the shared plugin shell for reflection and Harmony adapters across Unity Mono games.
Drag'n Wash is the first recognized build and URP RenderGraph validation case.

The plugin owns Unity discovery, engine hooks, camera/frame conventions and SR reinsertion.
The shared orchestrator retains vendor SDKs, GPU interoperability and presentation. URP, HDRP
and built-in rendering require separate adapters. IL2CPP is outside the initial Mono adapter.

Build detection, the renderer lifecycle, runtime setup and the Mono helper bootstrap are Rust
(`rsf-unity-mono`, `src/`, linked as a staticlib); `plugin.def` publishes the Rust
`rsf_get_game_plugin_api` entry. The shared frame state, bridges and package state are the
`rsf-unity` crate in `integrations/unity` (`src/` and `native/`): the Unity package links it whole and
this plugin bundles its `mono-plugin` subset. A process has one Unity renderer, so only one bridge
uses that state. Rust owns the state and policy; C keeps
only the Unity graphics interfaces, the D3D11/D3D12/DXGI/Vulkan calls on vendor and engine objects and
the exception guards.

| Rust owns (`integrations/unity/src` unless marked) | Files |
| --- | --- |
| Frame state: packet validation and storage, policy generation, command-ring admission, source-submission bookkeeping, retirement | `bridge_state.rs` |
| D3D12 submission ring, completion counter, shared D3D11/D3D12 fence values | `bridge_interop.rs`, `bridge_d3d12.rs`, `bridge_d3d11.rs`, `d3d11_pass.rs`, `d3d11_state.rs` |
| Vulkan bridge state, shared-device readiness and its single creator, queued render events, present interceptions, DXGI swap-chain ownership, presentation lease | `vulkan_state.rs`, `vulkan_bridge.rs`, `vulkan_bridge/` |
| Vulkan typed graphics calls, HWND swapchain, shared images, command pool and slots | `vulkan_platform.rs`, `vulkan_platform/` |
| Vulkan resolver keys, interceptor table, saved loader functions, Win32 interop extension selection | `vulkan_resolver.rs` |
| Editor preview window selection and validation | `preview.rs` |
| Runtime setup behind `rsf_unity_start`, `rsf_unity_fg_start`, `rsf_unity_sr_start`, `rsf_unity_sr_stop`: `[UnitySR]` keys, start gates, result codes, wait for Mono, FG descriptors | `runtime_setup.rs` (this crate), `generation.rs`, `runtime_config.rs` |
| Package state: settings, `rsfu_status` and `rsfu_frame_counters`, render plan, session lifecycle, per-view SR contexts, presentation, session services | `package_state.rs`, `package_state/`, `package_views.rs` |

C keeps `integrations/unity/native`: `render_bridge.c`, `package.c` and `vulkan_bridge.c` (Unity graphics dispatch, the `rsfu_*` and bridge
exports, guards for foreign callbacks, and the typed Win32, SDK and Runtime calls the package lends
through `rsf_unity_package_platform`). The Editor preview lends Runtime a destination token and a
source with its D3D12 state; Runtime owns the per-output swapchain, copy and submission fence and
waits for completion before replacing the old swapchain.

The behaviour recorded for these owners lives in their dated research notes
(`docs/research/rust-unity-*.md`) and [the implementation tracker](../../docs/implementation.md).
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
