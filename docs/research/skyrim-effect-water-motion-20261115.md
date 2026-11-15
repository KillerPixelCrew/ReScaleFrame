# Skyrim Effect and Water motion coverage, 15 November 2026

Question: which remaining native shader permutations carry independent motion,
and which require a repair through the shared shader replacement service?
An exhaustive bytecode census separates missing interfaces from intentional
non-motion passes before another power-consuming game experiment.

The input is the previously extracted Skyrim 1.7.104 shader archive,
`shaders011.fxp`, SHA256
`72cfc73eb0938d5949f0b3a8683722a33b671a34b41b750ba138de8d154c5fbc`.
The executable fingerprint remains the one in `games/skyrim-se/engine.json`.
Ignored reproduction material is
`.local/skyrim-research/audit-effect-water-inputs.py` and
`effect-water-inputs/manifest.json`. System32 D3DDisassemble succeeded for all
8,686 containers. The manifest retains family, descriptor, offset, hash,
constant-buffer declarations, output writes and representative assemblies.
No game, GPU fixture or production shader mutation ran for this census.

| Family | Native VS / PS | Bytecode evidence | Remaining proof |
| --- | --- | --- | --- |
| Effect | 606 / 3,216 | Exactly 264 VS export POSITION2; all have descriptor bit26 set. Of these, 129 read both previous b9 and current b10 bones; 135 use no bone buffers. The other 342 VS lack POSITION2, including 145 using only current b10 bones. Exactly 1,543 PS have bit26 and compute target1 XY with the native motion scale. All 1,673 PS without bit26 write literal target1 XY(1,0), while alpha remains computed. | Which descriptor and target/write-mask combinations execute on the primary motion surface; actual previous object/bone data and moving coverage. |
| Water | 2,172 / 2,172 | Only descriptors20480..20483, technique10, export POSITION1/POSITION2 and target1 XY. Their VS use current cb2[0..3] and previous cb2[4..7] transforms. Their PS compute the native motion scale. The other 2,168 variants per stage have neither that previous-position interface nor a target1 output. | Actual stencil pass coverage, depth/ordering, animated water and reflection/refraction ownership. |
| Utility | 339 / 177 | No POSITION2 VS output and no target1 PS output in this family. | Native pass purpose and target ownership. Absence alone does not establish a primary colour/motion defect. |
| BloodSplatter | 2 / 2 | No POSITION2 VS output; PS output only target0. | Camera-overlay ordering and exclusion from scene motion, rather than inventing world motion. |

The reference checkout is Community Shaders revision
`33d7896147b54ca72d1ee5ca938bb85739593167`. Its `ShaderCache.h` names Effect
bit26 `MotionVectorsNormals` and Water technique10 `Stencil`.
`ShaderCache.cpp`, `Effect.hlsl` and `Water.hlsl` agree with these interfaces:
previous skeletal transforms are selected only for the Effect motion path,
and Water motion belongs to the stencil permutation. This source comparison
helps interpret the native evidence; no reference implementation was copied.

Matching IDA session0044e197 also establishes the native Effect selector.
The primary Effect vtable is RVA0x1b48ab0, reached from constructor0x1556690.
Its technique function0x1556840 subtracts0x4000002c from the incoming technique,
normalizes the vertex/pixel descriptors through0x158dab0/0x158dac0, and calls
the native shader lookup0x1579120. Vertex normalization clears mask0x00380000;
pixel normalization conditionally clears skinning bit3 and clears bit24.
Both preserve motion bit26. Thus replacing only a PS cannot create the missing
previous-position interpolator in a non-motion VS. This is static evidence of
selection, not a receipt for an active draw.

The literal Effect(1,0) finding narrows the next runtime question. Current native
motion input configuration uses invalid sentinel-1000; treating all(1,0) samples
as invalid would also discard legitimate large displacement and would not prove
an Effect repair. First correlate native family/descriptor/hash with the actual
motion RTV, blend/write masks, source/view and indexed draw. If a non-motion
Effect path really overwrites primary motion, repair its producer and matching
VS/PS interface at native selection/binding, retaining real previous transforms
and bones. The shared same-length DXBC instruction replacement cannot add these
interfaces or history buffers. A shader replacement pack must retain exact
input hashes, native auxiliary exclusions, resource leases and an overlay control.

This extends the earlier selected literal-write census to an exhaustive
descriptor/interface census for these four families. It does not establish a
new live defect or justify replacing auxiliary shaders. No missing-vector repair
or complete animated-material acceptance is claimed.
