# ReScaleFrame for Ace Combat 7

Scope: the corrected published v0.1.0 AC7 **SR package**, replaced on 2 October 2026.
Later `main` includes FG/Reflex and Unity work; those features are absent from this ZIP. See
[current source status](https://github.com/KillerPixelCrew/ReScaleFrame/blob/main/docs/current-status.md).

DLSS, FSR 2/3/4 and XeSS upscaling for AC7, with the game's lighting, post-processing and
interface kept intact. Menus, HUD and briefing rendering keep full output resolution.
The cloud depth path preserves small aircraft details at reduced scene resolutions.

## Requirements

- The Steam version of Ace Combat 7 on Windows 10 or 11, 64-bit.
- A GPU and current driver supporting your selected upscaler. DLSS requires an NVIDIA RTX GPU.
- The current [Microsoft Visual C++ v14 Redistributable, x64](https://aka.ms/vc14/vc_redist.x64.exe).

The tested executable is Steam build 9855922. Other game builds have not been validated.
All required SR runtime files and the overlay are included. The corrected build was accepted
in game on an RTX 4070 Laptop GPU. Other GPU families have not received the same AC7 visual validation.

## Install

1. Close AC7.
2. In Steam, right-click the game and choose **Manage > Browse local files**.
3. Find the folder containing `Ace7Game.exe`. Copy `dinput8.dll`, `ReScaleFrame.Game.AC7.dll`,
   `rescaleframe_overlay.dll`, `ReScaleFrame.ini` and the `ReScaleFrame` folder from this archive
   into that folder. When updating, replace every included file, including the vendor runtimes.
4. Launch the game normally. DLSS and its in-game composition start automatically.

If another mod already supplies `dinput8.dll`, keep a backup and do not overwrite it blindly.
This release does not chain another DirectInput proxy. You do not need to replace any original
game file or add Steam launch options on Windows.

## Use

Press **Insert** to open or close the overlay:

A startup hint shows the hotkey for eight seconds, fading during the last second. Opening the
overlay dismisses it immediately. The hint does not capture mouse input.

- **Enable upscaling** turns upscaling on or off. Turning it off restores native scene rendering.
- **Backend** selects DLSS, FSR 2, FSR 3, FSR 4 or XeSS. An unavailable selection keeps the
  previous working backend and displays the refusal.
- **Preset** selects Native/DLAA, Quality, Balanced, Performance or Ultra Performance.

The first launch uses DLSS Performance. Native runs at output resolution, using DLAA with DLSS.
The enable setting and quality preset are saved automatically; choose an alternate backend
again after restarting. The UI and briefing layer stay at full output resolution when you change presets.

FSR 4 uses the SDK's native hardware path on supported Radeon GPUs. On compatible NVIDIA/Intel
devices, the included runtime can use FSR 4 INT8 through a device-scoped compatibility hook.
This requires Shader Model 6.6 and wave operations and was device-tested on RTX 4070.
The first FSR 4 activation can pause while shaders compile; subsequent runs use the driver cache.

Settings are saved in `%LOCALAPPDATA%\ReScaleFrame\AC7.ini`.
Logs are in `%LOCALAPPDATA%\ReScaleFrame\AC7\rsf-dump.log`.
The INI beside the game contains initial defaults and advanced options; most users can leave it alone.

This release provides super resolution. Frame generation, Reflex, the standalone launcher
and WSGM integration are not included.

## Update or remove

Close the game before replacing the mod's files. Saved preferences live outside the game folder
and survive an update.

To uninstall, remove the three mod DLLs, `ReScaleFrame.ini` and the `ReScaleFrame` folder you copied
in. Restore any files you backed up. Removing `%LOCALAPPDATA%\ReScaleFrame\AC7.ini` also resets
your saved choices.

## If something goes wrong

If Windows reports a missing `MSVCP140`, `VCRUNTIME140` or `VCRUNTIME140_1` DLL, install or repair
the x64 Visual C++ runtime linked above. If the overlay does not appear, check that all three mod DLLs
are beside the executable and that another mod is not using `dinput8.dll`.

Report issues at [KillerPixelCrew/ReScaleFrame](https://github.com/KillerPixelCrew/ReScaleFrame/issues).
Include your GPU, driver version, preset, game resolution and the relevant log excerpt.

## Credits and source

ReScaleFrame is a KillerPixelCrew project. NVIDIA supplies DLSS through Streamline, AMD supplies
FidelityFX FSR and Intel supplies XeSS.
Third-party components retain their own licenses; see `licenses/` and
`streamline/nvngx_dlss.license.txt`. NVIDIA runtime files are unmodified and governed by NVIDIA's
terms, separately from ReScaleFrame's GPL license. FidelityFX and XeSS terms and notices are
included in their respective runtime folders.

Source: [ReScaleFrame v0.1.0](https://github.com/KillerPixelCrew/ReScaleFrame/tree/v0.1.0).
The release also includes a source archive and SHA-256 checksums.
