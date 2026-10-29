# ReScaleFrame for Red Dead Redemption 2

This package replaces RDR2's upscaler and frame generation with ReScaleFrame's shared runtime.
It includes the RSF ASI, runtime, game plugin, Insert overlay and pinned DLSS/DLSS-G/Reflex,
FidelityFX SR/FG and XeSS SR/FG/XeLL runtimes. No original game files are included or replaced.

## Requirements

- Steam `RDR2.exe` 1.0.1491.50 (build 13773296). Other builds are refused.
- Windows 10/11 x64, the x64 Visual C++ v14 runtime and a D3D12 GPU.
- An ASI loader. The tested setup is
  [ScriptHookRDR2](https://dev-c.enbdev.com/rdr2/scripthookrdr2/) 1.0.1491.17 with its
  `dinput8.dll` ASI loader. Ultimate ASI Loader also loads ASI files. ScriptHookRDR2 is also
  needed for live quality-preset changes and for blocking game input while the overlay is open.
- DLSS needs NVIDIA RTX hardware. Non-NVIDIA hardware is untested.

## Install

1. Close the game. In Steam, open **Red Dead Redemption 2 > Manage > Browse local files**.
2. Install ScriptHookRDR2 (or another ASI loader) there if it is not already present.
3. Extract `ReScaleFrame.RDR2.asi`, `ReScaleFrame.ini` and the `ReScaleFrame` folder beside
   `RDR2.exe`. Back up the files of an older ReScaleFrame installation first.
4. Launch normally. RSF forces the DX12 renderer.
5. Press **Insert** for the overlay to choose SR and FG providers, quality, DLSS preset,
   generated frames, Reflex/latency and the frame limit.

Fresh defaults use DLSS SR with FG Off. On AMD or Intel hardware choose FSR or XeSS in the
overlay. Overlay changes are saved to `ReScaleFrame.ini` beside the ASI. `[RDR2] Enable=0`
disables RSF without uninstalling it.

## Logs and uninstall

Logs: `ReScaleFrame/rdr2.log` and `asiloader.log`. Include them, the chosen providers, quality,
resolution and a short recording with bug reports. `TraceD3D12=1` adds a D3D12 error log.

To uninstall, close the game and remove `ReScaleFrame.RDR2.asi`, `ReScaleFrame.ini` and the
`ReScaleFrame` folder. Licenses are in `ReScaleFrame/licenses/` and beside the vendor runtimes.

## Known limits

HUD-less FG guidance is established for normal SDR gameplay; other HUD modes, HDR, and
non-NVIDIA hardware are unverified. Native graphics-menu rows are not replaced; use the
overlay. Presented counters do not measure input latency.
