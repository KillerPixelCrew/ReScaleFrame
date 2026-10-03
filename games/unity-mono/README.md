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
The shipped-Mono fixture separately exercises managed packet layout, nine URP method contracts
and Harmony patch/unpatch. Native DX12 vendor readback and live game evidence are recorded in
[the runtime notes](../../docs/research/unity-dx12-runtime-20261004.md).

```powershell
./eng/verify.ps1 -Configuration Release -VS2026
ctest --preset windows-release-vs18 -R game_unity_mono_plugin_contract
./eng/build-unity-sr.ps1 -VS2026 -UnityManagedDirectory '<game>/DragNWash_Data/Managed'
./eng/deploy-unity-sr.ps1 -GameDirectory '<game>'
./eng/package-unity-sr.ps1 -GameDirectory '<game>' -Backend Auto
```

Use the Visual Studio 2022 preset on a machine that has VS 2022. The DLL lands under
`build/windows-x64/bin/<Config>/`.

[Plugin design](../../docs/research/unity-mono-plugin.md) records the inspected methods, proposed
bootstrap, semantic patch guards, native execution and lifecycle requirements.
[First game evidence](../drag-n-wash/engine.json) keeps build identity separate from capability.
