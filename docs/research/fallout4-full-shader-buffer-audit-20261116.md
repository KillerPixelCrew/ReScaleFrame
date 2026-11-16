# Fallout 4 native shader and buffer coverage audit, 16 November 2026

This is the source/bytecode stage of the required full SR/FG audit, not completed
moving-game acceptance. It asks which native producers can supply correct depth,
motion and history, which variants demonstrably omit them, and what exact owner
must be verified before a repair. Native-only validation comes first; F4SE/ENB
compatibility remains required afterward. The current native Quality1 visual
failure is retained as failed acceptance and belongs to the separate colour/
reinsertion investigation. This audit does not assign its cause to motion or ENB.

The scope here is shader packages, every enumerated program/stage/permutation,
buffer contracts and temporal coverage. The coordinator and independent reference
review own the complete nonshader subsystem inventory; this note does not replace
or duplicate that lifecycle/settings/presentation inventory. See the reconciled
[root reference comparison](fallout4-community-shaders-inventory-root-20261116.md)
and [independent source inventory](fallout4-community-shaders-inventory-claude-20261116.md).
It follows AGENTS.md
and the updated reference-and-shader-audit procedure at revision ca3021c. No
production file, CPU diagnostic, native module, reference source or game data was
changed. No build, device, game, attach, process query, REA/IDA/Ghidra operation,
deployment or commit was performed.

## Identity and reproducible offline evidence

| Input | Exact identity / scope |
| --- | --- |
| Executable | Fallout4.exe1.11.240.0,55343016 bytes, SHA256 `fdcef37ac1230af6d0b0050eb2142b139ef3a867b37b9211fb6edfcc646072f8` |
| Reference | Fallout4 Community Shaders `e0e711594291c1a8e447ac758099492514810fbb`; tracked checkout clean during review |
| Consumed shared source | `6e67083a54090d10806484549d07f75b7fbc8dc2`; consumed entries from parent `xmake/shared.lua`, not the whole shared checkout |
| CommonLibF4 / nested relocations | `7f8deca16dc7dd245c9c52151ec9ef27edd9ad60` / `5408d22adf937cc00e0ccd4bdeef66021172884d`; layouts/IDs remain source leads until matched natively |
| Native package | Fallout4 - Shaders.ba2, BA2v8/GNRL, SHA256 `6fecbd744f0f3235dea45dda4c259abd90fb599897ecb04226c9916a723889d3` |
| Extracted entry | ShadersFX/Shaders011.fxp,12844936 bytes, SHA256 `13569a497f9ba091a2be293859bcf5585ad8faa8fc1d3ad74991b8512eea125b` |
| Offline disassembler | System d3dcompiler_47.dll/D3DDisassemble flags0, SHA256 `81494adc53aae6a9b2cda729da5d20b0d15bffcb663dff7ae0242cab2e1f1a58`; no device or compilation |

The existing native container/input audit was reused and extended, not treated as
live binding proof. All3939 extracted DXBC records were independently disassembled
again and checked by raw SHA256. All3801 mapped VS/PS/CS records match the pinned
AE table's full FXP ordinal and raw SHA1. The table reader contract is
`tests/StockShaderTable.cpp:90`; the new inventory retains native name, class,
source group, native macros, early-depth flag and descriptor, rather than collapsing
all imagespace programs to descriptor0. There are312 named program/stage groups,
30 family/stage groups and138 still-unnamed HS/DS records. No inference assigns
those stages to an adjacent family.

Ignored evidence under `.local/fallout4-research/`:

- `full-shader-buffer-native-inventory-20261116.json`: every ordinal/hash, stage,
  permutation/macros, input/output/patch signatures, declared CB float4 extents,
  actual constant operands including dynamic indexing, resource declarations and
  operand slots, output/UAV/discard instructions, and per-family/named-program
  aggregates. Every live-binding field remains unobserved.
- `full-shader-buffer-native-examples-20261116.json`:35 representative complete
  disassemblies spanning every family/stage and current/previous palette contract.
- `full-shader-buffer-source-boundaries-20261116.json`: exact source-stage manifest,
  shader register/include/temporal-use inventory, all79 installed BA2 name tables,
  loose shader scan and package exclusions.
- `audit-full-shader-buffer-native-20261116.py` and
  `inventory-shader-source-boundaries-20261116.py`: offline generation only.
  Generate source boundaries first, then native inventory. The final freeze receipt
  hashes all artifacts and the unchanged CPU source.

An initial lexical instruction scan missed indented branch stores/discards;
left-stripping each disassembled instruction corrected that extraction. All105
CS now have recorded UAV write code. This corrects the audit parser, not game
behavior. Some archive names are legacy non-UTF8; a strict decode first failed,
then names were decoded with CP1252 fallback while raw name-table bytes remained
hashed. These failures are retained to prevent silent inventory omissions.

## Package and source ownership boundaries

All79 installed Data BA2 name tables were inspected, without decompressing other
payloads or asserting mounted load precedence. Only Fallout4 - Shaders.ba2 names
a compiled shader container, Shaders011.fxp. Other shader-name matches are meshes,
materials, textures or sounds, not extra `.fxp/.hlsl/.cso/.vso/.pso` entries.
No loose files were found under Data/Shaders or Data/ShadersFX. This is a filename
boundary check, not proof that embedded shaders are absent from DLLs, Scaleform
movies, executable data or Nvflex payloads. Those sources, possible driver/vendor
programs and future mods remain explicit discovery gaps. No GS occurs in this
FXP; GS absence across the running game is not proved.

The parent `xmake/shaders.lua:14` stager consumes local package/Shaders and each
feature's Shaders tree, plus49 explicit shared entries from `xmake/shared.lua`.
The expanded stage contains175 files:104 local and71 consumed shared. The machine
manifest records every destination/hash, include and register declaration. This
includes local geometry/deferred/imagespace programs; FO4 common/camera/depth
helpers; local feature shaders; and explicitly consumed shared colour, GBuffer,
lighting, GI, shadows, terrain, water, HDR and RCAS code. Entire vendor SDK
Upscaling/XeSS, FidelityFX and Streamline trees are excluded by that stager and
remain separate SDK ownership, not native geometry coverage.

Shared `Particle.hlsl`, `Lighting.hlsl`, Utility and other nearby Skyrim programs
are not implicitly consumed. The pinned AE table identifies635 native VS/PS/CS
records whose selected source file is absent from the staged tree. Examples are
native Particle, Lighting, Utility, BloodSplatter, FaceCustomization, mesh-copy/
combiner and skinned-decal compute. This is a reconstruction/source availability
gap, not proof that those native programs fail or that RSF must replace them.
Their shipped bytecode and interfaces are fully included below. Includes and
feature macros describe source contracts; this worker did not compile the staged
reference or certify its claimed identity tests.

## Whole native producer and consumer matrix

CB extents below count float4 registers, not bytes; a declared extent does not
prove a bound allocation/range or that every register is read. Exact variant-specific
ranges and structured/raw resource declarations are in the machine inventory.
Every row still needs actual IA/CB/SRV/UAV/OM state and visible draw/dispatch
association. Shader target numbers are registers, not logical engine texture IDs.

| Family / all native stages | Required producer contract and proven shader behavior | Remaining owner/binding/repair work |
| --- | --- | --- |
| Deferred prepass315VS/470PS | VS b12 up to37 camera registers; b0/b1/b2 per draw;155VS read both b9[180] previous and b10[180] current skin palettes. PS material t0..15 and b12 up to41.360PS expose target5 motion;110 expose only targets0..4. | Bind target5 to logical32 and prove write masks, depth test, clear and coverage. Match each mesh's current/previous history and frame. BLEND gap is real code capability, not yet its live pixel footprint. |
| Static rigid and alpha-test permutations | Per-draw world/previous rows and POSITION1/2 feed unjittered-current and previous camera projection.335prepassPS variants contain discard code. | Inspect native previousWorld update order, alpha thresholds and depth/colour/motion coverage alignment; no reconstruction from only camera motion. |
| Skinned prepass | Example ordinal2149, descriptor0x4000001e, SHA256 `e5e2fbc5c3c2197d63e16b3b3813852d6ca68ace0ceacf7ed6da680021966914`, dynamically indexes distinct b9/b10 palettes. Both declare180float4=2880bytes each. | Separate bone history exists in code; do not claim it absent. Need actual two buffers/ranges/content/frame generation and bone IDs/weights per independently moving skeleton, including first-person hands. |
| Instanced/combined/merged prepass | TwelveVS declare b13 (4095 or100 rows); native operands cover455x9-row records or terrain table. VS resources t4..8 hold packed geometry, indices/remaps and transforms. | Inspect IA offsets/stride/base vertex/instance and indirect CPU/compute stream generations. Instanced source uses current rows for previous projection; static instances can be correct, moving instances need true prior placement. Combined t6 explicitly carries current/previous rows; merged path needs its own owner proof. |
| Terrain/LOD/grass/tree/spline prepass | Descriptor flags distinguish landscape/LOD, vegetation and dismemberment. Reference independently computes current/previous wind/gust, origin and spline terms. | Prove wind phase/history CB contents and previous world/origin for moving foliage, LOD swaps and transition blends. No vegetation-history substitute from rigid camera reprojection. |
| Unnamed69HS/69DS | HS passes POSITION1/2/control attributes and optional b2[6]. DS uses b12[12/37], optional b2[6/8], t7/s7, outputs raster and POSITION1/2 lanes; all69DS use t7 operands. | Resolve actual HS/DS shader-object owner, topology, paired VS/PS, displacement texture/mips and current/previous displacement. No blind prepass-family assignment or tessellation exclusion. |
| Effect474VS/968PS | VS b12[47],122VS current b10[180], b11[128] in indexed variants; raw/structured geometry t8..11. All968PS output target0 only and contain discard code. | No independent Effect motion MRT exists in this corpus. Identify blend/additive/soft/distortion/weapon/weather draws, true history, target/depth semantics and their visibility before implementing an appropriate motion/reactivity path. |
| Particle6VS/6PS | VS b2[11/17/19], optional b0; PS b2[1], t0/t1 samplers. Target0 only; no b9/b10 palette. | Capture particle allocation/spawn identity, prior positions/billboard basis and lifetime. No output motion target is proven; do not infer absent previous particle data solely from unnamed CB2. |
| Forward Lighting18VS/18PS | FourVS read current b10[180]; other variants b1/b2, PS t0/1/2/4/6/12/14/15. PS target0 only. | Determine which forward objects already had motion/depth prepass, which are late independently moving layers and which are nonworld/offscreen; repair only the verified missing producer. |
| Utility259VS/177PS |92VS use current b10[180], b12[36]; raw resource t5..8 variants.72PS colour target0;105PS no colour target. | Shadow/depth/alpha-test/reprojection/merged ownership is not interchangeable. Prove DSV and stream generations; an absent colour target is not absent depth or a motion bug. |
| Distant tree2VS/2PS | b12[47]/b2[4] current projection, t0 alpha/colour; PS target0. | Determine LOD depth-only versus colour draw and motion supplied elsewhere; establish wind and LOD transition history without assigning zero globally. |
| Sky9VS/9PS | EightPS output0+1, one target0 only; current/previous world positions/camera projection. PS b12[41], t0..2. | Prove target1 logical32, sky depth/clear order, moving cloud/sun/stars history and any later overwrite. Preserve valid static/occlusion zero. |
| Water102VS/94PS |89PS target0, five target0+1; camera/material CBs, depth/reflection/refraction t1/2/4/5/6/7/9/10/11. | Target1 in reference SSLR path is ray/depth data, not motion. Resolve world water draw order, displaced geometry history, alpha/refraction and late depth writes; no water-motion claim from MRT count. |
| Deferred light1VS/306PS | PS target0+1; b12[28..31], t0..8 GBuffer/light/shadow reads. | Identify accumulation/additive write masks and active rectangle; these light buffers are not independent geometry motion producers. Preserve depth/normal coordinate agreement. |
| Deferred composite23VS/180PS |132PS target0,32target0..3,16target0..1; GBuffer/depth/effect SRVs and some raw t8 VS input. | Verify exact resource identities and late clear/copy/refraction/foreground writes before SR; target1 cannot be labeled motion without native binding proof. |
| Tiled lighting19CS | All19 contain UAV write code, t0/1/2/4/5/6/7, u0/1/5/7, b0 and optional b12. | Dispatch grid, structured strides/counters, depth convention and hazards/barriers must match later deferred consumers. No stage omission because it is compute. |
| Blood splatter2VS/2PS; face customization1VS/1PS | Blood target0 UI/overlay; face targets0..2 and large PS b12[320], t0..4. | Establish UI/offscreen versus world contribution, native scale and history/reset ownership; do not feed UI material fields into world motion. |
| Imagespace127VS/124PS/86CS | All native names/macros retained, including TAA, motion blur, HDR/exposure, blur/bloom, SAO, SSLR, VATS, PipBoy/HUD glass, local map, volumetric slices, lens flare, rain and native DR reconstruction. | Trace active branch and all later consumers; no blanket claim all pass-through shaders are resolution independent. Separate UI/offscreen histories and native/reduced/failure modes. |

The105CS include all19 tiled programs and86 other programs. The latter include
MeshCombinerCompute32, VertexBufferCopy2, IndexBufferOffset8,
MergeInstancedVertexBufferCopy2, DFDecals1, Dismember1 and SkinnedDecal1, plus39
HDR/exposure/blur/SAO programs. Their UAV writes are explicit in the inventory.
SkinnedDecal ordinal3938 has b0[9]/b10[180], raw t0/t1 and structured u0/u1/u2
strides32/4/36. Dismember ordinal3937 reaches u0..7. Their output streams must
be traced into later IA/SRV consumers and history ownership; a generic prepass
bone audit alone does not cover compute-produced cuts/decals/geometry.

## Proven temporal differences and repair decisions

1. **Existing two-palette skin history must be preserved and verified.**
   `DFPrepass.hlsl:1927/1932`, CurrentBone/PreviousBone at2422/2428 and the
   previous weighted skin at2808 agree with native dynamic b9/b10 operands in
   all155 skinned VS. Neither substituting current bones nor allocating another
   arbitrary previous palette is justified. If moving capture finds aliasing,
   stale range or wrong generation, repair that exact native upload/binding and
   retain its owner until both native and SR/FG consumers complete.
2. **The BLEND omission is a real shader-level gap, with exceptions.**
   Native descriptor bit15 and target signatures divide the470PS into327
   non-BLEND with motion,110BLEND without motion and33BLEND with motion.
   Ordinal2601/0x00018802 is a no-motion example. Reference
   `DFPrepass.hlsl:119` retains motion for tessellated-height/meat-cuff AE
   exceptions; it does not repair all BLEND. A repaired variant would require
   verified native MRT attachment, independent write-mask/blend/depth/alpha
   behavior and genuine previous geometry before adding its motion output.
   Simply enabling a sixth target or interpreting normal/alpha as velocity is
   not a repair. Live material/pixel footprint remains unresolved.
3. **Effect/particle/forward layers cannot be declared covered by the GBuffer.**
   Their native PS output signatures prove no separate motion MRT in those
   programs. They may reuse valid underlying world motion or have an earlier
   matching prepass; that distinction needs draw/resource evidence. Required
   independently moving translucent/additive/particle motion needs its real
   previous primitive/deformation/instance data and a verified composition or
   reactive/transparency contract. Camera-only vectors, fabricated zeros and
   current-as-previous do not satisfy independent motion. New allocation or
   shader replacement is not justified until the exact missing native path
   and buffer owner are established.
4. **Instance/deformation contracts have narrower history than a bone check.**
   Source `DFPrepass.hlsl:2004` combined transforms include previous rows;
   instanced455x9-row records use current placement again at2835. Native
   ordinal2174/0x0800001a reads b13 relative offsets0,1,2,6, not arbitrary
   reserved prior rows. Independent moving-instance history is therefore not
   supplied by that transform record as used; static placement may be valid.
   Native grass/tree/spline paths have dedicated previous phase/origin terms
   and must not be replaced by one rigid object matrix. DS displacement and
   compute-generated geometry require their own previous-state proof.
5. **MotionVectorFixes is a reference producer repair, not a bone replacement.**
   At pinned `features/MotionVectorFixes/src/MotionVectorFixes.cpp:30/43/58`,
   recursive helpers cache world and restore previousWorld. Around actual
   OnIdle_UpdatePlayer it snapshots the player's 3D world transforms before
   native update and restores previousWorld afterward. A separate sequence
   reposition hook resets history after NiAVObject::Update; pre-lighting
   callback132 handles frozen/menu and LOD/landscape except LoadingMenu.
   `src/Render/LightingRenderPasses.cpp:34` runs this before native pass-list
   construction, via source slot43, not at SR. Its deviation note explicitly
   leaves BLEND motion gaps uncorrected. RSF has no corresponding actor history
   repair today. Required next proof is the exact native update/pass-list
   owner and history writes, not copying its recursive hook or trusting
   CommonLib NiAVObject world+70/previousWorld+C0. Source IDs OnIdle_UpdatePlayer
   {1318162,2228929}, SetSequencePosition {854236,2200766} and call+1D7 remain
   relocation/search leads here, not executable patch permission. Existing
   matched worker UpdatePlayer ordering can guide that proof. Resetting history
   at a proven discontinuity is distinct from inventing previous motion to
   hide missing data; the reference's menu/LOD policy is not imported blindly.

## Inputs at the handoff and later consumers

Native motion is previous-minus-current UV from unjittered-current and previous
clip projection: source prepass1668..1695 and Sky189..204 use
`(currNDC-prevNDC)*(-0.5,0.5)`. Native instructions and exact record identities
retain the arithmetic; live previous transforms/history and camera jitter
contents are still unverified for moving objects. The native motion allocation
logical32/R16G16_FLOAT and same-source camera/active crop reader are already
recorded in the integration notes. Float zero can be correct stationary motion;
a clear, unwritten/masked pixel, previous-frame residue and a valid zero cannot
be distinguished from the value alone.

Before declaring coverage, establish initial clear value/rectangle, all binds to
logical32, blend/write masks, viewport/scissor/depth/stencil tests and last writer
for world and foreground partitions. In native first-person frames depth carries
partitioned world/near domains; the copied descriptor normalizes vendor depth
without relabeling native raw depth for downstream consumers. Foreground hands
need their own producer palette/pose/projection and overwrite order; a valid
world-domain conversion cannot repair missing near motion. Preserve native raw
depth/stencil and expand it through the established nearest auxiliary path only
under its verified active rectangle. Do not replace it with normalized vendor
depth for native SAO, water, DoF, refraction or stencil readers.

Reference `TemporalResolve.cpp:526/584` separately reconstructs depth/refraction
and motion/PipBoy/VATS; refraction has colour filtering while discrete motion,
stencil and masks require their established conventions. VATS logical15 is also
a menu background and is restored after its proved last consumer; logical14
refraction/30PipBoy may be absent independently. Reference
`TemporalFrameGenerationInputs.cpp:230` freezes FG inputs before SR mutation.
RSF implements that separation and generic auxiliary transaction, but successful
sampling/copying is not producer history proof. After SR, verify native unit/
dynamic/partial triangle selection, full output extent and every native or CS
feature consumer: exposure, bloom, SAO, SSLR, DoF, motion blur, water displacement/
refraction, volumetric light, lens flare, rain, local map, HUD/PipBoy/VATS and
final UI. Reference-only GI/cloud/shadow/terrain feature programs have their own
buffers; their source availability does not mean they were loaded in a native
trial. Native and failed-reconstruction paths must remain visually correct at
all scales, not merely return to untouched native code.

The coordinator separately supplied fresh matched-executable REA/IDA dossiers
for RenderEffect2185610, triangle2186110, half converter770110,
UpdateDynamicResolution1839ED0 and Composite21F82A0, retained in
`rea-cs-native-reference-root-20261116.json`. The retained triangle/RenderEffect/
UpdateDR records have evidence prefixes ev_69fe4021/ev_6e33f528/ev_c9e2622a.
Triangle assembly stores half(2*FBC) at buffer+6 and half(2*FB8) at+8 through
770110. UpdateDR has no calls and changes ratios/FE5, so ratio publication alone
does not prove that stored geometry fields were rebuilt. This is a concrete
producer/consumer dependency for the separate geometry audit, not a conclusion
about the failed live frame. Root owns that analysis session and final re-export;
this worker only read its retained export and the coordinator's report.

## Minimal next evidence and implementation boundaries

The actionable queue is deliberately ordered by missing fact, not by SDK success:

| Work item | Exact next facts / rejection condition | Owner and resulting smallest repair |
| --- | --- | --- |
| A: native binding census | At actual evaluated source/view, map every active VS/HS/DS/GS/PS/CS hash to this inventory, IA topology/streams/offsets and all CB/SRV/UAV/MRT/DSV views, blend masks and viewport/scissor. Include late writes to logical32 and foreground near depth. Missing active hashes reopen package inventory. | Game adapter owns native shader/producer identification; generic capture mechanism may copy complete bind state. No broad shader proxy or arbitrary target matcher. |
| B: skin/object histories | Two adjacent evaluated frames with camera-only, rigid-object and isolated skeletal movement: actual b9/b10 contents/ranges, CB12/geometry rows, object/bone identity and upload/update sites, cuts/spawn/LOD transitions. Reject current/previous alias or stale generation only after observing it. | Exact native previousWorld/palette producer repair in games/fallout4, with expected bytes and owner-lifetime checks. Reuse native previous palette when valid. |
| C: BLEND/transparent/particle | Visible descriptor/material, actual target/depth/blend state and nearest motion/depth before/after that draw; prove independent previous pose/instance/particle identity, including additive overlays and soft depth. | Correct shader variant and native output binding/write semantics together; genuine previous stream producer where missing, or justified reactive/transparency integration. No fake vectors. |
| D: tessellation/generated streams | Resolve69HS/69DS owner pairs and DS t7 format/mips; dispatch-to-draw dataflow for MeshCombiner/Dismember/SkinnedDecal with view/source generations and previous deformation. | Native producer/variant/history repair only after exact transform/stream ownership; no unproved stage exclusion. |
| E: temporal and full-output consumers | Actual evaluation captures at native and every standard quality, moving foliage/actors/water/particles, near/far edges, cuts, resize, backend/quality switches, overlays/offscreen views and injected real failure recovery. | Adapter consumer/reinsertion corrections and reusable generic GPU operations as needed. Current Quality1 rejection remains a blocker until its first bad same-source product is located. |

An acceptable bounded next capture must attach bindings and histories to the
actual evaluation source/native frame/view and matched Present, not to a
preparation call or arbitrary stationary frame. Existing F10 texture copies do
not contain the complete draw/dispatch/history census above. Root owns the next
capture/REA session; this worker did not attach or propose bypassing its target.
The current CPU reservation issue, shader coverage and visual Quality1 defect
are separate blockers, none solved by turning on FG or recording more Presents.

No complete shader/buffer or motion/visual acceptance is claimed. The offline
inventory is exhaustive for the enumerated FXP and consumed source stage,
including138 unnamed tessellation records; active resource ownership, shader
coverage outside that container and all moving native/reduced/failure cases
remain mandatory completion work. The user's requested repairs must be
implemented after proof at their actual owner and validated, not closed as an
interface-only or stationary-only integration.
