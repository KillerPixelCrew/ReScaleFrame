# ReScaleFrame {version}

Windows x64 packages from commit `{commit}`. Choose the download for your game. Each game ZIP includes its required
RSF files and vendor notices. The Minecraft mod keeps its own version, {minecraft_version}. Back up an older
installation before replacing RSF files. Earlier acceptance does not mean this build has been played.

## Ace Combat 7

SR, frame generation, sharpening and latency controls share the Insert overlay. Unsupported requests are refused.

1. Close the game and back up existing RSF files. Resolve any other mod that owns `dinput8.dll`.
2. Extract the AC7 ZIP beside `Ace7Game.exe`.
3. Launch normally and press Insert to choose your settings.

Requirements: the exact supported Steam executable, Windows x64, the x64 Visual C++ v14 runtime and a supported GPU.
See the included README for build and provider limits. DLSS requires supported NVIDIA hardware.

**Played on this build: not yet played.**

## Red Dead Redemption 2

SR and frame-generation providers, sharpening and latency controls are available in the shared Insert overlay.

1. Close the game, back up older RSF files and install a compatible ASI loader if needed.
2. Extract the RDR2 ZIP beside `RDR2.exe`.
3. Launch normally; RSF selects DX12. Press Insert for settings.

Requirements: supported Steam RDR2 1.0.1491.50, Windows x64, Visual C++ v14 and a D3D12 GPU. ScriptHookRDR2 is needed
for the tested live quality and overlay input-blocking paths. Read the included README before installing.

**Played on this build: not yet played.**

## S.T.A.L.K.E.R. Anomaly

The DX11 and DX11 AVX builds have SR, motion vectors, frame generation, sharpening and Insert overlay controls.

1. Close the game and back up existing RSF files in Anomaly's `bin` folder.
2. Extract the Anomaly ZIP into `bin`, resolving any other `dinput8.dll` mod first.
3. Select DirectX 11 in Anomaly Launcher, launch normally and press Insert. AVX is supported.

Requirements: supported Anomaly 1.5.3 executable, Windows x64 and Visual C++ v14. The recorded presentation path is
windowed/borderless SDR on NVIDIA. Other GPUs and HDR require separate acceptance; see the included README.

**Played on this build: not yet played.**

## Drag'n Wash

The Unity adapter offers SR, frame-generation, sharpening and latency controls in the shared overlay.

1. Close the game and back up older RSF files. Resolve any other mod using `version.dll`.
2. Extract the DragNWash ZIP beside `DragNWash.exe`.
3. Launch through Steam using the default DX12 renderer and press Insert.

Requirements: Steam build 25286774, Unity 6000.3.14f1 Mono/URP, Windows x64, Visual C++ v14 and a supported D3D12 GPU.
Provider and handheld limits are described in the included README. No game or Unity engine assemblies are bundled.

**Played on this build: not yet played.**

## Minecraft / ReScaleFrameCraft {minecraft_version}

The mod provides temporal rendering and native SR/FG controls through its settings. Its version is independent of RSF.

1. Close Minecraft and back up the instance and any older ReScaleFrameCraft JAR.
2. Put `rescaleframecraft-{minecraft_version}.jar` in the instance's `mods` folder; remove the older copy.
3. Launch the matching NeoForge instance and open the ReScaleFrameCraft settings.

Requirements: Minecraft 1.21.1, NeoForge 21.1, Java 21 and Windows x64 for the bundled native pipeline. Check the mod's
dependency requirements and the selected provider's GPU support. Controller input requires a compatible controller mod.

**Played on this build: not yet played.**

## Downloads and source

Use `SHA256SUMS.txt` to check the downloads. `ReScaleFrame-{version}-source.zip` contains the corresponding committed
source and incorporated pinned MIT source. Component notices remain inside each game download. Project Wingman is
observer-only and is not part of this shipping set. Counters do not establish lower physical input-to-display latency.
