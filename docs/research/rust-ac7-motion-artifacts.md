# AC7 F9 capture artifact publication in Rust

The question was which F9 artifacts rsf-ac7 could publish with the existing record owner while
preserving the capture directory contract. The source baseline is
13122ba152938b6fe1c5416c99bcbb4967f54880 on rust/orchestrator-anomaly. The comparison followed
the native producers through both writers in [motion_capture.cpp](../../games/ac7/src/motion_capture.cpp)
and the [offline analyzer](../../tools/analyze-ac7-motion-capture.py). The original hook and
partial-capture evidence is recorded in [renderer roots](ac7-renderer-roots-20260930.md) and
[the graph capture crash report](ac7-graph-capture-crash-20260930.md).

## Files and identity

| Artifact | Contract followed from the pinned source |
| --- | --- |
| engine.jsonl | Records keep insertion order, decimal QPC ticks and native IDs, Present interval, Windows thread ID, and the existing selection reason. The arrays retain their selection metric order, camera and bounds-origin values, and current and previous transform elements. Finite floats use nine significant digits with default-float formatting. Nonfinite elements are JSON null. |
| native.partial.jsonl and native.jsonl | Both use the same root serializer with the original field order, end-of-line convention, and numeric types. Scope IDs, parents, view/family and queue identities, graph methods, and RHI resource identities remain the copied values. Root native frames are the original 32-bit engine counters. Native graph output names are bounded, NUL-terminated, sanitized ASCII. The internal session field still gates admission without appearing in either JSONL file. |
| cb_&lt;id&gt;.bin | The ID is the one-based position in the native blob list. Root rows carry QPC and scope identity, and their existing blob IDs still reference view and graph snapshots. The view snapshot retains its 0x27c0 stride. |
| draws.jsonl and shader_&lt;stage&gt;_&lt;index&gt;_&lt;hash&gt;.dxbc | Draw and dispatch lines keep their native bytes and order; the writer adds one LF per entry. Shader data keeps native stage and unsigned decimal hash, with a one-based ordinal in the full shader list. |
| session.json | Schema 1 preserves the field order, counter sources and explanatory strings. Hook status remains numeric 0/1. The file is attempted last, and its files_written boolean reflects the preceding file attempts. Its own failure affects the finish status. |

QPC is a host clock in ticks, with frequency obtained from QueryPerformanceFrequency. Intervals
use Present age. A queue candidate joins graph work to a native command pointer and execute
thunk. The candidate labels and copied scope IDs establish neither GPU completion nor the
identity of the presented frame. The source port reuses the Rust root names and velocity reason
owner. The float writer follows the general-precision conversion in
[the Vulkan capture serializer](../../runtime/orchestrator/src/frame_capture_vk.rs), using stack
storage for formatting and fallible growth for the artifact bytes.

## Publication and lock lifetime

At the sampled post-process returns (intervals 0, 30 and 59), the native session guard protects
the narrow prefix, copied root records and companion blob snapshot. The Rust record lock
protects the root array only until the native copy finishes. Publication runs after both locks
are released. Each checkpoint overwrites native.partial.jsonl first and then attempts the
copied blobs in ID order. These writes can survive a crash before finish. Each checkpoint is
independent of final publication and has no completion manifest.

At interval 60, native finish disarms capture, clears the research callbacks and holds the
session guard. All record and association writers and reset require that guard. Rust captures
the counters and takes the stopped engine and root vectors under the record lock, then releases
that lock before any serialization or file effects. It returns the vectors and their capacity
before native finish releases its guard, including on formatting failure. The native guard
continues to protect the borrowed draw, blob and shader bytes. Finish attempts engine.jsonl,
native.jsonl, draws.jsonl, blobs, shaders and session.json in that order. Native stats and
frequency are sampled after the preceding file attempts.

The writer formats and publishes one file at a time using the original direct truncating
writes. A failed file open, write or close allows subsequent file attempts and produces the
existing partial-write status. A failed allocation or serialization stops publication and uses
the existing capture failure path. Previously published files remain available in either case.
The final writer leaves the latest partial checkpoint in the directory.

## Native boundary and analyzer

[The artifact writer](../../games/ac7/src/motion_capture_records/artifacts.rs) owns JSONL and
session JSON formatting, file suffixes, and publication order. Native code owns the F9 session
and Present timing, local clock and process/session directory naming, CreateDirectoryA and the
capacity-limited fNNN paired-sample prefix. It supplies a narrow fopen/fwrite/fclose callback.
Rust preserves the prefix as a C string of raw bytes and appends ASCII suffixes in a fallible
buffer, so the existing narrow Windows path interpretation still governs naming and file access.

Typed detours, guarded engine reads and name sanitization, shader and texture capture, live
D3D11 draw/dispatch serialization and readback, checkpoint blob copying, and the capture logs
also remain native. The paired backend dump starts from the prefix via
[the Present bridge](../../loader/proxy/src/dlss_bridge.c). The runtime depth replay reads
live frame-tap observations independently of these artifacts.

The analyzer opens session.json, reads engine.jsonl and draws.jsonl, counts shader stages by
filename, and checks paired data under f000, f030 and f059. The checkpoint and final root JSONL
are evidence for separate graph inspection. The analyzer expects a finish manifest and has no
partial-root ingestion path.

Validation covers the pinned source comparison of structure, operands, order, names, lock scope,
and analyzer inputs, including all 46 root, 25 engine and 27 session fields. Rust 1.94.1
formatting, locked offline Cargo metadata, and whitespace checks passed. Compile and link,
Clippy, tests, captured artifact comparison, game and device checks, and deployment remain open.
