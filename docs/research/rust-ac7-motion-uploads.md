# AC7 F9 constant-buffer observations

The source comparison is pinned to `43754c8`. It follows the upload bytes into an F9 record and
identifies which operations can move into `rsf-ac7` while the D3D11 observation points remain in
place. The [session audit](rust-ac7-motion-session.md) documents capture timing and the
[artifact audit](rust-ac7-motion-artifacts.md) documents file publication.

## Producers and byte ranges

| Observation | Span offered to F9 |
| --- | --- |
| Successful `CreateBuffer` | The [observer hook](../../runtime/graphics/src/d3d11_observer.cpp) supplies the returned `ID3D11Buffer*`, `pSysMem`, `ByteWidth` and bind flags. The [serialized creation callback](../../runtime/orchestrator/src/d3d11_observer_callbacks.rs) reaches AC7's registered buffer observer. The creation evicts a snapshot at that address even when initial data is absent or the new buffer has another bind role. Constant-buffer initial data covers `ByteWidth` bytes from offset zero. |
| `UpdateSubresource` and `UpdateSubresource1` | The [typed hooks](../../runtime/graphics/src/frame_tap.cpp) call the [watch](../../runtime/orchestrator/src/frame_tap.rs) before forwarding. It admits subresource zero without a box and a positive source row pitch at most 4096 and at most the watched buffer width. F9 receives exactly that many bytes at offset zero. A zero pitch, a boxed update and an update outside the observed context produce no snapshot through this path. |
| `Map` and `Unmap` | A successful subresource-zero `WRITE_DISCARD` map records the pointer and watched width in the tap's pending table. The watch receives the full watched width before the matching `Unmap` forwards, while the mapping is valid. This path uses the buffer width and starts at the mapping's first byte. |

The [loader](../../loader/proxy/src/dinput8_proxy.c) wires creation before D3D startup. The
[bridge](../../loader/proxy/src/dlss_bridge.c) wires the zero-size watch, which accepts constant
buffers up to 4096 bytes. When native owns rendering, the watch hands AC7 its observed bytes
directly. In compatibility mode the bridge first applies any promoted-size patch and handles the
view constants. On the writable Update path that observation uses the same scratch span forwarded
to D3D. The F9 cache copies the supplied span during the callback, including any compatibility
edits already made. It adds no padding and retains no mapped pointer or COM reference.

## Record and lifetime contracts

Rust owns the latest accepted snapshot per buffer address, up to 2048 addresses and 8192 bytes per
snapshot. A replacement can update an existing entry at capacity. An accepted observation increments
the process-wide 64-bit serial. The capture stores one serial with its exact bytes under the
observation lock. Successful session preparation clears the snapshots and constant readback counts
before the next interval is published. Uploads can span intervals in one session, and a writer
carrying an older session stamp cannot edit a later session. Creation invalidation and initial
snapshot insertion now occur under the same lock. A storage allocation failure disarms the observed
capture through the existing failure path, with the serial advanced only on successful storage.

The [native capture](../../games/ac7/src/motion_capture.cpp) still supplies slot arrays to
`constants()`. Draw records use the tap's VS/PS shadow bindings and the queried GS/HS/DS bindings
after temporary overrides are restored. Dispatch records query CS bindings. Each stage emits
non-null slots selected by its shader declaration mask in ascending slot order, with the all-slot
fallback when shader facts or disassembly are unavailable. The row's `buffer` is the interface address and
`upload_serial` describes the latest captured callback for that address. The serial gives CPU
observation order. The span starts at byte zero of the underlying buffer, and the reports carry no
D3D11.1 constant-buffer range or GPU completion evidence.

Ordinary constants prefer the upload copy. Contact-light before-draw records query live PS slots
0 and 1 and always take the live-read path when eligible. Rust keeps the 120-read shared threshold,
128-read total ceiling and eight lighting reads per interval. A live read reserves a slot after
native `GetDesc` gives a nonzero `ByteWidth` of at most 8192. Native still calls the typed
`rsf_read_constant_buffer` with that width and keeps its device and readback effects. JSON fields,
ordering and blob rules stay compatible. An upload row has the saved serial and `readback:0`.
A live attempt has serial zero and `readback:1` even on failure. A missing source emits zero for
both fields. Existing blob deduplication and limits determine the `blob` ID, and blob zero still
represents absent or dropped bytes. The manifest continues to report reserved attempts and read
failures completed within their active session.

Geometry rows use the borrowed draw facts and the observed input layout and VS hash. The capture
samples vertex buffers at the bound IA byte offsets. For its selected VS SRVs it uses the typed
format or structure stride and `FirstElement * stride`, bounded by the view span, resource width
and the 16384-byte sample limit. Geometry and its queued scope have no constant upload serial or
constant snapshot lookup. The loader invokes the existing velocity and depth consumers separately
from the geometry recorder. They retain their own resource and draw effects.

## Verification

Compared the producers, slot selection, geometry, typed reads and schema with the pinned source.
Rust 1.94.1 formatting, locked offline Windows Cargo metadata and Git whitespace checks passed.
Builds, compilation and linking, Clippy, tests, device/game validation, deployment and push remain
user-owned.
