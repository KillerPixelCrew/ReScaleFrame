# Ace Combat 7

**DLSS/DLAA, FSR 2/3/4 and XeSS Super Resolution for AC7.**

The integration starts with the game and keeps the original lighting, post-processing and UI.
Menus, HUD, briefing terrain and aircraft icons render at full output resolution, independently
of the selected quality preset. The corrected TrueSky depth path keeps clouds and aircraft details clean.

[Download the AC7 release](https://github.com/KillerPixelCrew/ReScaleFrame/releases/latest)
· [Installation details](../../docs/releases/ac7-install.md)
· [Report an issue](https://github.com/KillerPixelCrew/ReScaleFrame/issues)

## Supported setup

- Ace Combat 7 on Steam, Windows x64. Tested executable: build **9855922**, app **502500**.
- Windows 10 or 11 and a GPU supporting the selected backend; DLSS requires NVIDIA RTX.
- A current GPU driver and the
  [Microsoft Visual C++ v14 Redistributable, x64](https://aka.ms/vc14/vc_redist.x64.exe).

The release includes all required SR runtime files and the overlay. The corrected build was
accepted in game on RTX 4070 Laptop. Other GPU families, game builds and Proton have not received
the same AC7 visual validation. Frame generation and Reflex remain separate work.

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

## Integration notes

AC7 uses a modified UE4.18 renderer on D3D11. The proxy loads the game plugin, which owns guarded
engine hooks, per-view resources and reconstruction insertion before bloom, exposure and tonemapping.
The shared runtime owns vendor SDKs and the GPU-ordered D3D11/D3D12 SR bridge.

The view reader handles the game's 4096-byte uniform buffer. Scene motion needs UE encoding
removal, and the engine's camera transform supplies camera motion. The integration promotes the
game's own UI/composition targets, preserving its grading and glow. Enlarged separate translucency
uses matching view/depth selections and unjittered constants across VS, PS, HS, DS and GS.

- [Accepted result and debugging history](../../docs/research/ac7-consumer-session.md)
- [Executable and renderer evidence](engine.json)
- [Hook sites and expected bytes](../../docs/research/ue418-hook-map.md)
- [Capture research](../../docs/research/ac7-frame-capture.md)
- [Framework implementation tracker](../../docs/implementation.md)

ReScaleFrame is GPL-3.0-only except for its MIT Game SDK. NVIDIA, AMD and Intel runtime files
retain their separate included terms and notices.
