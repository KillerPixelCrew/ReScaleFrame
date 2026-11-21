# AC7 velocity shader token ownership

This source port starts from `6a0b10f`, with the DXBC container and signature
codec from `a4b44b6`. The question was which instruction work can move for all
five shader stages without changing the supported material paths or the typed
D3DCompiler boundary. The earlier [velocity research](ac7-velocity-20261013.md)
records the shader shapes, prior captures and WARP coverage used by those paths.

`rsf-ac7` now reads and writes the SHEX/SHDR instruction stream and decodes
operands for every transform. The native transform passes keep decoded
instruction boundaries when assembling a new body. The decoder distinguishes
ordinary instruction length from the full-word length of custom-data records.
It decodes instruction and operand extensions, 32-bit and 64-bit literals,
and immediate and relative index representations. Rust applies the selected
register edits in reverse token order, retaining the operand selection and
extension words. A native domain evaluation can also request edits to nested
operands, as it did before. Output uses the existing Rust signature and
container codec, shared chunk bounds and DXBC checksum.

The old stream reader could admit a one-word custom-data record, while replay
unconditionally read its second word. The reader now requires the length word
and the full record. It also rejects malformed or oversized temporary
declarations. PS assembly now checks the sum of native, colour and helper
temporaries against 4096. A requested edit requires the expected count of
32-bit immediate indices. This avoids treating a relative or absent index as
an absolute register offset. Converting a domain location to a temporary places
the new index after any operand extensions. A length-changing edit must still
fit the 127-word ordinary instruction limit.

The remaining C++ consumers in `games/ac7/src/velocity_shader.cpp` select edits,
assemble stages and write stage-specific declaration and instruction literals.

| Entry point | Remaining C++ logic |
| --- | --- |
| `rsf_ac7_velocity_pixel_shader` | The C++ vector and typed compiler cache. `velocity_pixel.rs` now owns admission, additive opacity, register mapping and return-time assembly. |
| `rsf_ac7_velocity_vertex_shader` | Generic, sprite, mesh and beam key helper selection, stage signature and declaration linking, dummy-position suppression and return-time injection. |
| `rsf_ac7_velocity_geometry_shader` | Compile and cache the history GS template, link its signatures, match position moves to three emissions and copy packed native varyings. |
| `rsf_ac7_velocity_native_geometry_shader` | Admit the native triangle GS and its bindings, map history-helper inputs, and inject clip position, ordinal and history work at emissions. |
| `rsf_ac7_velocity_hull_shader` | Admit the control-point layout, relay the key by control-point ID, materialize an implicit control phase when needed, and emit native-factor and private corner variants. |
| `rsf_ac7_velocity_domain_shader` | Admit a triangle domain and its bindings, map the corner helper and assemble the native sample plus three private corner evaluations. |

The HLSL, typed compile and shader creation paths, signature conventions,
reflection consumers and replay policy stay with the existing callers. The
decoder is for the existing SM5 transform paths. It frames opaque native
custom-data records where those paths already carry them through, and refuses
to edit a helper custom-data record as operands. It has no general opcode
grammar for validating declaration payloads or reconciling linked immediate
constant buffers. Existing stream, topology and stage restrictions still
apply. The broader motion coverage and gameplay acceptance gaps are recorded
in the velocity research.

The pixel-stage follow-up reused the existing Rust codecs. Source comparison matched its helper
HLSL, compiler arguments, stage entry and emitted token literals. The native wrapper keeps aliased
input alive while Rust writes a separate result, and Rust refuses a stream with no unconditional
return. Emitted bytecode parity and replay still need verification.

Validation for this port is source-only. HLSL literals, compiler call sites,
stage entry points, token emission sites and bridge declarations were compared
against the base. The Rust source was checked with rustfmt and the patch with
`git diff --check`. Build, linkage, shader replay and GPU validation are open.
