# AC7 F9 capture session in Rust

The comparison starts at `be38421b99a2f310e6c7ff591dd78eb4e5dc1838` on
`rust/orchestrator-anomaly`. It follows the source from the public
[capture ABI](../../games/ac7/include/rescaleframe/ac7_motion_capture.h) through
the [native capture](../../games/ac7/src/motion_capture.cpp), the loader and the existing
[Rust artifact writer](../../games/ac7/src/motion_capture_records/artifacts.rs). The earlier
[artifact comparison](rust-ac7-motion-artifacts.md) records the complete file fields and encodings.
This port asks which decisions can move into `rsf-ac7` while the same native sites continue to
report renderer facts. The evidence for this port comes from the pinned source.

## Entry points and interval identity

[The carrier](../../loader/proxy/src/dinput8_proxy.c) enables and configures F9 through
`RSF_MOTION_CAPTURE`, registers buffer creation before D3D11 startup, and installs the expected-byte
AC7 observation hooks after decryption. Its observer hook and its D3D11 Present facade both point to
`rsf_bridge_present_hook`. The overlay's separate `trigger_capture` action calls RenderDoc. The
existing [capture I/O fixture](../../tests/ac7_capture_io.cpp) exercises the public programmatic
`request`, `present` and `shutdown` wrappers and drives the full 0 through 60 sequence.

[The Present bridge](../../loader/proxy/src/dlss_bridge.c) calls the recorder from `on_present`,
after pending requests and before its backend maintenance and end-of-frame work. The
[observer](../../runtime/orchestrator/src/d3d11_observer_present.rs) invokes this callback before a
non-test native Present. The [facade](../../runtime/orchestrator/src/d3d11_present.rs) invokes it at
its application callback boundary before physical presentation. The bridge's completed event
independently reports matched native input dumps and successful application frames. Its renderer
shutdown path returns from `on_present` before the F9 tick and uninstalls the frame tap during
renderer retirement. The explicit motion recorder shutdown used by the fixture disarms and clears
tap callbacks before removing the MinHook observations. Shutdown leaves prior checkpoints on disk
and skips final publication.

[The scheduler](../../games/ac7/src/motion_capture_session.rs) implements the following sequence from
baseline `motion_capture.cpp:1097–1139`.

| Boundary | Result |
| --- | --- |
| Disabled or invalid call | Return 0 without touching the prefix or advancing the counter. |
| Valid call | Clear the caller prefix and increment the wrapping 64-bit Present counter. A failed `GetDesc` still counts. It leaves the key latch, queued request, interval and callbacks alone. |
| Valid poll | Consume the coalesced programmatic request and latch the observed F9 state. F9 is high-bit down only while the current process owns the foreground window. A foreground down edge or a request can arm an idle recorder. Active sessions consume triggers, including a trigger on the normal finish call. |
| Start | Try `motion-YYYYMMDD-HHMMSS-<pid>-<next session>` with native local time and narrow `CreateDirectoryA`. Only successful preparation and reset consume the next 32-bit session ID. The triggering Present is age 0. |
| Ages 0–59 | Use the low 32 bits of the Present delta. Observers can record CPU decisions throughout the interval range. The research tap is enabled at 0, 30 and 59, and the caller receives the capacity-limited `f000`, `f030` or `f059` prefix even if the tap is unavailable. Geometry, sampled graph/RHI work and sampled post-process checkpoints use the same selector. A `GetDesc` gap can skip an age without manufacturing a sample. |
| Age 60 or later on a valid poll | Disarm and clear the research tap before final publication. Return 0. Another edge or request can arm on a later call. |

The bridge uses each returned prefix for `rsf_bridge_request_dump`. Its started gate still governs
route images and the native composition request. The enabled gate also governs the pending backend
dump, whose single slot holds a prefix until the [evaluation path](../../runtime/orchestrator/src/dlss_pipeline.rs)
takes it. The backend writer emits frame metadata and available raw resources. The capture interval
names the callback window that requested the sample. An evaluation can be missing or late. Native
QPC ticks, Windows thread IDs, engine frame/view/command IDs, resource facts and any matched
input-dump Present IDs keep their own producers and units.

## Reset, publication and retirement

Native preparation commits the narrow path and clears draw/blob storage, blob accounting, upload
snapshots and the readback and geometry budgets while the recorder is inactive. Rust clears the
existing engine/root vectors, command map and their dropped counts before it publishes the new
session and interval together. The clock, shader and layout creation caches, shader-drop and upload
serial counters, root serial and process-wide failure count retain their original lifetimes. The
thread-local native scope and execution-pass observations keep their existing callbacks and identity.

On the sampled post-process return, Rust admits the checkpoint by the post-process call's entry
stamp. Native copies the current session's roots and companion blobs and prefix under its storage
guard. The existing Rust publisher writes `native.partial.jsonl` and `cb_<id>.bin` from those copies
after both locks are released. A checkpoint already copied can finish independently, including
after final publication, using its own directory. Only final publication writes the manifest.

At normal completion Rust serializes Present progression through the final callback, including
against a restart. The native guard protects borrowed draw, blob and shader lists. The artifact
owner temporarily takes the stopped engine and root vectors under its own lock and returns their
capacity on every result. File effects run outside that record lock. The existing writer attempts
`engine.jsonl`, `native.jsonl`, `draws.jsonl`, blobs, shaders and `session.json` in that order. Each
file uses the native narrow truncating writer. I/O failures allow subsequent attempts and yield the
partial-write status. Allocation or formatting failures stop the publication and increment the
process-wide failure counter. Prior files, including the latest checkpoint, survive. The manifest
still records schema 1 and `intervals: 60`, now from the scheduler constant.

The intentional differences concern overlapping calls and exceptional paths. Observers take a
coherent session and interval once, and storage commits recheck the session. GPU budgets check the
interval before reserving additional work, and lighting-after consumes a pending lighting-before
from the same sample. A delayed result from a retired session cannot enter a new session or disarm
it via a scoped failure. A stopped capture skips final publication if a fault wins retirement, and
its tap is inert immediately. The scheduler clears it on the next valid idle poll before any
restart, or on shutdown. A fault observed during sample registration also suppresses that prefix.
Operation-scoped failures still contribute to the process-wide count. A failed directory attempt
leaves the last committed path intact. A valid poll latches the F9 state even if allocation fails
during preparation. An in-flight readback can finish on the original thread using its captured
path. GPU work and already copied checkpoints proceed independently of final publication.

## Native residual and verification

C++ owns DXGI and Win32 polling, local time and QPC/QPF, in-process narrow paths and all directory
and file effects, typed MinHook install/retirement and originals, guarded native reads, and the
frame tap registration leaves. D3D11 observers retain the shader and layout caches, upload bytes,
draw/dispatch/geometry row serialization, live bindings, texture and buffer readbacks, and readback
budgets. The renderer owns shader replay and the proxy owns backend/route capture. The
[analyzer](../../tools/analyze-ac7-motion-capture.py) still reads the same manifest and JSONL files,
shader filename stages and the fixed three paired prefixes. Raw packed NR16 values, the unwritten
sentinel and the valid zero count are interpreted by the existing analyzer code.

Verification covers source review against the baseline, pinned Rust 1.94.1 formatting, locked
offline Cargo metadata and whitespace. Compile/link, Clippy, fixtures, captured output comparison,
device and game validation, deployment and push are user-owned.
