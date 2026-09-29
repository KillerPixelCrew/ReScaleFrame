# ReScaleFrame for Ace Combat 7

DLSS upscaling for AC7, with the game's lighting, post-processing and interface kept intact.
The briefing terrain and aircraft icons render at full output resolution independently of the
selected preset.

## Requirements

- The Steam version of Ace Combat 7 on Windows 10 or 11, 64-bit.
- An NVIDIA RTX GPU with DLSS support and a current NVIDIA driver.
- The current [Microsoft Visual C++ v14 Redistributable, x64](https://aka.ms/vc14/vc_redist.x64.exe).

The tested executable is Steam build 9855922. Other game builds have not been validated.
The required Streamline and DLSS runtime files are included.

## Install

1. Close AC7.
2. In Steam, right-click the game and choose **Manage > Browse local files**.
3. Find the folder containing `Ace7Game.exe`. Copy `dinput8.dll`, `rescaleframe_overlay.dll`,
   `ReScaleFrame.ini` and the `ReScaleFrame` folder from this archive into that folder.
4. Launch the game normally. DLSS and its in-game composition start automatically.

If another mod already supplies `dinput8.dll`, keep a backup and do not overwrite it blindly.
This release does not chain another DirectInput proxy. You do not need to replace any original
game file or add Steam launch options on Windows.

## Use

Press **Insert** to open or close the overlay. It has two controls:

- **Enable DLSS** turns upscaling on or off. Turning it off restores native scene rendering.
- **Preset** selects Native/DLAA, Quality, Balanced, Performance or Ultra Performance.

The first launch uses Performance. Native runs DLAA at output resolution. Your selections are
saved automatically and restored next time. The briefing layer stays at 100% output resolution
when you change presets.

Settings are saved in `%LOCALAPPDATA%\ReScaleFrame\AC7.ini`.
Logs are in `%LOCALAPPDATA%\ReScaleFrame\AC7\rsf-dump.log`.
The INI beside the game contains initial defaults and advanced options; most users can leave it alone.

This release provides DLSS Super Resolution and DLAA. Frame generation, Reflex, XeSS and FSR
are planned separately. The standalone launcher and WSGM integration are not included.

## Update or remove

Close the game before replacing the mod's files. Saved preferences live outside the game folder
and survive an update.

To uninstall, remove the two mod DLLs, `ReScaleFrame.ini` and the `ReScaleFrame` folder you copied
in. Restore any files you backed up. Removing `%LOCALAPPDATA%\ReScaleFrame\AC7.ini` also resets
your saved choices.

## If something goes wrong

If Windows reports a missing `MSVCP140`, `VCRUNTIME140` or `VCRUNTIME140_1` DLL, install or repair
the x64 Visual C++ runtime linked above. If the overlay does not appear, check that both mod DLLs
are beside the executable and that another mod is not using `dinput8.dll`.

Report issues at [KillerPixelCrew/ReScaleFrame](https://github.com/KillerPixelCrew/ReScaleFrame/issues).
Include your GPU, driver version, preset, game resolution and the relevant log excerpt.

## Credits and source

ReScaleFrame is a KillerPixelCrew project. NVIDIA supplies DLSS through the Streamline SDK.
Third-party components retain their own licenses; see `licenses/` and
`streamline/nvngx_dlss.license.txt`. NVIDIA runtime files are unmodified and governed by NVIDIA's
terms, separately from ReScaleFrame's GPL license.

Source: [ReScaleFrame v0.1.0](https://github.com/KillerPixelCrew/ReScaleFrame/tree/v0.1.0).
The release also includes a source archive and SHA-256 checksums.
