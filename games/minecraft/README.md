
# ReScaleFrameCraft

Minecraft 1.21.1 / NeoForge 21.1.262 / Sodium 0.8.13.

Native DLSS, FSR2/3/4 and XeSS reconstruction and independent DLSS/FSR/XeSS frame generation
use ReScaleFrame's shared runtime. Video Settings exposes a ReScaleFrame page through Reese's
Sodium Options. Provider, quality, frame count and latency settings apply without restarting.
Actual availability depends on the GPU and SDK; F3 shows requested and effective providers.
[Native implementation, build steps and validation limits](NATIVE.md).

VulkanStory-Rewrite's eight-sample raw-history TAA, including its homogeneous
sky reprojection fix. The HUD renders after the resolve. Vanilla hands remain native resolution;
with Iris shader packs, hands keep Iris's world shading and effects inside the reconstructed scene.
F8 opens settings; TAA can be toggled and history reset while playing. F3
reports the actual status. Settings persist in `config/rescaleframecraft.json`.

The retained resolve texture contains the HUD-less scene. `TemporalRenderer.sceneFrame()`
publishes that owned texture with its frame ID, dimensions and origin convention;
it is valid until the next world frame starts. HUD and menus are composited
onto Minecraft's main target afterwards and cannot modify the retained texture.

Eight raw scene/depth/motion snapshots remain unchanged in a ring. Reprojection
transports coordinates while retaining the selected surface through older
frames, then samples each original colour once. Repeatedly filtering colour
between cache ages spreads thin foliage into a trail, so this port avoids that
feedback. Available raw samples are averaged; resolved output never enters history.

Camera reprojection covers static terrain and celestial sky. Previous CPU
vertices supply entity, block-entity and particle motion; vanilla and Sodium
clouds use their actual cached mesh transforms, and rain/snow use their submitted
vertices. Re-uploading a cloud mesh invalidates its previous geometry even when
Sodium reuses the same vertex buffer. Replay matches the source depth precision.
Flywheel/Create and Colorwheel instanced geometry use final GPU vertex history,
including material cutout and deformation after the instance transform. Iris's
effective TAA options are coordinated with RFC temporal ownership while preserving
stored pack preferences. [Producer details, current gaps and shader-pack acceptance](MOTION.md).

## Build and test

Build the native module and stage its dependencies as described in [NATIVE.md](NATIVE.md).
Then use JDK 21 and `./gradlew test jar`, or run `./eng/test-temporal.ps1` with the
Prism JDK. The harness creates an invisible OpenGL
context and runs the shipped shaders and temporal pipeline. It requires an
OpenGL driver and desktop session, opens no visible window, and does not launch
Minecraft. Results are in `build/reports/tests/test`.

Run `./eng/deploy-prism.ps1` to install in the N1GHT Prism instance. Deployment
refuses to replace a running game, backs up earlier jars, disables Metal
Upscaler and verifies the copied hash. The full modpack is the integration-test
environment; a successful harness does not prove world rendering or visual
quality.

## Upstream

`taa-upstream.json` pins the revision and source hashes.
`eng/port-taa-shaders.py` exports the native shaders with OpenGL bindings into
ignored reference files for comparison. Active shaders are `taa-accumulate.frag`
and `taa-output.frag`.
Original attribution and license are preserved under `META-INF`.
The TAA regressions remain separate from native SDK execution and live modpack acceptance.
