# ReScaleFrame

**DLSS upscaling for Ace Combat 7.**

ReScaleFrame adds DLSS to AC7 while keeping the game's lighting, post-processing and interface.
It starts when the game launches, saves your settings, and keeps the briefing terrain and aircraft
icons at full output resolution independently of your upscaling preset.

A [KillerPixelCrew](https://github.com/KillerPixelCrew) project.

**[Download the latest release](https://github.com/KillerPixelCrew/ReScaleFrame/releases/latest)**
· [Report an issue](https://github.com/KillerPixelCrew/ReScaleFrame/issues)
· [Release notes](https://github.com/KillerPixelCrew/ReScaleFrame/releases)

## What you get

- DLSS Super Resolution, plus Native/DLAA.
- Automatic startup and in-game composition with the original post-processing and UI.
- A small overlay with an enable switch and preset selection.
- Full-resolution briefing terrain and aircraft icons with projection jitter removed, including
  the terrain's tessellation stages.
- Settings that carry over to your next session.

The current release is for **Ace Combat 7, Steam, Windows x64**. The tested game is Steam build
**9855922**. Frame generation, Reflex, XeSS and FSR are planned separately. Other game builds and
Proton have not been validated with this release.

## Requirements

- Windows 10 or 11, 64-bit.
- An NVIDIA RTX GPU with DLSS support and a current NVIDIA driver.
- The [Microsoft Visual C++ v14 Redistributable, x64](https://aka.ms/vc14/vc_redist.x64.exe).

The release ZIP includes the required Streamline and DLSS runtime files.

## Install

1. Download **ReScaleFrame-0.1.0-AC7-Windows-x64.zip** from the
   [release page](https://github.com/KillerPixelCrew/ReScaleFrame/releases/latest).
2. Close AC7. In Steam, right-click it and choose **Manage > Browse local files**.
3. Find the folder containing `Ace7Game.exe` and extract the ZIP there.
4. Launch the game normally. DLSS starts automatically once the renderer is ready.

The folder should contain:

```text
ACE COMBAT 7/
├── Ace7Game.exe
├── dinput8.dll
├── rescaleframe_overlay.dll
├── ReScaleFrame.ini
└── ReScaleFrame/
    ├── README.md
    ├── manifest.json
    ├── licenses/
    └── streamline/
```

Keep the original `Ace7Game.exe`. No game executable replacement or Windows Steam launch options
are needed. If another mod already provides `dinput8.dll`, do not overwrite it: this release does
not chain a second DirectInput proxy.

## Controls and presets

Press **Insert** to open or close the overlay.

**Enable DLSS** switches the feature on or off. Turning it off restores native scene rendering.
Choose a preset in the same panel:

| Preset | Behaviour |
| --- | --- |
| Native | DLAA at your output resolution |
| Quality | DLSS upscaling with the highest input resolution of the upscale presets |
| Balanced | A lower input resolution than Quality |
| Performance | A lower input resolution than Balanced; the first-launch default |
| Ultra Performance | The lowest input resolution |

The runtime queries DLSS for each preset's render size. The briefing layer stays at **100% output
resolution** whichever preset you choose.

Your choices are saved in `%LOCALAPPDATA%\ReScaleFrame\AC7.ini`. The `ReScaleFrame.ini` beside
the game contains initial defaults and advanced settings; ordinary use does not require editing it.

## Troubleshooting

**The overlay does not open:** check that both mod DLLs are beside `Ace7Game.exe`, then press Insert.
Check for another mod using `dinput8.dll`.

**Windows reports a missing MSVCP140 or VCRUNTIME140 DLL:** install or repair the x64 Visual C++
runtime linked above.

**Something looks wrong:** note your preset and resolution, then
[open an issue](https://github.com/KillerPixelCrew/ReScaleFrame/issues) with your GPU, driver version
and a relevant excerpt from `%LOCALAPPDATA%\ReScaleFrame\AC7\rsf-dump.log`.

## Update or uninstall

Close the game before replacing the mod's files. Saved preferences survive an update.

To uninstall, remove `dinput8.dll`, `rescaleframe_overlay.dll`, `ReScaleFrame.ini` and the
`ReScaleFrame` folder installed from the ZIP. Restore any files you backed up. You can also remove
`%LOCALAPPDATA%\ReScaleFrame\AC7.ini` to reset the saved settings.

## Building and contributing

The repository contains the loader, runtime, game support, SDK and overlay. The AC7 release uses
the proxy entry point; the standalone launcher, public plugin lifecycle and WSGM integration are
still being built.

The reference build uses Windows, Visual Studio C++ tools, CMake and the Rust toolchain pinned in
`rust-toolchain.toml`. With Visual Studio 2026:

```powershell
./eng/verify.ps1 -VS2026 -Configuration Release
cargo test --workspace --locked
cargo build --release --locked -p rescaleframe-overlay
```

Omit `-VS2026` for Visual Studio 2022. The verification gate builds and tests the native code,
checks Rust formatting and runs Clippy. It does not launch or modify the game.

- [Build tools and local dependencies](docs/tooling.md)
- [Vendor SDKs and release packaging](docs/dependencies.md)
- [Implementation tracker](docs/implementation.md)
- [Architecture](docs/design.md)
- [AC7 upscaling research and accepted result](docs/research/ac7-consumer-session.md)
- [Proxy settings and diagnostics](loader/README.md)
- [Agent instructions](AGENTS.md)

Research records distinguish source inspection, synthetic tests and actual game results. Game
binaries, licensed engine source and raw captures stay outside Git.

## License and credits

First-party code and documentation are [GPL-3.0-only](LICENSE), except the
[MIT-licensed Game SDK](sdk/game/LICENSE).

NVIDIA supplies DLSS through Streamline. The release includes unmodified NVIDIA runtime files
under their own accompanying terms, alongside notices for egui, MinHook and other dependencies.
Those third-party components are not relicensed under the project's GPL license.

ReScaleFrame is an independent mod, not an NVIDIA or Bandai Namco product.
