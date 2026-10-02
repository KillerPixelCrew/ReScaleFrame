# AC7 graph capture and pause-menu crash, 30 September 2026

The question was whether the engine graph could identify the passes that still reduce the
reconstructed image and UI, while preserving queued view identity. Process 215904 used the
19:29 capture build, proxy SHA-256
`995db001a80f627eedb46e2f182d00d87e368530d10c8729482e70f795b58db4`. Executable/dump and
UE4.18.3 reference fingerprints remain those in the [renderer audit](ac7-renderer-roots-20260930.md).
Raw game data and the minidump stay ignored. [Sanitized evidence](evidence/ac7-graph-capture-crash-20260930.json)
records file hashes, pass/draw pairs, function RVAs and the crash analysis.

## What survived

Nine sessions completed with 27 paired samples and no dropped native/command records.
The first two show mission briefing, 3-7 hangar and aircraft/weapon selection, and 8-9 flight.
Session 10 captured the pause-menu screenshot and its first colour input/output, depth and
raw/decoded motion. The game crashed before that session completed, so its final native/draw
JSONL and further samples were not written. These partial files must not be counted as a
completed 60-interval capture.

The native graph and RHI command matches identify the previously anonymous passes:

| Engine output | Process RVA | ComputeOutputDesc RVA | Flight descriptor / actual draw |
| --- | --- | --- | --- |
| Tonemap | `0x10b0220` | `0x1098ff0` | 928x524 / 928x524 |
| PostProcessAA (FXAA) | `0xfbfe90` | `0xfb6f20` | 928x524 / 928x524 |
| PostProcessMaterial | `0x1004960` | `0xff7b50` | 928x524 / 928x524 |
| NimbusHUDCombine | `0xfcbc90` | `0xfb7bc0` | 928x524 / promoted 1600x900 |
| Nimbus composite | `0xfc86c0` | `0xfb7b50` | 928x524 / promoted 1600x900 |
| Final Upscale | `0x10b1650` | `0x10991e0` | 1600x900 / 1600x900 |

FXAA maps to PS `4ceee9f1`, the material to `a5c9147b`, and HUD combine to `b2f7719d`.
The hangar promotes the tonemap and FXAA targets too, despite their native descriptors still
reporting render resolution. The flight chain does not. The captured HUD converter DrawSize
is 1920x1080; that alone does not prove every pause-menu producer rasterizes at that size.

This is a native allocation/processing boundary, not a texture-format guess. The exact stock
and custom method identities are named in Ghidra and retained in the renderer-root manifest.
Custom names describe the captured behavior rather than claim recovered original symbols.
The next renderer change must make those descriptors and source/destination rectangles agree
with SR output, while preserving material effects, HUD glow and depth where applicable.
Native FXAA still runs after SR. A graph-owned replacement should avoid that duplicate AA,
preserve SSR's independent temporal filter, and restore native behavior when SR is inactive.
Neither that replacement nor a complete UI/temporal correction is installed by this work.

## Crash cause and evidence

This is a user-visible rendering crash, unlike the earlier exit-only Streamline observation.
The fault is a driver access violation reading address `0x34` on render thread 65196.
The original log's symbol names are nearest exported symbols and wrongly suggest overlay
or observer teardown. Release did not have private PDB symbols.

Relinked the existing Release objects with a linker MAP into an ignored diagnostic directory.
The original and relinked `.text` are byte-identical, SHA-256
`d25a31fb62db25d5084fa4ce788027dc432c965bb9317642e872bda6171edc1e`. Only after that check
were the crash RVAs resolved:

`hooked_om_set_render_targets` -> `plan_render_targets` -> `on_gate` -> `route_dump` ->
`rsf_dump_texture_bytes` -> `hooked_copy_subresource_region` -> D3D11/driver.

The earlier scene and reconstruction-seed copies completed; the subsequent cached translucency
copy faulted. Compiler-reported bridge layout and minidump memory give cached layer pointer
`0x1da35c1e7e0`; the driver's source register is `0x1da35c1e970`, inside that object's wrapper
at `+0x190`. The geometry callback lends its target only for the callback. The bridge stored
that pointer without AddRef, then copied it after the screen changed. This violates the resource
lifetime contract regardless of whether a particular pool reuse leaves readable descriptor data.

The correction retains the observed target before releasing the old one. It records its current
Present interval and skips layer captures if no producer drew it in that interval. Retention
preserves the allocation; the freshness check avoids treating a previous role as current contents.
No invalid-pointer exception handler is used as a substitute for ownership.

Attempted recovery of the in-memory capture using the same compiler's RootRecord/Capture layouts
and minidump memory streams. The static vector headers and session number survive, but the
native/draw/blob heap allocations do not. Their unwritten contents cannot be recovered from
this minidump. To avoid that loss on future failures, the three sampled post-process returns now
write `native.partial.jsonl` and their immutable numbered blobs before queued graphics execution.
The complete-session files remain separately identified. Checkpoints add capture-only CPU/I/O
overhead; they are not runtime frame callbacks or a GPU completion barrier.

## Delivery and limits

Release now emits `dinput8.map` for precise private-function address lookup. The native gate
passed on Windows MSVC with VS2026: 31 passed, five vendor-environment skips, Rust formatting
and lint passed. The corrected proxy and matching Rust overlay were explicitly deployed with
backups after the real panel passed Insert, rendering and resize checks. The corrected pause
capture has not been game-tested yet. UI blur, shimmer, motion convention and native graph
migration remain open. Existing captures are sufficient for continued source/native work;
there is no need to repeat the nine completed sessions.
