# Distant Horizons integration

The development adapter targets Distant Horizons 3.3.3 for Minecraft 1.21.1, with the existing
Iris 1.8.14-beta.1, Colorwheel 1.3.0 and Complementary/Euphoria shader profile.

## Matched source and package

- [Official Modrinth version 9w34y8ai](https://modrinth.com/mod/distanthorizons/version/9w34y8ai).
  Installed JAR SHA256: `864e70def5b0d54d619b940b66bb2784e91d3f3fc253064fdc1b3fc5786cbfab`.
  The download's SHA512 matches Modrinth's published digest.
- [Official source tag 3.3.3](https://gitlab.com/distant-horizons-team/distant-horizons/-/tree/3.3.3),
  commit `a54dd3eb1df0078e63a71266d042c10d9cda4493`, core submodule
  `b02c66d778beea11a931291815d652cd8abaa7ad`.
- Installed javap bytecode confirms the LodRenderer anti-aliasing configuration read, the
  GlDhTerrainShaderProgram jitter gate, and their bounding field references. The hooks change
  effective values only during RFC capture; saved DH settings remain intact.
- Iris source is pinned in [MOTION.md](MOTION.md). Its reported DH 3.x initialization failure is
  [Iris issue 3346](https://github.com/IrisShaders/Iris/issues/3346). The installed version contains
  the same unguarded delayed-config access.

## Rendering contracts

Iris can create its world pipeline before DH publishes its delayed API. The adapter defers that
DH construction using Iris's own shaderless instance, then rebuilds a still-deferred pipeline
once the API is ready. This is an initialization transition, not permanent feature disablement.
DH's copied depth texture also tracks the actual world-target dimensions, rather than relying
solely on version counters local to different RenderTarget objects.

Iris builds a new DH projection in its LOD callbacks, discarding RFC's raster jitter. The adapter
adds the current scene jitter to both the draw projection and shader-pack DH projection uniforms,
excluding shadow passes. Without a shader pack, DH inherits the current Minecraft projection;
the adapter captures its actual matrix and suppresses its additional TAA/jitter during RFC capture.

Minecraft and DH have different near/far clip planes. Converting all geometry to the original
Minecraft far plane clips the distant guide data. Extending Minecraft's rendering projection
was considered and rejected because shader-pack depth/fog contracts must stay unchanged.

The final implementation retains the original render camera and a second temporal camera with
an extended depth domain. They share view, position, frame/reset, FOV and jitter. Geometry replay
and visibility tests finish in the original domain. One GPU pass then maps their depth and DH's
same-frame depth into the extended guide domain, selecting the closest visible surface and
preserving existing object vectors. TAA/native reconstruction receive that guide camera with
matching depth/motion. Color rendering, native-resolution HUD/hand depth and shader-pack camera
uniforms retain their original projection. The extra pass and camera are required by these two
depth domains; no CPU texture readback is used.

Clear depth 1 is distinguished from distant geometry below 1. The previous 0.999999 sky threshold
discarded camera translation for far surfaces. The deep-depth GPU regression separately requires
nonzero parallax and sub-millipixel agreement, allowing normal CPU/GPU float cancellation error.
No independent motion is invented for shader-deformed DH water or other animated LOD materials.

## Current evidence

The build and 32 Java/GPU checks pass. New checks cover off-axis projection preservation, near
and far plane conversion, a near occluder over distant geometry, and far camera translation.
Existing foliage, cloud, sky, frame-token/JNI and Flywheel checks still pass. The development
JAR and DH are installed in N1GHT with previous JARs backed up. Iris has bound DH's callbacks in
the real modpack without the reported initialization failure. World LOD rendering, seams,
shader/provider transitions, resizing and visible temporal stability remain live acceptance.
The general missing-motion goal remains active, including Iris hands and shader deformation.
