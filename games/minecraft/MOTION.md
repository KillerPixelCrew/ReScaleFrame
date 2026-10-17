# GPU motion producers, 18 October 2026

Create's Flywheel draws bypass the CPU vertex consumers used by the existing geometry history.
Camera reprojection cannot recover moving machinery or shader deformation. Colorwheel has its
own instancers, draw lists and shader entry points, so Flywheel hooks alone do not cover it.

## Matched inputs

- Create 6.0.10 embeds Flywheel 1.0.6. The extracted Flywheel jar SHA256 is
  `31dda15c205eb596d3b3449ef03f6af7363a6cd35b3da4bfe916b304f9e5337e`.
  The [official 1.0.6 source artifact](https://maven.createmod.net/dev/engine-room/flywheel/flywheel-neoforge-1.21.1/1.0.6/flywheel-neoforge-1.21.1-1.0.6-sources.jar)
  and shipped GLSL establish the private draw and page contracts. REA artifact evidence:
  `ev_652254f90d66865c0b28cb36970ec637eb5521bf456b90260a1471a8f3fc90a7`.
- Colorwheel 1.3.0 for NeoForge 1.21.1, SHA256
  `3c7c1f91b1add47053f40a58375de492b87ce83309be378527f100cb842c07c0`.
  [Source revision 497b820](https://github.com/djefrey/Colorwheel/tree/497b82075e2626d880b9a382da9011f7f71905cf)
  and installed javap signatures establish its separate instanced/indirect implementations.
- Iris 1.8.14-beta.1 for NeoForge 1.21.1, SHA256
  `60d5f8bf52f25e9986440f4a4270a6bd986d9cff994510e1108ac1547063b314`.
  [1.21.1 source revision eb7afb9](https://github.com/IrisShaders/Iris/tree/eb7afb99f747cc8ed5ee4072119539035d33cefd)
  corroborates the installed API. This source commit is not asserted to be the exact beta build.
  REA artifact evidence: `ev_e877f2d8d7227c2594f24ee71da4d3c0ed4312592b5a4bf7729fb0cf2e229410`.
- Selected shader test target: Complementary Unbound r5.9.3. Reference clones, jars and shader
  pack remain outside Git. No upstream implementation code is copied into this producer.

## Producer and lifetime

At native instanced or indirect draw submission, the ordinary color draw executes unchanged.
A motion-only linked variant replays its complete vertex and fragment entry points, uniforms,
UBO/SSBO bindings and material discard into a separate framebuffer. Fragment MRT writes become
private globals, preserving Iris/Colorwheel color attachments. Shadow passes are excluded.

The vertex variant records final GPU-deformed clip positions after the original main returns.
Each vertex retains unjittered clip position, UV, packed color and frame tag in 32 bytes. Two
GPU buffers alternate per group. A storage/texture-fetch barrier orders writes before next-frame
reads; no vertex readback or CPU wait is required. Output motion is previous minus current,
in render-resolution pixels, with camera motion included and jitter excluded. Reactive weight
marks missing history, changed UV/color and transparency. Depth is normalized to the existing
forward-depth guide convention and merged against source-matched hardware depth precision.

An identity table maps stable Instance and Model identities to previous vertex ranges, then maps
current physical instance slots to those ranges. Compaction, page movement, replacement, missed
frames, model changes, camera resets and topology changes cannot reuse another object's history.
Mesh prefixes distinguish a model's submeshes, including crumbling draws. Group history retires
after a frame without submission; program deletion and renderer shutdown release GPU objects.

An earlier design evaluated old instance values with previous uniforms. It was replaced because
Colorwheel can deform vertices after Flywheel's instance transform. Capturing final GPU vertices
retains that deformation directly. The first GPU fixture found an Intel error when binding built-in
vertex attributes; only actual nonnegative user attribute locations are now copied.

## Iris composition

Iris renders opaque/translucent hands inside its world pipeline. The chosen policy preserves
those shader-pack effects and resolves after Iris's color conversion, before the 2D HUD. The
unused vanilla hand depth clear is suppressed for that frame. World target resizing keeps the
same RenderTarget object so Iris's per-object attachment version counters remain meaningful.
RFC applies jitter before Iris captures gbuffer projection. While RFC owns temporal processing,
Complementary's effective TAA_DEFINE is zero; declared boolean TAA switches are likewise disabled.
Stored shader preferences remain intact and runtime ownership changes reload the pack.

## Validation and open coverage

The Java build and 26 checks pass, including an invisible real OpenGL 4.3 test on Intel UHD 770.
It verifies instance translation plus subsequent shader deformation, changing jitter, cutout
discard and GL error state. Identity tests cover physical slot migration/reuse, missed frames,
resets and model/topology changes. This is synthetic GPU evidence, not modpack visual acceptance.

Live mixin binding, Complementary shader compilation, moving Create machines, runtime pack/provider
switches and resizing still require the modpack run. General Iris waving terrain/water, independent
hand motion and shader-deformed entities/particles need further producers. Geometry/tessellation
shader pipelines are not implemented by this vertex/fragment replay. No claim of full shader-pack
motion coverage is made.
