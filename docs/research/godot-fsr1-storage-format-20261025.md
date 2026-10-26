# Godot FSR1 storage format checkpoint

The new aggregate Vulkan harness tested the advertised FSR1 selector rather than
inferring support from configuration. Its first standard-editor case saved valid
player PNGs and passed scalar checks, but strict validation rejected it: RCAS
used an `Rgba16f` storage-image declaration with an `R8G8B8A8_UNORM` image view.
The retained failed report is
`.local/godot/matrix-20261025-211312-790/index.html`; its fixture is
`.local/godot/validation/standard/sr-20261025-211313-974`.

Pinned Godot source is `ed1daf0bf001b61586d9930840f2f1394092c079`.
`servers/rendering/renderer_rd/shaders/effects/fsr_upscale.glsl` declares one
RGBA16F output for both passes. `effects/fsr.cpp::FSR::process` first writes EASU
to a base-format intermediate, then writes RCAS to the viewport destination.
Those allocations can have different storage formats. A healthy status or a
visible cube does not make the mismatched shader access defined.

The checked patch in `eng/patch-godot.py` adds RGBA8 variants alongside RGBA16F,
retains the hardware's normal/fallback arithmetic choice, and selects each pass's
pipeline and uniform-set shader using its actual output allocation format.
Unsupported storage formats are refused explicitly. Temporal inputs, FSR2,
resource ownership and the FSR1 reconstruction equations are unchanged.
The source patch applies only at exact expected anchors in the pinned checkout.

Both editors build:

- Standard SHA-256: `1D8A3672E1B97945DD2A600CB427D87CC6FDE924D95F854BE880B5744387B2FB`.
- .NET SHA-256: `069363A63087F5BE228DB4B37A5CDE93A04DCF8DA8086B7A27249366B44A251A`.
- Build logs: `.local/godot/build-standard-editor-fsr1-formats.log` and
  `.local/godot/build-mono-editor-fsr1-formats.log`.
- The normal native gate separately passes 75 discovered tests, 64 executed and
  11 opt-in skips, plus formatting and Clippy
  (`.local/godot/verify-godot-wrapup-20261025.log`). It does not execute this engine
  shader or establish the fix's GPU acceptance.

The user requested an end-of-day stop before the GPU retake. The format
correction, expanded Off/FSR1/Auto fixture and aggregate runner remain pending
their passing runtime checks; Debug/Release templates also need rebuilding.
The failed secondary-latency experiment was reverted before these builds.
No broad FSR1 image-quality or DLSS lifecycle claim follows from them.
