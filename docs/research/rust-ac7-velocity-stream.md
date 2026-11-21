# AC7 velocity-pass stream policy in Rust

Base: `31a06a94` on `NightHammer1000/rust-ac7-velocity-stream-policy`. This is a source comparison
of [velocity_pass.cpp](../../games/ac7/src/velocity_pass.cpp) before and after the port. Nothing
was built, linked or run for it. It continues
[the model and view history port](rust-ac7-velocity-history.md).

## Question

Can the translucent velocity pass's remaining per-draw CPU policy (scope and draw admission, the
pipeline route, record bounds and packing, capture and zero-source sizing, the indirect argument
range, the hash build's count and dispatch, and the b13 draw parameters) move into `rsf-ac7`
while the D3D11 calls, buffer descriptions, map and copy work, HLSL, draws and merge stay native
and the public C entry points keep their identity?

## Traced paths at the base

| Path | Behavior at `31a06a94` |
| --- | --- |
| Draw admission | Return unless the scope is `RSF_GAME_RENDER_TRANSLUCENT_MESH` with `camera_valid` and `RSF_GAME_RENDER_PRIMARY`, the context is non-null and immediate, depth is bound, `samples == 1`, `kind <= 6` and a direct kind (`< 4`) has a nonzero count. |
| Route | After reading GS, HS and DS: HS without DS, DS without HS, or HS with a native GS logs the tessellation refusal. Without HS only `TRIANGLELIST` (4) and `TRIANGLESTRIP` (5) continue. |
| Record bounds | After the variant: with HS the topology must be `33 + points - 1`. Input vertices are `geometry_capacity * (HS ? points : 3) / 3` for kinds 4 to 6, else `count * (kind >= 2 ? instances : 1)`, in 64 bits. Expansion is the variant's maximum vertices, times 3 for strips. Zero vertices or more than `(UINT32_MAX / 128) / expansion` return; records are vertices times expansion. |
| Indirect | Kinds 5 and 6, after state save and predicate capture. Packing is 4 with a native GS, 3 with HS, 2 for strips, else 1. The source buffer must exist, capacity be nonzero, the 20-byte (kind 5) or 16-byte range lie within `ByteWidth` from a 4-aligned offset and the buffer be on the pass device. The range is copied and `{vertices, records, packing, points}` uploaded for the validation shader. |
| Capture and zero source | A missing or smaller capture buffer is created with the smallest power of two from 1024 that holds the records. With a native GS or DrawAuto a smaller zero source grows to the same capacity and is cleared, then `records * 64` bytes are copied over the capture. |
| Draw parameters | One viewport with `Width > 0` and `Height > 0` (NaN passes), else return after the capture draw. Rect is the viewport; jitter is `(j.x * 2 / W, j.y * -2 / H)` for current and previous; cutoff is -1 for AlphaComposite (5), else `opacity_clip + 1/255`; history is `usable` and a previous buffer; mask is the previous capacity minus one or 0; the matrix is `clip_to_previous_clip`. |
| Hash build | Direct fixed producers build `(count / points) * 3 * instances` for HS, `(max(count, 2) - 2) * 3 * instances` for strips, else the input vertices. Native GS, DrawAuto and indirect draws build the record bound. Zero returns; groups are `ceil(records / 64)`, width `min(groups, 65535)`, the cbuffer is `{records, capacity - 1, mode, dynamic ? 0 : width}` and the direct dispatch `width x ceil(groups / width)`. The rehash on growth uses the same build. |

## Moved

[velocity_stream.rs](../../games/ac7/src/velocity_stream.rs) now owns, through
[velocity_stream.h](../../games/ac7/src/velocity_stream.h):

- `rsf_ac7_velocity_stream_draw`: the scope, context, depth, sample, kind and count admission.
- `rsf_ac7_velocity_stream_route`: the stage combination refusal and the non-tessellated
  topology filter, from a stage mask.
- `rsf_ac7_velocity_stream_plan`: the patch topology, input and record bounds against
  `RECORD_LIMIT`, the packing, the capture and zero-source capacity from `MIN_CAPACITY`, whether
  the zero source is needed, and the hash build's record count. Both constants are shared with
  `velocity_history.rs` and have the same values as before.
- `rsf_ac7_velocity_indirect_end`: the argument size per kind and the range and alignment check.
- `rsf_ac7_velocity_draw_parameters`: the viewport check and the full 112-byte b13 layout.
- `rsf_ac7_velocity_hash_dispatch`: the hash cbuffer and the direct dispatch dimensions.

`velocity_pass.cpp` keeps the immediate-context check, the shader queries, renderer creation,
variants, the state save and predicate, the argument buffer's device check, copy, upload and
validation dispatch, every buffer and view description, allocation and clear, the zero-source
copy, both draws, the hash dispatch, the merge, profiling and every refusal message. Buffer bind
and misc flags, raw view element counts, the 64-byte record stride, the 36-byte indirect dispatch
offset and the HLSL are unchanged. The public calls in `ac7_velocity_pass.h` are unchanged.

## Deliberate differences

- The pure draw checks now run before the immediate-context type check instead of interleaved
  with it. `GetType` has no side effect, so the outcome is the same.
- A tessellated variant with zero control points is refused explicitly. Before, its required
  topology was 32, which D3D11 never reports, so the same draw returned at the same point.
- A zero or overflowing expansion is refused. Before, it divided by zero, which `catch (...)`
  does not catch. No current variant reports zero maximum vertices.
- The history flag tests the previous capacity instead of the buffer pointer. `raw_buffer` sets
  both together and a reset model clears both, so they agree.

## Checks

Source comparison against `31a06a94` of every former use of `count64`, `count`, `expansion`,
`packing`, `records`, `capacity`, `Parameters` and the hash and indirect cbuffers. `cargo fmt -p
rsf-ac7` with the pinned toolchain, locked offline Cargo metadata, and Git whitespace. The Rust
tests in `velocity_stream/tests.rs` cover scope and draw admission, every route, list, strip,
patch, native-emission, DrawAuto and indirect plans, the record limit and overflow refusals,
capacity doubling, the argument range, parameter packing including AlphaComposite and NaN
extents, dispatch splitting and null pointers. They were not run. Build, link, Cargo tests,
Clippy, CTest including `ac7_velocity_shader`, deployment and game runs are user-owned and not
done.

## Remaining in C++

The commit's native depth, format and device checks and its merge, CPU profiling and its log,
the device renderer lifecycle and variant construction, and every D3D11 call and HLSL source.
