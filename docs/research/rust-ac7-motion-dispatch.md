# AC7 F9 dispatch records in Rust

The source comparison starts at `1eda132ede6ac25ac5d30f7a304782fed5ef139e`. It follows the
dispatch callback because it exposes a complete row of bounded renderer observations and already
uses the Rust [shader, upload and constant-read owner](rust-ac7-motion-uploads.md). The port moves
dispatch JSON formatting and the shared draw-stream admission into that capture's Rust record
owner. The [session](rust-ac7-motion-session.md) and [artifact](rust-ac7-motion-artifacts.md)
audits describe the existing capture boundaries.

## Dispatch producers and file fields

| File fields | Source and order |
| --- | --- |
| `kind`, `groups`, `indirect`, `offset` | The [typed frame tap](../../runtime/graphics/src/frame_tap.cpp) forwards the original D3D11 command before reporting it. Direct calls give group counts, null indirect identity and zero offset. Indirect calls give three zeroes, the argument buffer address and the argument byte offset. The recorder does not read the indirect argument values. |
| `qpc`, `interval`, `thread`, `native_scope`, `native_view` | The [tap admission](../../runtime/orchestrator/src/frame_tap.rs) restricts reporting to the installed context. F9 takes the sampled session stamp at the callback, queries the active CS, then copies the current thread's root scope before sampling QPC ticks and Windows thread ID. The interval is Present age, with callbacks sampled at 0, 30 and 59. |
| `cs_hash`, `inputs`, `outputs` | After the clock sample the [native capture](../../games/ac7/src/motion_capture.cpp) resolves the active CS through shader observation stage 5. It queries 128 CS SRV slots, then 8 CS UAV slots. Each array contains only bound slots in ascending order. The nested `view` keeps the underlying resource address and D3D dimension. Texture2D adds width, height and DXGI format. Texture3D adds width, height, depth and DXGI format. The values describe the allocation and carry no view range. |
| `cs_cb` | The native leaf next queries 14 CS buffer slots, uses the shader's declaration mask and visits selected non-null slots in ascending order. The all-slot fallback, latest upload span and serial, and live-read attempts follow the existing constant observation owner. Each object retains `slot`, `buffer`, `blob`, `upload_serial` and numeric `readback`. A live attempt keeps `readback:1` even when the read fails. |
| `rhi_command`, `rhi_execute`, `queued_native_scope`, `queued_native_view`, `queued_native_family`, `queued_native_frame`, `queued_list`, `queued_uid` | After releasing the queried constant buffers, the native leaf reads the command and execute identity through the existing guard. Rust resolves the candidate association and prefers the existing thread-local execution-pass fields where applicable. The queued native frame field retains its 32-bit counter. Root and queue identities establish neither GPU completion nor a matched Present. |

The private ABI copies numeric facts into a native stack record and lends it synchronously.
Only native code calls COM or reads GPU buffers. CB identity, selected byte source and read
attempt are captured during the callback. Native `save_blob` deduplicates exact bytes before
applying the shared 4096-entry and 32 MiB limits. IDs are one-based append positions shared
with graphics, geometry and native root captures. Absent bytes or a refused insertion give
ID zero. The final writer emits `cb_<id>.bin` in that order, and native checkpoints retain the
same blob IDs. A blob can survive a later row refusal because the read and blob insertion happen
before final row admission.

## Row admission and readers

Rust uses one 12,000-row cap for ordinary draws, dispatches, geometry and before-draw diagnostics.
Ordinary draws and dispatches preflight the cap before the D3D queries. A preflight does not
reserve a row. Every row checks the active session and space again at commit. It can retain its
original interval if capture progressed in the same session while the observer did its work.
Rejecting a full active stream counts once; a retired session stamp is ignored. The record lock
serializes appends and the finish snapshot, and the session resets the rows and their drop count
before publishing the next active stamp. Typed graphics work and file writes occur outside that
record lock.

Final publication writes one LF per row to `draws.jsonl` in append order. The manifest uses
that snapshot for `draw_records` and `draw_dropped`. File attempt order remains engine rows,
native roots, draw-stream rows, numbered blobs, original DXBC, then the manifest. Shader files
keep the creation-cache bytes, stage 5 for CS, the ordinal in the full shader list and the
unsigned decimal hash used by `cs_hash`.

The [offline analyzer](../../tools/analyze-ac7-motion-capture.py) reads `session.json` and parses
each line of `draws.jsonl`. It counts the `draw` and `dispatch` kinds and counts shader stages
from the DXBC names. It does not resolve CB blobs or infer deferred commands or Present identity
from dispatches. The [capture I/O fixture](../../tests/ac7_capture_io.cpp) reads the same JSONL,
checks the original kinds and resolves numbered blob payloads. Paired backend reports and raw
files still use the sample prefix documented in the session audit.

## Native formats and verification

C++ still formats ordinary draw, geometry, cloud before-draw and contact-light before-draw
rows. Those rows enter the shared stream as bytes. Native also keeps geometry buckets, all
texture readback and file formats, colour capture admission, shared blob storage and checkpoint
copying. The capture still observes the existing frame tap and uses the current shader cache,
with no new hooks or shader generation.

Compared the dispatch fields and source order, shared row cap, blob paths and artifact schema
against the pinned source. Rust 1.94.1 formatting, locked offline Windows Cargo metadata and
Git whitespace checks passed. The user owns compilation and linking, Clippy, tests, captured
output comparisons, game and device checks, deployment and push.
