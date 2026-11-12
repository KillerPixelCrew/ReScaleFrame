# ReScaleFrame for S.T.A.L.K.E.R. Anomaly

This package adds super resolution, motion vectors, frame generation and the Insert overlay to Anomaly 1.5.3's DX11
renderer. It includes the thin DirectInput carrier, shared Runtime, Anomaly adapter, overlay, generated motion shader
sources and the pinned DLSS/Reflex, FidelityFX and XeSS runtimes. First-party code and third-party components retain
separate licenses and notices in `ReScaleFrame/licenses` and beside the vendor DLLs.

## Requirements

- Anomaly 1.5.3, `AnomalyDX11.exe` or `AnomalyDX11AVX.exe`. Other executable builds are refused.
- Windows x64, the x64 Visual C++ v14 runtime and the game's normal DX11 dependencies.
- Tested on an RTX 4070 Laptop GPU in SDR. AMD/Intel GPUs, HDR and exclusive fullscreen are untested.

## Install

1. Close the game. Back up any existing `bin/dinput8.dll`, `bin/ReScaleFrame.ini` and `bin/ReScaleFrame` folder.
2. Extract the game ZIP into Anomaly's **bin** folder. The result must be `bin/dinput8.dll`, `bin/ReScaleFrame.ini`
   and `bin/ReScaleFrame/`, with no extra enclosing directory. Keep an existing DirectInput mod loader backup;
   this package's carrier occupies that filename and does not promise chaining with other loaders.
3. In Anomaly Launcher select **DirectX 11**. The AVX performance executable is supported; select AVX if desired.
   Use windowed/borderless SDR for the tested presentation path. On a hybrid laptop, select the NVIDIA GPU for the
   executable in Windows/NVIDIA graphics settings.
4. Launch normally and press **Insert** to open the overlay. Choose SR provider, quality, sharpening and FG there.
   First use can compile and cache verified motion shader variants.

Defaults: SR on with DLSS Quality, motion vectors on, subpixel jitter on, sun shadow margin 0.0005, FG off, FG bridge
available for live switching, and captures/diagnostics off. Choose FSR or XeSS in the overlay if DLSS is unavailable;
those alternative GPU vendors have not been tested. Overlay changes are saved to `ReScaleFrame.ini` in bin.
Ultra Performance is available, but its image quality depends on the scene and output resolution. Scope behavior needs
more testing. The package does not imply broad adapter readiness or measured lower input latency.

## Logs and uninstall

`bin/ReScaleFrame.Loader.log` records carrier startup and refusals. `bin/ReScaleFrame.Anomaly.log` records profile
selection, hook installation, SR/FG status and the sun cascade/depth-margin counters. Include those logs, the executable
name, GPU, provider/quality, resolution and a short recording with a bug report.

To disable startup, set `[Loader] AutoStart=0` while the game is closed. To uninstall, close the game, remove this
package's `dinput8.dll`, `ReScaleFrame.ini` and `ReScaleFrame` folder, then restore any backed-up files. The folder's
`anomaly-motion/cache` is generated locally and can be removed with the package. The source ZIP is for building and
inspection; it is not extracted into the game.
