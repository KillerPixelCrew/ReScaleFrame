# AC7 temporal velocity, 13 October

## Mesh-particle correspondence

The instanced mesh-particle path now keys stored generated geometry by the complete instance
transform and vertex ID. Lookup uses the native previous transform for that particle in the
current sorted order. It does not assume that SV_InstanceID or a pooled factory address stays
fixed across frames. Unavailable previous-transform data uses camera motion at the fragment's
own depth, without selecting another particle's pose.

Native/source matching located `FMeshParticleVertexFactoryShaderParametersVS::Bind` at RVA
`0x1874330` through the `PrevTransformBuffer` and `PrevTransform0` UTF-16 references. Its vtable
at RVA `0x2dac808` identifies `SetMesh` at `0x188cfa0`, entry bytes
`48 89 5c 24 08 48 89 6c 24 10 48 89 74 24 20 57`.
Ghidra decompilation confirms the factory uniform at +`0x488`, previous-transform SRV at +`0x4a0`,
and shader resource/+`0x48` -> native vertex shader/+`0x38`. The matching 4.18.3 source is
`MeshParticleVertexFactory.cpp:50-88,269-271` and `MeshParticleVertexFactory.ush:185-205`.

The guarded producer hook queues an extra reserved-slot binding. It copies and retains the RHI
shader/SRV references; execution uses the actual RHI command-list context and its vertex SRV
method at virtual offset `0x1d0`, corroborated by the native command executor at `0x120ccd0`.
References retire through the renderer's existing owner-thread mechanism. Stop waits for pending
bindings. The game-owned source resource remains native; no CPU/GPU transform readback is needed.

The complete WARP test now reverses two mesh instances, moves them independently and replaces
their factory address. The foreground motion still follows its own previous transform. The
native graph fixture separately calls the actual producer hook, executes its queued binding and
checks exact retention/retirement. These are source, queued-ABI and synthetic GPU checks; no new
AC7 run was made. Native geometry/tessellation-stage input routes still remain unfinished.

Release verification passed 45 native tests, with seven opt-in vendor skips, plus formatting and
Clippy. A later deployment rebuild found Visual Studio 18.10.4 marked incomplete/reboot-required
by the Installer. The existing build recovered through CMake's documented local instance
specification using build number `18.10.12306.391`; no installation/global settings changed.

## Indirect arguments and sorted particle coverage

The next source increment adds indexed and nonindexed indirect replay with the exact borrowed
argument buffer and byte offset. Graphics ABI 15 preserves that draw contract; game ABI 16 carries
a copied native mesh-element vertex bound. In matched 4.18.3 `MeshBatch.h`, and AC7's previously
matched 0x60-byte element loop, NumPrimitives and NumInstances are at +0x34/+0x38. GPU-sprite
submission in `ParticleGpuSimulation.cpp:2962-2963` supplies both. The producer sums selected
elements with overflow checks rather than estimating capacity from a texture or draw order.

Arguments are copied into private GPU indirect storage and validated there against the native
bound. Both motion draws consume that same GPU snapshot; no CPU Map/readback or per-draw fence
is introduced. An oversized submission produces no partial velocity writes. Direct and indirect
history share vertex identity when their geometry corresponds.

The complete WARP producer fixture now covers indexed/nonindexed indirect calls, a nonzero
argument offset, capacity refusal, direct/indirect continuity and cuts. The actual frame-tap hook
fixture verifies the borrowed buffer/offset and distinct indirect kinds. A two-particle fixture
reverses draw order, moves the foreground particle and changes its pooled vertex-factory address;
the prior billboard still matches by particle counter and corner. These are synthetic results,
not new flight acceptance. GS/tessellation and mesh-particle instance correspondence remain open.

## Later source increment: dedicated material velocity/depth producer

The separate pass is now implemented and wired to AC7's static/dynamic translucency DrawMesh
owner, enabled with the native reconstruction configuration. ABI 15 carries copied primitive,
vertex-factory and material identity, element mask, blend mode and opacity clip value. Native
colour draws are forwarded unchanged. Their queued scope reaches the actual D3D11 geometry
callback; it does not read later CPU mesh/view state.

The pass reuses the game's exact material and vertex shader instructions in independently
validated variants. Standard material evaluation and discards remain. For additive base-pass
shaders, the guarded final scalar-times-RGB operation supplies opacity even though output alpha
is zero. The pass clips against the copied material setting plus `1/255`, writes near-or-equal
depth, and writes packed motion. Other shader shapes refuse with bounded diagnostics.

Current generated clip positions are recorded with stream output. CPU sprite keys include their
stable particle counter and corner UV, so sorting need not substitute a different particle's
history. The previous generated geometry carries billboard, vertex deformation and skeletal
pose as actually rendered. View/native-frame continuity, camera cuts and inactive-model retirement
control history. Missing or ambiguous geometry history uses the native camera transform at this
fragment's own depth. Stream-output storage and shader-readable raw storage are separate D3D11
buffers, copied on the same GPU stream; the combined allocation was rejected by WARP.

The material pass commits its motion/depth into the leased engine allocations from a queued
marker at the start of native post-processing, before its consumers execute. It leaves native
colour composition intact. Current and previous clip coordinates use matching interpolation;
deriving current motion from rasterized SV_Position introduced a measurable rounding error and
was corrected. Valid zero motion remains encoded as the native bias, never the clear sentinel.

New WARP checks exercise the complete generated-geometry/history/opacity/depth/commit path across
two moving frames and a camera cut, preserving clipped opaque motion and native shaders. All
retained candidate flight VS/PS blobs used in this investigation accept the generated shader
variants, including packed particle varyings. The current Release gate passes 45 native tests,
with seven opt-in vendor tests skipped, plus formatting and Clippy. This remains synthetic and
build evidence. No new AC7 game run was performed.

Still unfinished: native GS/tessellation and indirect geometry need their own input route; the
current triangle-list path explicitly refuses those cases. Sprite sorting/emitter identity,
mesh-particle instance correspondence and affected carrier/refuelling/vehicle/kill-cam scenes
still need broader producer validation. These are remaining implementation/coverage work, not a
claim that a successful build finished the full goal. Earlier notes below describe the first
increment and are retained as history.

The requested outcome includes missing weapons/attachments, moving vehicles, animated meshes,
kill-cam, particles and a complete translucent velocity/depth pass. This increment repairs native
opaque eligibility and prepares the missing particle-stream evidence. It does not complete that
outcome or port the translucent pass.

## Native opaque producer

The existing captures measured small-object rejection and unchanged root transforms. The native
velocity shaders already carry previous primitive transforms and skeletal buffers. Removing their
motion-blur selection restrictions retains those producers instead of inventing object motion
from depth. Source: UE4.18.3 `0a14a8d537a31ecc77488ced41dbaa0166612ef8`,
`VelocityRendering.cpp` and `PrimitiveSceneInfo.cpp`. Executable SHA-256:
`c7da97f5f8a807d4f1264adbb074146fcffe9bdc2ffa98791b822cd28e558f4f`. Decrypted dump:
`fafd1db2808d32333caecf4056e8bcd676c2f07e4c1fabc0f270ef29f22d24db`.

Capstone checked the retained dump again. Read-only Ghidra decompilation of the existing AC7
database confirmed `ShouldRenderVelocities`, `HasVelocity`'s consumers and native mesh history.
Private evidence is under `.local/ac7-velocity-20261013`. No new gameplay was run.

| RVA | Native bytes | Replacement | Behavior |
| --- | --- | --- | --- |
| `0x118a4e3` | `75 0e` | `90 90` | Consider velocity when native TAA is off; keep the camera-cut test immediately afterward |
| `0x10fdd0c` | `76 38` | `90 90` | Retain small visible movable opaque primitives |
| `0x11838c4` | `77 88` | `eb 88` | Retain unchanged-root movable primitives after a successful native history lookup |

The last change deliberately follows history lookup at `0x1183869` and missing-history rejection
at `0x1183870`. It does not force `AlwaysHasVelocity` at `0x118384c`, which would bypass history.
Visibility, mobility, main-pass/material relevance, shader support and previous-geometry production
remain native. This does not manufacture absent previous bones or turn repeated engine cuts into
valid history. Kill-cam still needs a same-build capture to distinguish pass selection from cuts
and absent geometry/history.

The group preflights all sites before writing. A failed write attempts guarded rollback. Stop
restores owned bytes after quiescence and refuses foreign replacement bytes. The native graph
fixture checks mismatch rejection, complete installation, idempotence and guarded restoration.
Capture hooks recognize the owned size-gate replacement without weakening the unowned guard.

## Full translucent pass remains required

The checked UE4.27.2 reference is `d94b38ae3446da52224bedd2568c078f828b4039`.
`FTranslucentVelocityMeshProcessor` retains even nonmoving geometry because this pass writes
depth. It uses depth writes with near-or-equal comparison. `MaterialTemplate.ush` clips
translucent/additive/modulate opacity against the material clip value plus `1/255`.
These semantics are absent from the experimental AC7 blend-gate patch, which remains opt-in.
The new opaque patch group does not admit translucent batches.

UE4.18 `BasePassPixelShader.usf:1099-1100` writes additive RGB multiplied by opacity but writes
zero output alpha. A generic replay reading output alpha cannot reproduce the material's opacity
clipping. Likewise, the sprite previous-position helper returns current geometry despite an
`OldPosition` stream. Enabling eligibility alone solves neither problem.

The native translucency DrawMesh owner was matched at RVA `0x1158250`, entry
`4c 89 4c 24 20 4c 89 44 24 18 48 89 54 24 10 48`. Its unwind chain includes
`0x11582c9..0x115840d`; the earlier scale consumer was a function fragment. Read-only Ghidra
decompilation confirms the material query through mesh+`0x98`, feature level view+`0x13e8`,
blend query at material virtual offset `0x198`, vertex factory mesh+`0x90` and proxy argument.
That owner is located, not hooked by this increment.

## Capture inputs needed to complete coverage

F9 now records actual input-layout elements and up to 24 bounded vertex-stream reads per sampled
interval inside the native translucency execution scope. Each stream records its buffer offset,
stride, allocation size, sampled bytes and blob identity, alongside shader/draw/view identity.
Reads occur synchronously while the draw's buffers still hold that geometry. `cb_<id>.bin` blobs
remain deduplicated and budgeted; geometry records explicitly identify their vertex-stream use.
There is no continuous vertex readback outside the opt-in capture.

The source sprite stream stores a stable particle counter in OldPosition.w and previous centers
in OldPosition.xyz. Fresh affected-scene captures must establish AC7's actual declarations,
emitter identity, billboard/deformation history and opacity producer. The earlier captures did
not retain vertex streams. Needed scenes are carrier launch/refuelling, moving ground vehicles,
animated aircraft/kill-cam, and smoke/contrails or other additive/translucent effects.

F9 motion summaries now select the packed-velocity and decoded-motion views rather than RAW.
Packed clear zero and decoded sentinel are unwritten; decoded zero is valid. Lossless raw files
remain the original GPU values.

## Validation

MSVC Release `eng/verify.ps1 -Configuration Release -VS2026` passed: 44 native tests passed,
seven opt-in vendor tests skipped; Rust formatting and Clippy passed. The WARP capture fixture
verifies exact particle-layout attributes and a 96-byte vertex-stream readback. These are build
and synthetic results, not weapon, vehicle, particle, kill-cam or translucent-pass game acceptance.
The shared-motion and Project Wingman changes already present in the working tree were preserved.
