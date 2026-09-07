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
- At 50% scale, the scene and HUD composite run at render resolution before final scaling. The interface itself is rasterized at a fixed 1920x1080 through `Nimbus.WidgetToTextureConverter` and, on the briefing and hangar, drawn as world-space widget quads into AC7's own render-resolution `R8G8B8A8` layer with the scene's depth bound. It is extracted into a mod-owned layer rather than promoted; see [AC7 UI composition](../../docs/research/ac7-ui-composition.md) and [extraction](../../docs/research/ac7-ui-extraction.md).
- The separate translucency layer renders at native on heavy frames (the briefing relief) through a rewritable immediate and four depth-gate patches; game-tested 7 September.
- The game keeps its own per-context screen percentage table (`FGraphicsSettingsManager`), which is why it overwrites `r.ScreenPercentage` on transitions; the plan applies the render scale there.
- Live DLSS evaluation and the F7 debug display have recorded mission runs. Full integration and controlled motion validation remain open.

Move AC7 addresses and preparation out of `loader/proxy` as the plugin lifecycle is built; the [representation plan](../../docs/representation-plan.md) gives this directory the draw classifier, the frame tail, the screen policy, the graphics settings and the patch table, behind game SDK ABI 2. Revalidate pass/view identity across missions, weather, menus, and camera changes. See [the hook map](../../docs/research/ue418-hook-map.md) and [todo](../../docs/implementation.md).
