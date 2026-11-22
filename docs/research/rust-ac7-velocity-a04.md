# AC7 velocity shader transforms

A04 started from `3cadb40a`, with the Rust stage transforms, shared System32
compiler and AC7 capture code already in place. The question was which remaining
shader decisions belonged in Rust and which D3D11 calls needed native ownership.
Tracing the calls from `Renderer::make_variant` through the linker and the history
compute dispatch exposed a native round trip for hull signatures and independent
copies of the GPU table format. This work used source evidence. The existing
[geometry](rust-ac7-velocity-geometry.md) and
[tessellation](rust-ac7-velocity-tessellation.md) audits record the upstream
shader evidence.

## Closed PORT findings

| Finding | Implementation |
| --- | --- |
| Share the GPU history hash, record layout and lookup across AC7 velocity shaders. | `games/ac7/src/velocity_history_table.hlsl` defines the hash, 32-probe addressing and 64-byte table records. `velocity_history_lookup.hlsl` defines clip validation and nearest unambiguous selection, with the stage choosing t0 or t127. The geometry, domain and writer sources use the common table. Emission ordinals, tessellation corner IDs and beam interpolation retain their stage-specific logic. |
| Share the geometry and domain helper-instruction rewrite. | `games/ac7/src/velocity_stage.rs` handles declarations, output mapping and temporary rebasing. Geometry and domain pass distinct input mappings and decide where to place the resulting body. |
| Keep intermediate shader signatures inside the Rust stage linker. | `games/ac7/src/velocity_tessellation.rs` keeps the hull output entries borrowed from the source containers and links the domain stage before publishing the three final DXBC products and domain outputs. `velocity_shader.cpp` and `velocity_pass.cpp` use the combined entry. |
| Classify SM5 declarations consistently before splitting shader bodies. | `games/ac7/src/velocity_tokens.rs` recognizes SM4 and SM5 declarations, including raw and structured SRVs, independently of hull phase markers. Geometry retains these SRV declarations ahead of its first executable instruction. |
| Use the common DXBC stage reader and writer for AC7 vertex transforms. | `games/ac7/src/velocity_vertex.rs` uses the stage codecs with its OSGN choice and 32-register bound. `velocity_pixel.rs` uses the same writer with only code and input signature substitution. |
| Share traversal of the DXBC chunk offset table. | `runtime/graphics/src/dxbc_chunks.rs` owns checked traversal. `games/ac7/src/velocity_container.rs` keeps declared-length slicing, trailer acceptance and its 32-chunk bound. `runtime/orchestrator/src/shader_patch/dxbc.rs` keeps exact length, hash, checksum and its existing refusal categories. |
| Reuse the velocity-stage compiler callback adapter for pixel shaders. | `games/ac7/src/velocity_stage.rs` sends compiler requests through the shared Rust System32 compiler's C entry and copies the borrowed bytecode into Rust-owned module caches. All stage transforms use it, including pixel and the four vertex helpers. |

## Port paths and ownership

- `games/ac7/src/velocity_container.h` was removed with the uncalled standalone
  signature export. The borrowed signature view used at the private native boundary
  lives with its consumer in `velocity_shader.cpp`.
- `games/ac7/src/velocity_shader.cpp` and `games/ac7/src/velocity_shader.h` now
  adapt the C++ pass's vectors and stream-output metadata to Rust. A destination
  becomes visible to the pass after linkage completes. The compiler arguments,
  cache ownership and history shader source assembly moved into Rust.
- `games/ac7/src/shader_bytes.h` remains a native allocation leaf shared with
  contact shadows. Rust writes directly to caller-owned vectors so the pass can
  hand the same bytes to D3D11 without a second output copy.
- `tests/ac7_velocity_shader.cpp` retains WARP device creation, shader creation,
  draw and readback, and the optional retained DXBC replay. It exercises the combined
  tessellation call and adds a native GS raw SRV at t126. CPU checks for declaration
  classification and untouched chunk payloads live in the Rust token and stage
  modules.
- `games/ac7/src/velocity_pass.cpp` has two A05 integration points. It compiles the
  shared history writer source and invokes the combined tessellation linker.
  Variant publication, device and renderer lifetimes, history ownership and draw
  or commit decisions remain part of A01/A05.

## Verification

Source checks traced each C entry and caller, compared compiler request names and
profiles, checked shared HLSL addressing and bindings against the earlier embedded
sources, and checked replaced symbol references. The writer preserves b0 and
t0/t1/u0. The history readers preserve b13 and the respective t0 or t127 slot.
Pinned Rust 1.94.1 formatting and `git diff --check` passed. No generated binding
input changed.

Builds, lint, Rust fixture execution and WARP are user-owned. Runtime compilation,
emitted bytecode parity and active game rendering remain unverified on this change.
No executable fingerprint or game shader coverage claim changed.
