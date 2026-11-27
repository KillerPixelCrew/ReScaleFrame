# AC7 velocity geometry shader assembly

The source port starts from `37dc694`, which includes the Rust vertex
stage. It uses the existing DXBC container and operand codecs.
The [native producer evidence](ac7-velocity-20261013.md#native-geometry-shader-producer)
records why the rewrite keeps the position in a temporary and uses each
emission's ordinal in the history key.

- The history GS still compiles the fixed triangle prototype once with
  `gs_5_0`. The Rust linker maps the prototype to the current VS signature,
  forwards every native varying at each of the three emissions and appends
  the current and previous clip outputs. The old prototype initializer
  discarded the return value of its separate layout check. The linker
  continues to use the prototype's actual compiled signatures.
- The native GS path still admits a triangle input, stream zero, a single
  instance and a declared output limit of 1 to 1024. It checks the native
  bindings for conflicts with `b13` and `t127`. The original geometry-stage
  container supplies all chunks. Rust replaces only the instruction and
  signature payloads and rehashes the container with the existing codec.
  The `vs_5_0` helper keeps its original register rebasing and runs before each
  native emit or emit-and-cut. The original 4096-temporary ceiling includes
  the helper, the ordinal and the held position.
- C++ owns both compiler caches, the public result vectors and the semantic
  strings. The history compile counter still counts its initial attempt.
  The native wrapper holds its incoming bytes and semantic names through the
  Rust call, including when the caller uses the same vector or metadata
  object as a destination. It publishes bytecode and metadata together and
  keeps the existing scalar progress outputs on refusal.

Rust rejects malformed link data before emission. In the old history GS the
input check rejected a zero mask but allowed high mask bits into
`0x00201002 | (mask << 4)`. The linker now requires the four-bit mask used
by the operand token. Register counts use checked arithmetic and mapped
signature registers must fit the 32-register interface. The old native linker
could write an upstream out-of-range register or a helper triangle index
above two into a new operand. Both values are checked before that edit.
Missing register mappings still refuse, as they did through `std::map::at`.

Source comparison recovered byte-identical HLSL for both compiler calls,
matched compiler names, entry points, profiles and emitted token literals,
and confirmed the same hull and domain assembly, native replay call sites,
public header and existing fixture. Rust formatting, locked Cargo metadata
and diff checks passed. Compilation, linkage, emitted-bytecode comparison,
replay and game results remain unverified for this port.
