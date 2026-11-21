# AC7 F9 geometry rows in Rust

The comparison starts at `2b1af9eb4934048f8ec1eea1536b035046241fa8`. The question was
whether the geometry recorder could share the Rust observation and draw-stream owners
while keeping synchronous D3D11 resource access in the native callback. This continues the
[ordinary draw port](rust-ac7-motion-draws.md). The existing
[AC7 producer evidence](ac7-velocity-20261013.md#coverage-capture-correction-14-october)
explains why declared IA slots, separate candidate buckets and late VS slot 127 matter.

## Callback and admission

The [frame tap](../../runtime/orchestrator/src/frame_tap.rs) reports geometry on the
observed immediate context after the original direct draw and after restoring constant
overrides. DrawAuto and indirect draws reach the geometry callback after forwarding through
the [typed native hooks](../../runtime/graphics/src/frame_tap.cpp). The tap requires a single
colour target, a depth view and an unsubstituted target. It seeds the shadow with native
IA/VS getters and tracks later binds. The [bridge](../../loader/proxy/src/dlss_bridge.c)
calls F9 capture before handing the same geometry to velocity processing.

[Rust admission](../../games/ac7/src/motion_capture_observations/geometry.rs) uses the active
session and exact sample interval, a copied input-layout observation, the stage-zero VS
creation hash, and the native execution role. An R16G16_UNORM target with depth takes bucket 4.
The other candidates require the native translucency or translucent mesh role. Their layout
classifiers keep the same precedence: FLOAT4 attributes 8/9/10 select mesh particles in bucket 2,
FLOAT4 0/2/3/4 with FLOAT3 1 selects curves in bucket 3, and FLOAT4 0/1 selects sprites in
bucket 1. Other admitted layouts use bucket 0. These classifications select diagnostic
samples. Binding semantics and whether the values carry valid history require separate evidence.

The five read limits are 8, 12, 12, 8 and 12, for at most 52 buffer read attempts per
sample at intervals 0, 30 and 59. Buckets 1 through 3 admit each (VS hash, layout address)
once per sample, even if no read succeeds. A new session resets the budgets and deduplication
with the existing observation reset. Read preflight and reservation each recheck the exact
sample. Reservation occurs before staging allocation and consumes the quota even when
CreateBuffer or Map fails. The observation lock covers policy and copied metadata. Native
queries and readbacks run after it releases.

## Bindings, ranges and publication

| Data | Evidence and JSONL contract |
| --- | --- |
| Layout and draw arguments | `layout_count` is the original descriptor count. `elements` contains up to 16 copied descriptors in creation order. The tap supplies kind, count, start, signed base vertex, instances and start instance. DrawAuto and indirect draws use zero count/start placeholders. Indirect draws also expose the argument buffer and offset through the tap. The geometry row records only the existing numeric draw fields. |
| IA vertex and index bindings | The tap shadows 32 vertex buffers, strides and byte offsets, plus the index buffer, index format and byte offset. The row uses only vertex slots declared by the copied layout, in ascending slot order. Native staging reads from the IA binding offset, up to `min(ByteWidth - offset, 16384)` bytes. `source_bytes` is the buffer allocation size and `sample_bytes` is the requested window, even when `blob` is zero. The existing geometry schema has no index-buffer or index-byte fields. |
| VS buffer views | Bucket 2 queries VS SRV 127. Bucket 4 queries VS SRVs 0, 1 and 127 in that order. Native requires an underlying buffer and reads its current view's FirstElement and NumElements, selecting BufferEx when bound. Stride is 16/12/8/4 bytes for the supported typed float formats, 4 for R32_TYPELESS, or the structured buffer stride otherwise. Native rejects zero stride, out-of-range starts and overflowing lengths. The read clamps to the view window, the allocation end and 16 KiB. This array records actual staged byte count before blob admission. |
| PS and other VS bindings | Ordinary post-draw rows already hold shadow PS input facts, live PS/VS SRV slots and stage CB records. Geometry captures the VS hash and selected buffer view slices. The PS-side before-draw observations use their separate callback path described below. |
| Scope and time | Native supplies QPC ticks before the reads. The copied execution scope contributes role, 64-bit native frame, primitive, material and geometry capacity. Existing queued fields are obtained after reads and keep the shared queued scope's 32-bit native frame. These observations do not prove the identity of the presented frame. |
| Blob IDs | The native blob table compares complete byte sequences across geometry, CB and native root snapshots, then returns the one-based ID used by `cb_<id>.bin`. Matching bytes reuse the ID even at the limit of 4096 blobs or 32 MiB. Empty reads and refused saves use zero. Native saves can precede row rejection. |

The [native capture](../../games/ac7/src/motion_capture.cpp) still reads buffer and view
descriptors, temporarily disables and restores predication, creates staging, copies the
bounded buffer range, maps it and saves any bytes. The copied row crosses only the private
records ABI. The [Rust writer](../../games/ac7/src/motion_capture_records/draws/geometry.rs)
retains the existing field order, numeric types and nested arrays, then uses the shared
12,000-row commit check. A late completion can retain its original sample number while
the same session is active. The [artifact writer](../../games/ac7/src/motion_capture_records/artifacts.rs)
adds one LF per draw-stream row and publishes the existing manifest and numbered files.

## Before-draw and analyzer evidence

The paired research callback runs before forwarding Draw or DrawIndexed on the observed
context, using the same target ordinal as the after callback. It reads the live PS inputs
before the draw can change the target. Cloud PS `0x83524e47` records colour, the optional
loss MRT and PS SRVs 0–3. The contact-light shaders, `3619939816` and `4154163049`, record
colour, PS SRVs and samplers 0–5, and the live PS CB bindings at slots 0–1. Those buffers
use the cached upload or the reserved readback path. The 400-by-200 minimum applies to
the allocation extent. Both row writers still use the native text path. The measured
[TrueSky composite](ac7-plane-artifacts-20261002.md#measured-truesky-composite-and-capture)
and [contact-light samples](ac7-lighting-shadow-20261004.md#fresh-capture-isolates-the-directional-light-draw)
record why the pre-draw state matters.

The [offline analyzer](../../tools/analyze-ac7-motion-capture.py) parses every nonempty
`draws.jsonl` line and counts ordinary `draw` and `dispatch` kinds. It leaves geometry,
before-draw bindings and blob contents to separate inspection. The manifest and paired
sample prefixes stay compatible.

## Verification

Source comparison covered ordered geometry keys, individual ranges, shader stages, quota
and reset order, and draw-stream admission. Two archived capture I/O sessions
(`motion-20261116-204547-61220-1` and `motion-20261116-203847-71704-1`) each supplied 11
geometry rows with all 15 referenced blobs within the array's recorded bounds. In each,
mesh slot 127 and native velocity slots 0/1/127 reported offset 16 and length 32 and
shared blob ID 3. The archived data predates this change. Pinned 1.94.1 formatting, locked
offline Windows Cargo metadata and whitespace checks cover this source change.

Compile and link, Clippy, fixtures, new capture comparison, device and game checks,
deployment and push remain user-owned.
