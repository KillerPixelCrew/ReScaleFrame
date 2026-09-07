# Validation plan

The complete target is lower-resolution AC7 XeSS-SR, XeLL, and MFG on the Claw, usable standalone and through WSGM. Native-resolution AA and SR-only are intermediate milestones.

AC7 observation, input capture, jitter/render-scale control, and diagnostic DLSS evaluation have recorded runs. That evidence does not complete the experiments below. See [the tracker](../implementation.md) and [capture report](ac7-frame-capture.md).

## 1. Loading and build recognition

Record executable path/hash, Steam build, PE architecture, renderer, adapter LUID, driver, output mode, and graphics wrappers. Unknown or ambiguous builds must stop before game-specific patches.

Trace loading through the real Steam/launcher handoff, bootstrap, plugin selection, device/factory/swap-chain creation, first buffer, and first Present. Identify game/render/RHI, input, scene, and HUD boundaries.

Pass when both standalone and Steam routes load early enough and observation preserves normal rendering. A late load or research shim alone does not complete the product launcher.

## 2. Synthetic DX11/DX12 bridge

Use a test application with known camera/object motion, depth, jitter, exposure, transparency, fixed UI, and frame-ID patterns. Establish passthrough before enabling interpolation.

| Experiment | Acceptance |
| --- | --- |
| Swap-chain facade | Correct DX11 interfaces, adapter identity, image, and HWND ownership |
| Shared resources | Fenced reuse, valid states, bounded memory, no stale frames |
| XeLL | Sleep before input, valid marker pairs, IDs survive thread handoff |
| 2× / higher MFG | Effective interpolation agrees with supported/initialized counts |
| UI | Fixed and translucent elements remain sharp and blend correctly |
| Lifecycle | Resize, minimize/restore, focus, tearing/VRR, device failure, shutdown |

Use SDK validation, Inspector where supported, and graphics debug layers. Measure performance separately without instrumentation. Test the Claw early: another GPU cannot establish XMX or Intel-only interpolation counts. Measure latency against native and bridge-only baselines.

## 3. AC7 temporal inputs

Capture stationary-camera jitter, camera-only motion, object-only motion, combined movement, and a cut. For each input, establish producer, shader/pass/view/frame identity, extent/rectangle, format/convention, and valid lifetime.

Verify motion storage, sentinel, units, signs, temporal direction, camera/jitter inclusion, and dilation. Check depth clear values, sky, exposure/pre-exposure, and reset signals. Inspect clouds, glass, rain, smoke, cockpit, targets, subtitles, and menus separately.

Compare numeric reprojection against known movement. A plausible vector preview is insufficient. Reuse engine-produced full motion only after verifying its semantics; otherwise use engine camera transforms with depth and the appropriate object-motion resolve.

## 4. True SR reinsertion

Start with native-resolution AA, then use a vendor-recommended smaller input and return an output-resolution result to post-processing. Verify allocations, viewports, scissors, constants, bindings, grading, and the final scaling route. Preserve an output-resolution HUD.

Compare original rendering, runtime present with effects disabled, native-resolution AA, reduced rendering with ordinary scaling, and reduced rendering with SR. Match output dimensions and settings; record actual GPU input/output extents. Require useful frame-time savings as well as correct images.

## 5. AC7 MFG and timing

Connect the proven bridge and test every supported count. Carry the real source-frame ID from before input through rendering to Present. A latest-global counter is insufficient when threads overlap.

Provide completed output-resolution HUD-less colour and suitable UI/final-frame data. Verify UI alpha/blend rules and include egui in the composition design.

| Scene | Main checks |
| --- | --- |
| Banking, terrain, fast flight | Camera motion, disocclusion, edge stability |
| Cockpit/chase/FOV changes | Geometry, cuts, history reset, HUD alignment |
| Clouds, smoke, glass, rain | Transparency and missing motion |
| Missiles, targets, subtitles, menus | Object motion and sharp UI |
| Loading, pause, cutscenes, mission changes | Eligibility, continuity, resource replacement |
| egui and focus changes | Input ownership, capture exclusion, recovery |

Require effective SDK interpolation, stable independent presentation evidence, and useful XeLL timing. Present-only markers do not describe simulation. If mixed-API XeLL underperforms, investigate submission/pacing before expanding the design.

## 6. Claw measurements

Use a repeatable mission route, matched settings/output/refresh/power, warm caches, repeated runs, and a thermally settled sample. Compare native, SR, SR+MFG, and bridge-only where relevant. Benchmark a controlled Vulkan/DXVK variant before changing the initial native path.

Record rendered frame-time distributions, generated/presented counts separately, cadence/drops/repeats, GPU processing/copy/synchronization costs, memory, power, thermals, visual defects, and input-to-display latency where measurable. Label CPU/SDK estimates. Choose multipliers from sustainable base frame rate and panel refresh; a larger displayed counter is not sufficient.

## 7. Frontends and development tools

Wire the existing egui DLL, D3D11 renderer, and input hook. Validate requested versus effective settings, refusal/restart reasons, keyboard/mouse/controller navigation, DPI, focus, and clean shutdown.

Build Compare, Inspect, Capture, Performance, and Status views alongside engine work. Capture bounded matching sequences with numeric buffers, constants, settings, identities, and reset state. Exercise cancellation, overflow, gaps, freeze/resume, and readback overhead before adding offline replay.

Standalone and both WSGM launch paths must select equivalent profiles/plugins while preserving Steam arguments/environment, de-elevation, Steam Input lifetime, and exits. One store owns persistence per session. Frontend loss must not block rendering.

Coordinate and restore limiter policy. Feed AutoTDP rendered timing and targets. Upscaling must remain usable with Device Integration disabled.

## Reporting

Tie results to game hash, RSF/plugin/SDK versions, GPU/driver/API, capture identity, and effective settings. Include reproduction steps and failed cases. Distinguish source-inspected, built, synthetic-tested, capture-validated, game-tested, and Claw-tested results. Keep the same diagnostic harness for resource/presentation changes.
