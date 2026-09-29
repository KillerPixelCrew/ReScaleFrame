# Ace Combat 7

**DLSS Super Resolution and Native/DLAA for AC7.**

The integration starts with the game and keeps the original lighting, post-processing and UI.
Briefing terrain and aircraft icons render at full output resolution, independently of the selected
DLSS preset, with projection jitter removed through the terrain's tessellation stages.

[Download the AC7 release](https://github.com/KillerPixelCrew/ReScaleFrame/releases/latest)
· [Installation details](../../docs/releases/ac7-install.md)
· [Report an issue](https://github.com/KillerPixelCrew/ReScaleFrame/issues)

## Supported setup

- Ace Combat 7 on Steam, Windows x64. Tested executable: build **9855922**, app **502500**.
- Windows 10 or 11 and an NVIDIA RTX GPU with DLSS support.
- A current NVIDIA driver and the
  [Microsoft Visual C++ v14 Redistributable, x64](https://aka.ms/vc14/vc_redist.x64.exe).

The release includes the required Streamline and DLSS runtime files. Other game builds and Proton
have not been validated with this release. Frame generation, Reflex, XeSS and FSR are separate work.

## Install

1. Close the game and download `ReScaleFrame-0.1.0-AC7-Windows-x64.zip`.
2. In Steam, choose **Manage > Browse local files** and find `Ace7Game.exe`.
3. Extract the archive into that folder. Both mod DLLs and `ReScaleFrame.ini` go beside the
   executable, along with the included `ReScaleFrame` folder.
4. Launch normally. DLSS and reinsertion start automatically when the renderer is ready.

Keep the original game executable. No Steam launch options are needed on Windows. If another mod
already uses `dinput8.dll`, do not overwrite it: this release does not chain another DirectInput proxy.

## Use

Press **Insert** to open or close the overlay. Its two controls are **Enable DLSS** and the presets.

| Preset | Behaviour |
| --- | --- |
| Native | DLAA at output resolution |
| Quality | Highest input resolution of the upscaling presets |
| Balanced | Lower input resolution than Quality |
| Performance | Lower input resolution than Balanced; the first-launch default |
| Ultra Performance | Lowest input resolution |

DLSS supplies the render sizes. The briefing layer stays at **100% output resolution** across
preset changes. Turning DLSS off restores native scene rendering. Your choices are saved in
`%LOCALAPPDATA%\ReScaleFrame\AC7.ini` and restored on the next launch.

The INI beside the game contains initial defaults and advanced options. Logs are written to
`%LOCALAPPDATA%\ReScaleFrame\AC7\rsf-dump.log`.

## Update, troubleshoot or remove

Close AC7 before updating the mod. If Windows reports a missing MSVCP140 or VCRUNTIME140 DLL,
install or repair the x64 Visual C++ runtime linked above. If Insert does nothing, check the DLL
locations and any other mod using `dinput8.dll`.

When reporting an issue, include your GPU, driver, preset, output resolution and a relevant log
excerpt. The [full installation guide](../../docs/releases/ac7-install.md) has the remaining details.

To uninstall, remove the two mod DLLs, `ReScaleFrame.ini` and the installed `ReScaleFrame` folder.
Restore any files you backed up. Delete `%LOCALAPPDATA%\ReScaleFrame\AC7.ini` if you also want
to reset your saved preferences.

## Integration notes

AC7 uses a modified UE4.18 renderer on D3D11. The working release runs through the proxy entry
point. The separate public plugin lifecycle still reports `rendering_ready = 0`; migrating the
working hooks into that lifecycle remains framework work.

The view reader handles the game's 4096-byte uniform buffer. Scene motion needs UE encoding
removal, and the engine's camera transform supplies camera motion. The integration promotes the
game's own UI/composition targets, preserving its grading and glow. Enlarged separate translucency
uses matching view/depth selections and unjittered constants across VS, PS, HS, DS and GS.

- [Accepted result and debugging history](../../docs/research/ac7-consumer-session.md)
- [Executable and renderer evidence](engine.json)
- [Hook sites and expected bytes](../../docs/research/ue418-hook-map.md)
- [Capture research](../../docs/research/ac7-frame-capture.md)
- [Framework implementation tracker](../../docs/implementation.md)

ReScaleFrame is GPL-3.0-only except for its MIT Game SDK. NVIDIA supplies DLSS through Streamline;
its bundled runtime files retain their separate included terms and notices.
