# ReScaleFrameCraft 1.1.9

Minecraft 1.21.1, NeoForge 21.1.262 or newer in the 21.1 line, Sodium 0.8.13.
Windows x64 with an OpenGL driver supporting external Win32 memory/semaphores and a
D3D12 device on the same GPU. Java 21 or newer is required by Minecraft.

## Install

1. Close Minecraft and remove older ReScaleFrameCraft JARs from the instance's `mods` folder.
2. Disable or remove MetalUpscaler, then place `rescaleframecraft-1.1.9.jar` in `mods`.
3. Launch normally. Native runtimes and their notices are included and extract automatically.
4. Open Video Settings and the ReScaleFrame page. Reese's Sodium Options 2.2.4 is supported.
   F8 opens the temporal settings screen; F3 includes native processing status.

Select the reconstruction backend/quality, independent frame-generation backend/count,
low-latency mode and real-frame FPS limit. Settings apply while running and persist in
`config/rescaleframecraft.json`. GPU/provider limits can reduce the effective frame count.
DLSS requires a supported NVIDIA RTX GPU. Other SDK features depend on GPU/driver support.
DLSS model presets Default, E, F, J, K, L and M are available independently of quality.
The driver can override the requested model.

## Changes since 1.1.5

- Iris and Colorwheel integration, including Complementary Unbound with Euphoria Patches.
- Fixed missing world geometry and depth-format errors with shaders enabled.
- Fixed grainy stationary clouds by preserving Complementary's temporal sampling.
- Iris hand shading and effects stay in the reconstructed scene; the 2D HUD stays separate.
- Added GPU motion history for Flywheel/Create and Colorwheel instanced geometry.
- Added DLSS model preset controls and reduced per-frame CPU allocations.

## Included

- Stable temporal antialiasing with sky, foliage and cloud history handling.
- DLSS, FSR and XeSS reconstruction through ReScaleFrame's native runtime.
- Independent DLSS, FSR and XeSS frame-generation integration.
- World reconstruction before HUD composition and a separate HUDless FG input.
- Vanilla hands render after reconstruction; shader packs retain Iris hand rendering and effects.
- Reflex/PCL input integration, including optional Controlify and Controllable adapters.
- Compatibility with Ixeris's separate event/render threads and window resizing.
- Native vendor runtimes with their accompanying license notices.

## Known limitations

Shader-pack behavior varies. Dedicated motion coverage remains incomplete for shader-deformed
terrain, water, entities and particles, and for geometry/tessellation shader pipelines.
Frame-generation availability and multipliers vary by GPU; consult the effective status in F3.
FSR4 is hardware-dependent. This is a client mod; do not install it on a dedicated server.

To uninstall, close Minecraft and remove the JAR. Extracted files under `rescaleframe/native`
and `config/rescaleframecraft.json` may also be removed after closing the game.
Include `logs/latest.log` and the active native payload's `minecraft-native.log` with bug reports.

First-party code uses GPL-3.0-only; included third-party components retain their own notices.
The separate source archive contains the corresponding committed source and build scripts.
