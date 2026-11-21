# AC7 F9 ordinary draw records in Rust

The source comparison starts at `31a06a94c9d96ed1da7fc96a5b46aa38465857bc`, after the
[dispatch record port](rust-ac7-motion-dispatch.md). Ordinary draws already entered the Rust
draw stream as complete native strings. They now enter as a bounded stack record of the same
observations. The native callback lends the record synchronously and releases its D3D11 objects
locally. Rust formats the row through the existing session and publication owners.

## Sources and order

| Row fields | Producer and meaning |
| --- | --- |
| `kind`, `qpc`, `interval`, `thread`, `native_scope`, `draw_index`, `native_view` | The post-draw tap takes the sampled session stamp. The native observer queries GS, HS and DS, then copies the thread-local root scope, QPC ticks and Windows thread ID. The draw index is the tap's ordinal since the target bind. The interval is Present age at the callback, sampled at 0, 30 and 59. |
| `target`, `format`, `extent`, `targets`, `depth_bound`, `indexed`, `elements`, `layout`, `topology`, `stride`, `viewport` | The frame tap supplies the draw facts, including slot-zero stride and viewport-zero origin and extent. The origin uses the existing nine-significant-digit float writer. It preserves finite values and represents a nonfinite origin as JSON null. |
| `vs_hash`, `ps_hash`, `gs_hash`, `hs_hash`, `ds_hash` | The creation cache resolves the tap's VS and PS pointers and the live GS, HS and DS pointers. Missing shaders keep hash zero. The original shader files retain the stage, ordinal and hash naming scheme. |
| `inputs`, `ps_resources`, `vs_resources`, `render_targets`, `depth` | The tap provides up to 16 shadow PS inputs in their original order. The observer then queries the 128 live PS SRV slots, 128 VS SRV slots, and eight output slots plus depth. Live arrays omit unbound entries and keep ascending slot numbers. Nested views describe the underlying allocation. Texture2D includes width, height and format. Texture3D also includes depth. Other dimensions retain only the address and dimension. An absent DSV produces null. |
| `snapshot`, `snapshot_result` | Native output-slot reads keep the existing shader roles, target size filters and signed dump results. The cloud composite can report colour at slot 0 and loss at slot 1. The contact-light after read uses slot 0 and its reserved quota. The basename stays `f<interval>_stage_<role>_<ordinal>` with three digits for the interval and capture-wide ordinal. |
| `vs_cb`, `ps_cb`, `gs_cb`, `hs_cb`, `ds_cb` | The observer visits each active stage in order using the same tap CB arrays, declaration masks and upload or live-read selection. Entries keep ascending slot order, buffer address, one-based blob ID, upload serial and numeric readback flag. Native blob storage still deduplicates exact bytes. A row can be refused after its blob has been saved. |
| `rhi_command`, `rhi_execute`, `queued_native_scope`, `queued_native_view`, `queued_native_family`, `queued_native_frame`, `queued_list`, `queued_uid` | The existing queued lookup still runs after CB collection. Root scopes and queued associations describe CPU observation. GPU completion and Present identity need separate evidence. |

The frame tap observes the draw after forwarding and after restoring temporary constant
overrides. Native capture still owns typed querying, texture dumps and buffer readbacks.
[Frame tap](../../runtime/orchestrator/src/frame_tap.rs) ·
[Native capture](../../games/ac7/src/motion_capture.cpp) ·
[Upload audit](rust-ac7-motion-uploads.md)

## Admission and publication

The shared 12,000-row cap still applies to ordinary draws, dispatch, geometry and before-draw
diagnostics. Ordinary draws preflight before D3D11 queries and commit against the same active
session. A commit can retain its original interval after Present advances within that session.
Rows stamped with a retired session are discarded before counting cap drops. The final draw
stream keeps append order and one LF per row.

The Rust observation owner now keeps the contact-light pairing state and colour snapshot quotas.
The first contact-light before callback in a sample arms the after callback for the same sample.
Colour admission checks the exact interval before querying for a 2D texture and again when
reserving its ordinal. The shared limit remains 48 reads, with up to 72 total for the reserved
lighting path. A failed read still consumes its ordinal. Its failure counts while the same
session is active, even after the sampled interval advances. Publication reads those counters
from the observation owner and keeps the existing `session.json` keys. The native guard ties a
reserved ordinal to the session directory. The GPU read and file write run after admission
releases the observation lock.

## Verification

Compared ordered draw, view, CB and queued keys against the pinned source. Two archived
capture I/O fixture sessions contained six ordinary draws, and their rows matched the key order,
manifest counts and numbered CB references. The offline analyzer still sees a `draw` kind on each
ordinary row. Rust 1.94.1 formatting, locked offline Windows Cargo metadata and Git whitespace
checks passed. [Offline analyzer](../../tools/analyze-ac7-motion-capture.py)

Geometry and the cloud and contact-light before-draw JSON writers remain native. Compilation,
linking, Clippy, tests, comparison of newly emitted capture files, device and game verification,
deployment and push remain user-owned.
