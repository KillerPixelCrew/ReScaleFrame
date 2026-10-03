# Unity Mono

`rsf_game_unity_mono` builds `ReScaleFrame.Game.UnityMono.dll` through the existing game-plugin
ABI. It is the shared plugin shell for reflection and Harmony adapters across Unity Mono games.
Drag'n Wash is the first recognized build and URP RenderGraph validation case.

The plugin will own Unity discovery, engine hooks, camera/frame conventions and SR/UI reinsertion.
The shared orchestrator retains vendor SDKs, GPU interoperability and presentation. URP, HDRP
and built-in rendering require separate adapters. IL2CPP is outside the initial Mono adapter.

Detection currently requires the exact x64 Drag'n Wash executable name and SHA256. A different
build remains unknown, including other Unity Mono games. Recognition only establishes identity.
`prepare` and `start` return `RSF_ERROR_NOT_READY`; status remains inactive and not ready.
The scaffold retains no host services and installs no native or managed hooks, so refused
preparation can be quiesced and stopped repeatedly.

The contract fixture loads the DLL in a test process and verifies identity, wrong-build and
architecture refusal, ABI and structure guards, inactive lifecycle and cleanup. This is DLL
contract validation, not a Mono bootstrap, Harmony patch, graphics-device or game test.

```powershell
./eng/verify.ps1 -Configuration Release -VS2026
ctest --preset windows-release-vs18 -R game_unity_mono_plugin_contract
```

Use the Visual Studio 2022 preset on a machine that has VS 2022. The DLL lands under
`build/windows-x64/bin/<Config>/`.

[Plugin design](../../docs/research/unity-mono-plugin.md) records the inspected methods, proposed
bootstrap, semantic patch guards, native execution and lifecycle requirements.
[First game evidence](../drag-n-wash/engine.json) keeps build identity separate from capability.
