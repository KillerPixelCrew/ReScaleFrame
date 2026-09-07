# Ace Combat 7

AC7 uses a modified UE4.18 renderer. The researched target is Windows x64/D3D11, Steam app `502500`, build `9855922`. The plugin recognizes its executable name, PE machine, and SHA-256. [engine.json](engine.json) records the evidence.

The plugin API still reports `rendering_ready = 0`: the live DLSS hooks are driven by the research proxy, outside this lifecycle. The AC7 view reader is implemented and used by that path.

## View data

`rsf_ac7_view` reads the 4096-byte view constant buffer: projection, camera basis/position, `ClipToPrevClip`, jitter, and view/buffer extents. It checks matrix and size relationships before accepting the data and identifies secondary views.

UE4.18 has no `ViewToClipNoAA` field. Read the engine's camera transform, remove projection jitter where required, and validate backend conventions separately. The [capture report](../../docs/research/ac7-frame-capture.md) contains the offsets and checks.

## Current evidence

- The shipped code section is encrypted; analysis uses a decrypted runtime dump.
- Captures show a temporal-filter candidate with colour, history, depth, velocity, and exposure. Jitter was zero before patching the AA gate.
- The gate patch at RVA `0x112b1f3` enables sub-pixel jitter. `r.ScreenPercentage` reduces actual scene-buffer sizes.
- Sparse `R16G16_UNORM` velocity needs bias removal. Camera movement is absent from unwritten pixels and can be resolved using depth and the engine transform.
- At 50% scale, the scene and HUD composite run at render resolution before final scaling. Reinsertion must also move the composite to output resolution.
- Live DLSS evaluation and the F7 debug display have recorded mission runs. Full integration and controlled motion validation remain open.

Move AC7 addresses and preparation out of `loader/proxy` as the plugin lifecycle is built. Revalidate pass/view identity across missions, weather, menus, and camera changes. See [the hook map](../../docs/research/ue418-hook-map.md) and [todo](../../docs/implementation.md).
