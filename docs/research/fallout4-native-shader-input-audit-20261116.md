# Fallout 4 native shader-input audit, 16 November 2026

## Scope and method

This independent offline audit asks which shipped shaders can produce temporal
inputs, which current/previous data they consume, and what the adapter still
has to establish at real draws. It does not replace shaders or establish SR/FG
quality. No game, GPU device, build or shared REA/IDA session was used.

The executable is Fallout4.exe 1.11.240.0, SHA256
`fdcef37ac1230af6d0b0050eb2142b139ef3a867b37b9211fb6edfcc646072f8`.
The retained archive SHA256 is
`6fecbd744f0f3235dea45dda4c259abd90fb599897ecb04226c9916a723889d3`;
entry `ShadersFX/Shaders011.fxp` has SHA256
`13569a497f9ba091a2be293859bcf5585ad8faa8fc1d3ad74991b8512eea125b`.
All 3,939 inventory records were independently hash-checked and disassembled
with system `D3DDisassemble`, flags 0. The system compiler DLL SHA256 is
`81494adc53aae6a9b2cda729da5d20b0d15bffcb663dff7ae0242cab2e1f1a58`.
This disassembles existing bytecode; it does not compile replacements.

All 3,801 VS/PS/CS records also match the retained reference SHA1/SHA256 and
ordinal join to Community Shaders revision
`e0e711594291c1a8e447ac758099492514810fbb`, including its
`tests/data/stock-shader-identity-AE.tsv`. The 69 HS and 69 DS at ordinals
2463..2600 remain family-unmapped. They sit between deferred-prepass VS and PS
records, but adjacency is not a verified runtime pairing or technique name.
The container has 1,339 VS, 2,357 PS, 105 CS, 69 HS and 69 DS records; this
does not claim every shader loaded by every mod or another archive is included.

Reproducible ignored artifacts under `.local/fallout4-research/`:

- `audit-native-shader-inputs.py`: CPU-only disassembly and signature audit.
- `native-shader-input-audit.json`: compact family/stage counts, CB layouts,
  operand ranges, resources and input signatures for all records.
- `native-shader-input-excerpts.json`: bounded native instruction excerpts for
  33 selected ordinals, with hashes, signatures and terminal instructions.

These extend the supplied container inventory and reference join, rather than
dumping the full container repeatedly. Reference HLSL is reconstructed and
feature-extended CS code. Its names help interpret independently checked native
operands; a CS replacement expression or new feature output is not evidence
that the shipped shader implements that feature.

## Coverage table

Counts describe container permutations, not draws or screen coverage. A shader
output register does not identify the resource bound to that MRT slot.

| Path | Native bytecode and source correspondence | Concrete limit or required live fact |
| --- | --- | --- |
| Opaque/static prepass | 315 VS and 470 PS. Exactly 360 PS declare target 5 and CB12[41]; 110 declare only targets 0..4 and CB12[31]. PS2607 computes current NDC from CB12 rows 37/38/40 and previous NDC from 31/32/34, then writes `(current-previous)*(-0.5,0.5)` to o5.xy. | Identify which material variants take the 110 non-motion PS paths and whether they overlap the world reconstruction input. Verify MRT5 actually names logical motion32 with correct blend/write mask and native clear. |
| Skinned geometry | 155 prepass VS declare and dynamically read both CB10[180] and CB9[180]. VS2149 is a checked example. `DFPrepass.hlsl` names them current and previous bone rows, respectively. Both positions also account for current/previous origin CB12[35/36]. | Native bytecode has separate bone history inputs, disproving a blanket missing-bone-buffer claim. Actual per-actor/partition CB9/10 allocation, contents, update order and reset lifetime remain unobserved. Reusing a current vertex with previous bones does not prove previous non-skeletal deformation. |
| Plain instancing | VS2174, descriptor 0800001a, indexes CB13[4095]. Current and previous output positions reuse the same instance rows 0/1/2; only origin35 versus36 changes. Row6 supplies UV transform. | This shader has no independent previous instance transform. It cannot describe an independently moving instance through those lanes. Determine whether such draws move, their stable instance identity, and whether another native pass supplies that motion before proposing a history producer. |
| Combined/GPU-driven objects | Other prepass variants declare structured t6 stride100; reference `CombinedTransform` separates current and previous rows. Native declarations also include raw streams and other t6/t8 layouts. | Do not generalize plain-instance limits to every combined variant. Map the exact active descriptor and stream layout, preceding compute writes, UAV-to-SRV lifetime and previous-row contents for the draw. |
| Terrain and LOD | VS2191 uses CB13[100], structured t4 stride76 and t5 stride68, instance/vertex IDs, camera origins and per-draw CB2. Distant-tree family has 2 VS/2 PS; both PS write only target0, with no paired-position lane or previous-camera reads in its VS. | Static terrain can use camera motion only if depth matches visible geometry. Check LOD morph/patch replacement history and coverage of distant vegetation; a depth-only or colour-only tree path is not independent wind motion. |
| Grass/tree/spline deformation | Native prepass variants consume distinct current/previous per-draw positions/origins; selected tree VS2148 and skinned VS2149 retain native operands. The reference branches explicitly evaluate separate wind/origin histories for grass, tree and spline. | The reference expressions do not prove native animation constants are fresh or correctly rolled. Capture corresponding CB2 lanes and the native variant under actual wind, actor interaction, spline motion and history reset. |
| Tessellation/displacement | All 69 HS and 69 DS signatures carry POSITION1/2 lanes. DS2532 samples displacement t7 and uses current CB2 rows0..2/origin35 versus previous rows4..6/origin36; 17 DS declare CB12[37], while 52 declare CB12[12]. | Signature presence is not proof every lane is read: DS2532 does not declare all signature control-point lanes as used. Name and pair the real HS/DS/PS techniques. Test previous displacement content as well as previous transforms; current height applied twice cannot encode a time-varying previous surface. |
| Sky/cloud layers | 9 VS/9 PS. Eight PS declare target0 and target1, consume previous POSITION1, and read current/previous CB12 projection rows. PS30 has native motion arithmetic; one PS declares only target0. | Confirm MRT1 is the shared motion allocation for sky draws, sky depth/clear conventions, cloud animation history and later overwrite. CS feature cloud-shadow targets2/3 are not shipped-native evidence. |
| Water/refraction | 102 VS/94 PS. 89 PS declare target0 only; five have targets0/1. Native water CB12 operands do not read previous projection rows31..34. PS2059's two outputs are ray/depth data, not a motion pair, matching the reference ray/depth helper. | Counting target1 as water motion would be wrong. Measure actual water surface depth, refraction/displacement, coverage and late compositing relative to motion32 and pre-SR Main. No dedicated native water-vector producer is established here. |
| Effects, particles and forward surfaces | All 968 effect PS and six particle PS declare only target0. Effect VS read current CB12 projection/origin/fog but not previous projection31..34; 122 declare bone CB10 without CB9. Particle VS use local CB0/2, not CB12. Lighting family has 18 VS/18 PS with target0 only. | These interfaces do not supply a dedicated paired motion MRT. Establish actual world contribution, depth writes, blend and any alternate motion/mask pass for moving particles, alpha/additive effects, skinned forward effects and distortion. Camera reprojection cannot recover their independent motion. |
| Compute deformation and decals | DismemberCS3937 reads raw t0/t1 and writes structured/raw UAVs. SkinnedDecalCS3938 reads CB10[180] but no CB9 and writes three structured UAVs; MeshCombinerCompute3603 reads structured t0/t1 and writes raw u0. | A compute output is not a motion image. Trace which later vertex/SRV streams consume it and whether prior positions survive topology/decal generation; dispatch existence does not prove temporal geometry history. |
| Volumetrics and sunbeams | Named native VLSApplication3831/Composite3833 write target0; ScatterRay3841/ScatterAccum3845 read CB12[24] and several 2D/array textures and write scalar/colour output. Sunbeams3880 reads t0/t1 and writes target0. | These are volume/lighting consumers, not demonstrated object-vector producers. Identify depth, volume/history texture identity, render extent and composite order before Main is captured. Do not call an arbitrary sampled texture a previous-frame history. |
| Imagespace, menus and camera-free UI | Imagespace includes 127 VS, 124 PS and 86 CS. Native TAA3868, motion blur3866, Pip-Boy3801, VATS3819 and lens-flare3896/3904 have distinct identities; fullscreen VS often have no camera CB. Blood splatter writes target0; face customization writes targets0..2. | Screen-space effects and UI must retain their native view/rect. Neither shader family nor a bound inherited CB12 proves world ownership. Confirm world versus menu/map/Pip-Boy/reflection views and which consumers execute after the chosen SR seam. |

The native PS2607 arithmetic independently establishes the signed vector
convention previously supported only by source inspection: native XY is
previous-minus-current UV with Y down. Adapter `motion_to_uv={-1,-1}` converts
it to the shared current-minus-previous convention. This establishes the checked
shader's encoding, not that every visible pixel has a current vector.

## Producer and lifetime boundary

Existing exact-build guards select ordinary-world entry, temporal update
`182d580`, native imagespace/SR call `21fa283`, and the post-imagespace viewport
return `21fa2ca`. The reader verifies the world camera getter/cache, copied
current/previous matrices and origins, logical-to-platform motion32/depth1/Main1
mapping, device identity and callback leases. Those are useful engine/resource
facts. They do not verify CB12 upload contents at each shader stage, CB9/10 bone
history, instance/compute stream writers, or per-draw OM bindings.

CB12 projection/previous-projection roles agree with native PS operands and the
reference layout. Camera cache history is independently traced in the native
reader research. The missing link is the active per-stage constant-buffer
update/bind and correct frame/view pairing. There is no verified bone/instance
producer hook in the current adapter that fills a missing lane, and this audit
does not invent one or replace missing vectors with zero/current transforms.

The pre-SR synchronous lease must capture motion and depth before reinsertion
changes Main or later imagespace consumers overwrite resources. The actual
last motion writer, clear region, viewport/scissor, native depth convention and
resource contents at that seam still need draw evidence. No native reactive or
transparency-mask absence is proved from these signatures: a separate pass can
write a mask through target0, whose resource identity is not encoded in DXBC.

## Prioritized next facts

1. At one verified ordinary-world source, correlate the actual VS/HS/DS/PS hash
   and descriptor with MRT resource identities, blend/write masks, depth target,
   viewport/scissor and final motion32 writer before `21fa283`. Include one
   moving rigid object and one skinned object. Retain CB12 current/previous rows
   and CB9/10 identity/content for the same source and preceding frame.
2. Check the native plain-instance limit using one independently moving instance
   or prove the admitted draws are stationary. For combined/compute geometry,
   identify the stream writer, current/previous layout and stable object ID;
   no generic camera fallback can supply absent object history.
3. Cover non-motion prepass variants, alpha/additive particles, water and distant
   vegetation with actual colour/depth/vector samples and any separate mask
   passes. Missing coverage matters even when the SR SDK evaluates successfully.
4. Identify actual tessellation pairings and animated displacement history, then
   volume/refraction/DoF/AO/TAA consumers and view exclusions across native versus
   reduced extents. Reuse existing bounded capture/draw tools; no new capture
   framework or blanket shader patch is justified by this offline audit.

The concrete plain-instance history limit and sparse colour-only interfaces
are prioritized findings, not proof of the current startup or input-admission
failure. No shader replacement or production fix was made. Whole-integration
motion coverage and visual acceptance remain incomplete.
