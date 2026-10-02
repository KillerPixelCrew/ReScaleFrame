---
name: unreal4-render-integration
description: Integrate and debug upscaling in shipped Unreal Engine 4 games by tracing view and render-target ownership, post-process dependencies, UI raster density, temporal conventions and queued RHI lifetimes. Use for reusable UE4 renderer concepts; use the UE4.18 skill for matched AC7 addresses and layouts.
license: GPL-3.0-only
---

# Unreal 4 render integration

Use this for shared UE4 renderer reasoning in ReScaleFrame. Read
[AGENTS.md](../../../AGENTS.md) and relevant [render analysis](../game-render-analysis/SKILL.md).
Determine the game's engine branch and match the implementation before using any private field.
UE4 games can carry custom renderers; concepts transfer more reliably than layouts or addresses.
For AC7/4.18 specifics, read [ue418-render-integration](../ue418-render-integration/SKILL.md).

## Find the native owner

Work backward from corrupted pixels/bindings to scene allocation, view preparation, graph nodes
and UI producers. Inspect matching source before intervention:

| Concern | Typical UE4 owners |
| --- | --- |
| Family/view size and allocation | `SceneView.cpp`, `SceneRendering.cpp`, `SceneRenderTargets.cpp`, `RenderUtils.cpp` |
| Jitter, current/previous transforms | `SceneVisibility.cpp`, `SceneView.cpp`, shader parameter producers |
| Pass dependencies/output | Composition graph/post-processing code; implementation varies across UE4 branches |
| UI layout and raster | UMG widget/converter/render-target owners, `WidgetRenderer.cpp`, `GameViewportClient.cpp`, Slate RHI |
| Velocity coverage | Material eligibility, rigid/skinned previous geometry and velocity shader permutations |

Keep detection/preparation distinct from graphics activation. The game plugin owns hooks,
private layouts and conventions; the shared runtime owns vendor contexts, interop and GPU services.
Patch verified instructions after decryption when engine policy must change. Do not substitute
global console settings for guarded producer ownership where the game resets those settings.

## Carry exact dimensions and dependencies

Separate output extent, engine-padded allocation, active render rectangle and logical UI size.
Set matching views before allocation and rebuild family sizes/derived uniforms through their
native owners. Handle allocation hysteresis without sampling padded regions. Secondary, stereo,
reflection or orthographic views require explicit support rather than a width-based guess.

Insert SR before the native tone-mapping/UI consumers and preserve a complete native fallback.
Review all graph dependencies, not input zero alone. Bloom and eye adaptation can be auxiliary
branches still reading low-resolution jittered colour. Resizing shared view state too early changes
those branches. Feeding reconstruction back into its own exposure dependency can create a cycle.
Verify native registration, pooled target descriptors and retirement for added nodes.

Resize only reviewed consumers and align their rectangles, depth choices and shader parameters.
Skip obsolete post-SR filtering only when matched to the engine path, not by globally disabling
AA or post-processing. An allocation may hold GBuffer at one phase and tonemap output later.

Linear float scene radiance is distinct from HDR display output. Preserve native lighting,
reflections, bloom, exposure and grading. Higher precision must begin at the native producer;
copying packed colour into FP16 after rendering cannot recover discarded precision.

## Respect the execution layers

Game/update, render-thread graph construction, parallel CPU command recording, RHI command
execution and GPU completion are distinct. Queue copied frame/view/history metadata with
resource leases; do not read a later CPU frame or retired object from delayed commands.

Pool/COM references preserve ownership, not immutable pixels. Pool leases must prevent reuse
until consumption, then release on the engine's owning thread. Copy/AddRef a shared uniform
before replacement; native move can clear the source. Join outstanding CPU recorders before
shared view edits, outside plugin locks, and retain generated uniforms through queued use.
Rebuild parameters with the pure native producer, not dynamic draw-resource initialization.

Quiesce new producers, drain in-flight callbacks and retire references before restoration/unload.
Keep reachable callback/trampoline code resident if restoration or retirement cannot complete.
For API-side passes, restore every touched graphics stage and inherited binding.

## Keep UI and temporal production native

Raise physical UI raster density at its owning target/converter while preserving logical layout,
native dirty/repaint behavior, depth, glow and composition. Inspect shared versus privately owned
widget targets. Extracting already-composited UI can discard processing the game applies later.
Full-resolution UI is not proof of a reusable HUD-less FG surface or alpha convention.

Enable jitter during native view preparation so matrices and derived state agree. Trace final
projection through VS/HS/DS/GS and middleware. Preserve current/previous jitter units and reset
history on cuts, owner/size changes and skipped frames as required by the actual backend contract.

Audit raw motion validity before decode. Valid zero motion is different from unwritten velocity.
Camera/depth reprojection covers camera movement, not missing independent objects. A shader
replacement cannot add geometry that the engine excludes from the pass or missing prior bones.
Study eligibility, previous geometry and material coverage before adding a sidecar/replay.

## Establish acceptance

Verify graph order, frame identity, exact dimensions, leases, fallback, resize and teardown with
focused fixtures and raw captures. Then verify moving-game output across scenes and modes.
Source matching or a synthetic graph walker cannot establish that a shipped private layout works.
Keep game acceptance, device experiments, numeric quality and measured FPS/latency separate.

Use [the session lessons](../game-render-analysis/references/session-lessons.md) when comparing
an AC7 regression. Record new evidence and keep game-specific constants out of this general skill.
