# Drag'n Wash character motion, 20 October 2026

## Question and evidence

The user reports trails on moving characters and their textures with XeSS SR and FG
enabled on an MSI Claw 8 A2VM (Arc). Turning FG off has not been confirmed as an
independent comparison. The existing local player logs belong to the NVIDIA machine,
not the reported Claw run.

The examined executable remains SHA-256
`5fdfffe386a2f43b77626cd3d70554d84c6588c94d309544924d6fab088ddafc`, Steam build
25286774, Unity 6000.3.14f1, URP package identifier `3b809f23691d`.
The original `globalgamemanagers.assets` is
`0948b788c15132fccb447a120daa4ad991ebf7878db7365757381dbb7c0ab4f0`.
Baseline source is `3cbaaa9`; unrelated work in the checkout is excluded from this repair.
Delivery reconciles this change onto `main` at `267c0e5`, with a fresh Release managed
build and both contract fixtures from the isolated checkout.

UnityPy 1.25.0 inspected all 106 packaged Shader objects and renderer/material references.
The 272 serialized SkinnedMeshRenderers request object motion and retain previous skinning
data. However, character materials reference `WalkNWashLit` 127 times, and that shader
contains only Forward, ShadowCaster, DepthOnly and DepthNormals passes. URP's native
motion producer selects only opaque draws with the MotionVectors LightMode. Enabling
skinned motion on the renderer cannot create the missing shader pass.

[Unity's matched-version motion-vector documentation](https://docs.unity3d.com/6000.3/Documentation/Manual/urp/features/motion-vectors.html)
explains the shader-pass requirement and the opaque-only producer. This establishes a
coverage defect, not proof that every reported Claw trail has the same cause.

## Other shader findings

Seven shaders contain a named MotionVectors pass: Rain, WalkNWashLitStatic,
Vilar/EyeNaelEdit, URP Lit, URP Simple Lit, URP Unlit and WalkNWash_transparency.
The other 99 include fullscreen post-processing, blits and UI, which do not need
geometry motion passes. Absence alone does not make those shaders defective.

Relevant additional coverage gaps are:

| Shader/group | Finding |
| --- | --- |
| Jiggle/ProceduralPrimitiveURP | Forward, DepthOnly and DepthNormals; no object-motion pass for its procedural deformation |
| Particles/Standard Unlit | No motion pass; the `flaps` material has 13 skinned-renderer material references |
| 35 Hidden/VFX shaders | No named motion pass; procedural particle history and transparency need their own producer |
| FluidParticles and the fullscreen fluid composition | No motion producer for the animated fluid surface/composition |
| WalkNWash_transparency | A pass exists, but URP's opaque renderer list does not establish transparent coverage |
| Decal/projector shaders | No independent motion pass; changing surface shading is distinct from mesh movement |

These findings are an audit, not a claim that all these paths have been repaired.

## Repair

`tools/patch-drag-n-wash-motion.py` generates a private ReScaleFrameMotion pass from
the installed character shader's DepthOnly variants. It checks the exact original asset
hash, keeps original game bytecode/decompiled material in ignored output, and stages a
new asset outside the installation. Only serialized Shader object 81 changes. All
original colour, shadow, depth and normal passes remain intact.

The transformation uses the local 3DMigoto decompiler and D3DCompiler 47 to wrap all
32 vertex and 32 fragment variants, including the original GPU deformation and clipping.
The original depth vertex entry exposes the deformed world position in TEXCOORD0.
The wrapper retains that position in a per-renderer/submesh double buffer indexed by
SV_VertexID. The next draw projects the retained position through the previous view
projection. This avoids substituting a stock pass that omits custom deformation.

The matched player/PDB also establishes the Direct3D program envelope. At RVA
`0x94e4e0`, `D3D12CommonShader::CreateFromSource` consumes a version byte, five
resource-header bytes, then the version-2 32-byte extension before DXBC. Resource
counts at offsets 1/2/3/4 describe SRVs/CBVs/samplers/UAVs. The CBV, SRV and sampler
limits in `IsValidOnTier` (`0x94e810`) and the per-stage/root descriptor updates in
`D3D12ImmediateContext::BindShaders` (`0x941ac0`) distinguish these fields. The wrapper
needs 32 SRV slots and two UAV slots. An initial header edit incorrectly increased
the sampler field; static player analysis caught it before any game launch. The
verified original asset was restored and the corrected artifact regenerated. These
are offline instruction/symbol observations, not a player execution result.

The game-specific managed producer is under `games/drag-n-wash/managed`. A Harmony
postfix on MotionVectorRenderPass.Render records an unsafe graph pass after ordinary
camera/opaque motion production and before SR's consumers. It writes the existing
motion/depth attachments, imports both history buffers with explicit read/write access,
binds the vertex UAV at slot 1 and the previous-position SRV at slot 31, and clears
random-write targets after each draw. The private LightMode prevents an ordinary URP
renderer-list draw without those history bindings.

Motion remains Unity current-minus-previous UV, including camera and object motion,
without projection jitter. The shared normalizer retains its existing conversion to
SDK render-pixel units for both SR and FG. Frame tags reject unwritten/stale vertices;
new views, generation changes, gaps, changed meshes/materials and ForceNoMotion avoid
reusing incompatible history. Buffer identity belongs to renderer and material slot,
so different deformations on a shared mesh cannot overwrite each other's history.

`eng/deploy-unity-character-motion.ps1` verifies the game, original asset, staged
shader report and owned helper; refuses a running game; backs up both files and the
manifest; and records the one intentional game-asset patch. The normal Unity deployer
recognizes that recorded hash while retaining the vanilla baseline. Original game
assets and generated derived shader binaries are not committed or publicly released.

## Executed checks and remaining limits

- All 64 generated shader variants compiled with D3DCompiler 47.
- Asset serialization/reload changed only object 81. Original shader passes and all
  other objects remain intact. All 277 original installed files match their baseline
  except the one recorded, authorized asset patch.
- Release managed build has zero warnings/errors. CoreCLR and shipped Mono fixtures
  match all 13 URP contracts; the Mono Harmony patch/cleanup fixture passes.
- A real D3D12 fixture on the RTX 4070 Laptop executes the generated base vertex and
  pixel shaders with double-buffered histories. First-frame motion is zero; a combined
  0.1 vertex displacement plus 0.2 object translation produces 0.15 UV X motion; the
  settled third frame returns zero. This is a shader/device fixture, not Unity or Claw
  gameplay acceptance. Its initial sample was outside the moved triangle; moving the
  sample inside the overlap corrected the fixture, not the production shader.
- The isolated Unity Editor fixture could not run: the installed 6000.6.6f1 Editor
  exits with code 198 because it has no valid license. No activation settings changed.
- An offscreen test helper is prepared for the actual player. Launch authority is
  pending. Unity's native shader/resource binding, a real character scene, Claw pixels,
  wider shader coverage and an SR-only versus SR+FG comparison remain unverified.

The patch and managed helper are deployed locally with backups. This remains a
candidate repair until the player binding and moving-character checks succeed.
Final asset SHA-256 is
`96150c2617614ae2657e235845f795403614a4173b7f7676f7ffcb16ed10c791`;
the matched helper is
`18fcfa9c2568f5d083205d914fbdec936cd5e9b8ac72a6404ce0d298b9915b90`.
Raw shader sources, logs and GPU fixture material stay in `.local/drag-n-wash`.

The earlier local build recorded helper SHA-256 `f638929cd531cb8e1bda00f9543a5f85def0aeb587180c9e07686a98a8359808`.
The reconciled delivery identity above supersedes that local binary identity.
