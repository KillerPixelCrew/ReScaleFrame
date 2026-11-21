# AC7 velocity hull and domain shader assembly

The Rust port starts from `fa2321a81a306a0de42a2529ba3a57e2c3a84351` on
`rust/orchestrator-anomaly`. It moves the remaining velocity stage assembly
into `rsf-ac7` and shares the geometry linker's DXBC stage helpers. The
[native tessellation research](ac7-velocity-20261013.md#tessellated-material-velocity)
records the producer, the prior WARP fixture, and the retained 12-input,
3-output PN shader evidence.

- **Hull.** Rust reads the native `ISGN`, `OSGN` and `PCSG`, then relays
  `RSFKEY`, `RSFLOOKUP`, `RSFCENTRE` and `RSFOLD` by the native output
  control-point ID. An explicit control phase keeps its native body and
  receives the relay before each return. For an optimized identity hull
  shader, the first fork or join gets a preceding control phase that copies
  by semantic name and index. The ordinary variant keeps native factors and
  partitioning. The corner variant selects integer partitioning and writes
  `1.0f` only to the tessellation-factor components declared by the current
  phase. Both retain the original patch-constant signature payload.
- **Domain.** The native `b13`, `t127`, triangle, control-point and return
  guards still precede helper compilation. The native `OSGN` supplies the
  last full float4 system-position register and the starting register for
  the helper outputs. The native sample preserves material varyings and
  restores its original displaced position. The three corner evaluations
  use private temporaries and one-hot domain coordinates. The operand codec
  edits nested indices on the native path. The helper receives domain
  location with zero indices, corner positions as one-index temporaries,
  and hull data as two-index control-point inputs in point/register order.
  Its current and prior clip outputs use the existing corner interpolation.
- **Native boundary.** Both wrappers hold borrowed DXBC and semantic names
  until Rust returns, including when the caller reuses input storage for a
  destination. They publish bytecode and semantic metadata together and copy
  the upstream particle tag. The metadata position field keeps its prior
  default. Hull input points start at zero and are reported after signatures
  and before point bounds or relay checks. The domain clip scalars are
  written in helper signature order during output mapping, so their prior
  values survive early refusals and reported values survive later refusals.
  Both shader stages replace only the selected `SHEX` or `SHDR`, `ISGN`
  and `OSGN` payloads and use the existing container checksum.

The shared linker now refuses an output register that would wrap while
reserving new slots. It also refuses out-of-range registers used in new hull
declarations or control-point operands, zero or wider-than-four-bit masks in
synthetic hull copies, and helper indices outside the three corners or the
register map. The compiled helper determines the output order and the first
matching upstream semantic still wins. The existing signature query retains
`OSGN` before `OSG5`, stream-zero handling and partial results if a later
record refuses. It was the remaining consumer of the native container
reader. The port traced the repository call sites before retiring the
private container, signature and token codec bridges.

Source comparison recovered the exact 2,516 HLSL bytes with SHA-256
`fe6de2a48ce9ef70687a4f50977096177b77c15d09c6fca81742798aa9fb67e4`.
The C++ cache still lends the `ac7-tessellated-control-motion` shader compiled
by System32's D3DCompiler 47 with `main`, `vs_5_0` and default strictness.
The hull and domain hexadecimal word sets match the base. Reused literals
serve the same declaration sites and the domain corner move places the
original `1.0f` word in the selected component. The public header, replay
caller, native compile implementation and existing fixture match the base.

Pinned Rust formatting, locked Cargo metadata and whitespace checks passed.
Compilation, linkage, emitted bytecode parity, replay and game acceptance
remain open for this port. The older WARP and retained-shader evidence applies
to the original path. Motion from interior-only deformation remains limited
to the original corner correspondence, and combined native GS/tessellation
remains outside this path.
