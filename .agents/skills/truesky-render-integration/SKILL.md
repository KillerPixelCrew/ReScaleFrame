---
name: truesky-render-integration
description: Investigate and repair TrueSky cloud rendering by tracing projection, cloud resolution, depth bounds, effect permutations, resource views and history owners. Use for cloud jitter/blockiness, missing volumetrics and sky bleeding through aircraft details, including AC7's mixed-resolution path.
license: GPL-3.0-only
---

# TrueSky render integration

Read [AGENTS.md](../../../AGENTS.md), relevant
[render analysis](../game-render-analysis/SKILL.md) and
[the AC7 depth reference](references/ac7-depth.md). TrueSky versions differ; these sites and
the x1 kernel are specific to the fingerprinted AC7 DLL/effect. Preserve the user's testing scope.

## Trace the complete cloud producer

Start with `references/TrueSkyPlugin`, `references/TrueSkyPluginSource`, the game integration
and installed DLL/effect. Record provenance, hashes and version. These reference folders need
not be Git checkouts; do not invent a revision when only file hashes exist. Connect source
concepts to Ghidra instructions and native resource ownership.

Establish projection/jitter transformation, downscale policy, near/far colour, scene-depth bounds,
temporal history and tile allocations together. Inspect raw depth and actual view types, not
only colour previews or shader declarations. Trace engine view -> plugin parameters -> native
allocator/depth pass -> cloud colour -> native scene composition.

For stationary holes on thin aircraft details across upscalers, compare opaque depth and cloud
masking inputs before attributing them to missing vertices, reflections or motion vectors.
For blocks below Balanced, measure cloud grid relative to output and active scene dimensions.
Balanced is the user's observed baseline, not a universal cloud-resolution floor.

## Make resolution, permutation and precision agree

Changing the divisor alone is insufficient. AC7's effect has x2/x3/x4 depth permutations while
allocation arithmetic accepts x1. Apply/Dispatch/Unapply can continue without selecting an x1
shader and destroy cloud/lighting output. Preserve balanced cleanup when adding a first-party
x1 kernel at the verified native call.

Prepare the shader on the actual device, validate native CB/SRV/UAV bindings and formats, then
activate the full grid. Compilation does not prove guard acceptance or dispatch execution.
AC7 creates a Texture2D UAV despite the old shader's array declaration. Log verified dimensions,
view types and actual dispatch with bounded messages.

Preserve units, radial conversion, saturation, sky sentinel and offsets/padding. Normalized
two-channel UNORM can lose near-aircraft precision before the cloud consumer samples it.
Change the native allocation format through its owner so texture/SRV/UAV rebuild together;
do not attach incompatible views or globally change every two-channel texture.

Keep cloud colour, depth, history and tile sizes coordinated at production. The accepted AC7
fix uses scene-resolution clouds, separate from opaque render scale, without post-SR cloud
recomposition. Relocation would require evidence for atmosphere, lighting, depth, history,
bloom and tone-mapping order first.

## Patch and retire safely

Require DLL/effect fingerprints and expected instruction bytes. Announce refusal and retain
the supported path until readiness is proven. Rebuild shaders on device recreation. If bindings
fail after full-grid activation, do not dispatch native x2 onto x1 targets: restore the supported
divisor for the next allocation, suppress the incompatible update, balance cleanup and report it.

Quiesce callbacks before restoring format/divisor/pass/call sites or releasing resources. Failed
restoration retains reachable code/resources. Current source is authoritative for exact guards;
this skill's addresses are research anchors, not unconditional patch recipes.

## Validate result and cause

Compare cloudy aircraft overlap at native/off, Balanced, Performance and Ultra after history
settles. Include stationary aircraft, moving camera, thin flaps/wingtips, distant clouds, cuts
and resource transitions. Confirm volumetrics and lighting still exist when aircraft looks clean.
Pair F9 raw data/metadata and ready/activation/dispatch logs with user-run visuals.

Captured-depth GPU replay proves conversion precision and shader/view compatibility, not live
callsite activation or every visible pixel's cause. Cloud wind/history and motion vectors remain
separate from the accepted depth correction. Stability does not prove that clouds are static.
