# Fallout 4 native motion history producers, 16 November 2026

The question is which native owners correspond to Community Shaders' object-history
corrections, and whether the installed executable actually produces and consumes
separate current and previous transforms. This extends the
[full shader and buffer audit](fallout4-full-shader-buffer-audit-20261116.md).
It supplies verified analysis boundaries, not a history patch or moving-image acceptance.

The target is Fallout4.exe 1.11.240.0, SHA256
`fdcef37ac1230af6d0b0050eb2142b139ef3a867b37b9211fb6edfcc646072f8`.
The reference remains Community Shaders
`e0e711594291c1a8e447ac758099492514810fbb` and its pinned CommonLibF4
`7f8deca16dc7dd245c9c52151ec9ef27edd9ad60`.
No reference or game code was executed for this analysis.

## Reference mechanisms and unsafe address leads

`features/MotionVectorFixes/src/MotionVectorFixes.cpp` has three distinct mechanisms:

- Cache the player's existing object world transforms before OnIdle_UpdatePlayer,
  then restore those cached values into surviving objects' previousWorld afterward.
  This does not create a previous bone palette.
- Reset object history after a sequence reposition's actual NiAVObject::Update call.
- Before lighting pass-list construction, reset frozen/menu or selected LOD history,
  excluding LoadingMenu. This is reference policy, not proof that every ordinary draw
  needs current-as-previous data.

The older 1.11.221 address library was read only as a discovery aid. All three raw
addresses land inside different current unwind ranges and are unsafe hook targets:

| Reference identifier | Old address | Current independently resolved target |
| --- | --- | --- |
| OnIdle_UpdatePlayer, 2228929 | C34C10 | C34FA0 |
| SetSequencePosition, 2200766 | 4F93D0 | Candidate 4F96F0, with matching sequence logic and call offset below |
| NiAVObject::Update, 2270101 | 16C8170 | 16C8570 |

The differing displacements also rule out applying one global offset adjustment.
The sequence function's semantic name is reference-correlated; its instructions,
call, arguments and return path are direct executable evidence.

## Complete native function boundaries

Windows exception records can split one logical function into several chained
fragments. Treating the first `.pdata` entry as the whole function would truncate
OnIdle_UpdatePlayer to five instructions and the prepass SetupGeometry to 64.
Following UNW_FLAG_CHAININFO gives:

| Entry | Chained fragments | Decoded instructions |
| --- | --- | --- |
| C34FA0, selected player update | 5 | 123 |
| 4F96F0, sequence-position candidate | 2 | 131 |
| 2205540, prepass geometry setup | 10 | 2155 |

The sequence candidate has entry `40 55 41 54 41 55 41 57`. Its call at 4F98C7
is exactly entry+1D7, bytes `E8 A4 EC 1C 01`, targeting 16C8570 and returning
to 4F98CC. It compares the requested sequence name, clamps the supplied position,
sets sequence state and passes the selected object and a stack update record to
that native update. The record has the selected time at +0, zero at +8, value1
at +10 and zero at +18. Its full native type and all callers remain separate proof.

At 16C8570, native Update resets update-record fields +12/+14, invokes this
object's virtual slot30, then conditionally invokes its parent object's slot42.
Entry bytes are `48 89 5C 24 08 57 48 83 EC 20`. The second call belongs to
parent+28, not a new shader/history consumer on the original object.

Root obtained actual REA/IDA pseudocode and 123 assembly instructions for
C34FA0, parent evidence
`ev_693271f2a7b1ff1b64d98011576f065ae6961681747c522caa4f69c07b4953fb`.
It agrees with independent chained-range decoding: Main menu/freeze gates,
PlayerCharacter global32DD370, native update calls and actor virtual dispatch.
The decompiler's inferred integer return and incomplete typed references do not
prove a public ABI or every indirect target. REA's IDA profile does not supply
typed direct-caller, basic-block or value-flow evidence. The session was closed
with its retained snapshot after analysis; no live game was attached.

## Native transform producer and shader consumer

Exact MSVC type descriptor, COL self/type references and primary-vtable pointers
resolve NiAVObject vtable26851F0, NiNode26849B8, BSGeometry26861E8 and
BSLightingShaderProperty29037B8. Known slots establish the following native paths:

- NiAVObject slot34 is 16C8AC0. Its reached base update block at 16C8AF7..16C8BC9
  copies all 16 four-byte words from object+70..AF to object+C0..FF before computing
  the new world transform from local/parent data. The retained scalar verifier
  checks every source/destination pair. This is a real previous transform producer.
- A collision-object branch delegates or returns before that block. Its behavior
  is not covered by the base copy proof. Repeated updates can replace history at
  each reached base update, but this analysis does not prove that a visible object
  is updated repeatedly in one rendered frame or that its history is wrong.
- BSGeometry slot34 at 16D5A00 calls 16C8AC0. Derived paths and their flags retain
  distinct behavior; a base-class result is not universal geometry coverage.
- BSLightingShaderProperty slot2B at 217A050 is the native pass-list boundary
  corresponding to the reference's decimal slot43 callback. This is before pass
  construction, not an SR callback.

Prepass shader RTTI resolves vtable29157A0 slot7 to 2205540, corresponding to
BSShader::SetupGeometry. The native pass supplies property at +10 and geometry
at +18. In the reached descriptor-bit2-clear branch, current transform comes
from geometry+70; 2205B7B and 2205BB0/BB8/BC0 read previous translation/scale
and rotation from +F0/+C0/+D0/+E0. Reflected metadata +69/+6A selects separate
current/previous destinations, and 2205C68..2205CC1 writes the assembled matrices.
Missing reflected slots use the native scratch path. The actual selected shader,
reflection values, bound constant-buffer slots and GPU upload remain live facts
to verify, not assumptions supplied by these offsets.

This connects the native history fields to the real prepass constant producer.
It does not prove the instanced/skinned branches, previous palettes, material
motion-write coverage or complete current/previous camera conventions. Preserve
the existing two native bone palettes and the audit's separate instance, BLEND,
particle, tessellation and compute-generated geometry work.

## Smallest remaining evidence before a repair

Capture two adjacent moving evaluated sources with the actual geometry identity,
native frame, shader/descriptor and current/previous transform at setup, plus
the exact update sequence affecting that object. For player-specific work, prove
the live PlayerCharacter/Get3D owner, node-child storage and object lifetime across
native update before caching pointers or writing previousWorld. A discontinuity
reset needs its actual native event and scope; reference menu policy is not enough.

If repeated native updates overwrite a required prior pose, repair that producer
with bounded game-owned history and validated object lifetime. If the native
history is already correct, preserve it. Neither resetting every object to current
nor copying current bones into a previous palette is an acceptable substitute for
independent motion. No production repair is justified solely by stationary captures.

Ignored evidence is frozen in `motion-native-history-proof-freeze-20261116.json`,
including exact RTTI entries, complete chained ranges, assembly, all 16 copy pairs
and artifact hashes. `rea-motion-native-root-snapshot-20261116.json` retains the
actual REA dossier. This work changed no production source, resource, option,
capability or game installation and ran no build/device/game test.
