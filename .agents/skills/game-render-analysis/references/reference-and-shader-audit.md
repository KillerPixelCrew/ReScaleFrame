# Complete reference and shader audits

Use this procedure for a new integration, a requested full inventory/audit, or evidence that
contradicts an integration's reference assumptions. A routine fix can reuse a valid recorded
audit; it need not repeat unrelated analysis.

## Reference inventory

Record the repository URL, revision, fetch date, local changes, submodule pins and consumed
shared manifests. Check the actual source used by the build, including generated shaders and
external code. A dependency that is not inspected stays explicitly unreviewed.

Account for every subsystem and feature in a coverage table. Include:

- Entry, version/build admission, hooks, native state preparation and shader replacement.
- Resource allocation and proxy targets, formats/views, active versus allocated extents,
  viewport/scissor/clear state, copies, resource pools and synchronization.
- Camera/projection, depth partitions, motion/history, jitter, mip bias and reset producers.
- Reconstruction and publication, post-processing, auxiliary surfaces, HDR/tonemap, HUD/UI,
  offscreen views, presentation, frame generation and latency/input boundaries.
- Configuration/UI, runtime switching, startup, resize, teardown and failed-frame recovery.
- Compatibility restrictions, feature dependencies, build/package inputs, tests and the
  runtime behavior those tests do not cover.

Give file/symbol/line anchors and explain when each mechanism executes and what owns its
state. A file/hash catalog supports completeness but does not explain behavior. Distinguish
first-party, adapted, shared, generated, third-party and unused material.

Compare reference mechanisms with RSF in a separate column: implemented, deliberately
different with evidence, missing, or unverified. Follow the complete producer-to-Present order.
Do not port a convenient wrapper while overlooking the engine/shader changes that make it
work. A reference's unsupported ENB/mod policy does not remove RSF's requested compatibility
requirement. Independent inventories, when requested, must be reconciled rather than counted
as agreement from their titles alone.

## Shader and buffer coverage

Inventory shipped bytecode by hash, family, stage and permutation. Join reconstructed or
reference HLSL only when identity or native instructions support that correspondence. Include
VS, HS, DS, GS, PS and CS where present; unmapped stages remain gaps rather than adjacent-family
guesses. Record the shader packages searched and excluded sources/mods.

For every participating pass/material class, record:

| Fact | Required evidence |
| --- | --- |
| Output and coverage | Actual RTV/DSV/UAV identity, blend/write masks, clear/unwritten state and render rectangle |
| Input contract | Bound CB/SRV/UAV/vertex/index/instance streams, slots, ranges, formats and units |
| Motion production | Current/previous matrices, origins, instances, bones and non-skeletal deformation; encoding, jitter and camera inclusion |
| History ownership | Allocation, update order, lifetime, pool reuse, cuts, transitions and reset behavior |
| Consumers | SR/FG input seam and later bloom, reflections, DoF, water/refraction, particles, volumes, UI and auxiliary reads |
| Validation | Source, native disassembly, compiled test, device capture and moving-game results kept distinct |

Cover static and instanced objects, skinning, tessellation, terrain/LOD, generated or
compute-deformed meshes, alpha-tested/blended/additive surfaces, particles, water and
volumetrics. A shader declaring a motion output does not prove a bound target or written
pixel. Valid zero is different from missing motion; camera reprojection does not reconstruct
independent object or deformation history.

For each gap, establish whether the native producer is missing, its shader variant suppresses
the output, its buffer/history is stale, a binding is wrong, or the consumer misinterprets
correct data. Repair the verified owner, including necessary shader variants and buffers.
Preserve game policy in the adapter and reusable GPU/vendor mechanisms in generic services.
Do not replace missing data with fabricated vectors or current-as-previous state to make
admission succeed. An explicit supported native path/refusal can document an out-of-scope
case; it cannot silently remove a required feature from the user's goal.

## Acceptance

Retain rejected images, failed tests, tool failures and contradictory evidence. Locate the
first bad same-source product before assigning cause. Neither reference code, bytecode counts,
an SDK success result nor a synthetic test proves complete runtime coverage.

Validate settled native and every required reduced quality, camera motion, independent object
and deformation motion, history resets, UI/offscreen views, quality/backend switches, resize,
failed reconstruction and teardown. Reduced commitment must have proven recovery and valid
full-output consumers; returning to an untouched native path is not that proof. Distinguish
spatial recovery from successful temporal SR and device/provider telemetry from scanout or
latency claims.

Update research, engine metadata and the implementation tracker with each unresolved required
input, binding, repair and validation case. Do not mark the integration complete until these
requirements are satisfied at the scope requested by the user.
