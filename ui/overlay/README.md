# In-game overlay

Rust/egui settings and performance HUD behind ABI 9 in
[overlay.h](include/rescaleframe/overlay.h). The crate owns layout, input history, meshes and
texture patches. Native hosts own GPU uploads, drawing and game state restoration.

The [loader overlay host](../../loader/proxy/src/overlay_host.h) connects the D3D11 renderer and
window input. The shared [D3D12 overlay host](../../runtime/orchestrator/include/rescaleframe/overlay_d3d12.h)
draws the same panel through D3D11On12. **Insert** toggles settings. A noninteractive startup
hint and optional FPS HUD can draw while settings are hidden.
The AC7 hint lasts eight seconds, fading in the last second; opening settings dismisses it.
Planned inspection/capture views remain in
[the tracker](../../docs/implementation.md#egui-development-and-validation-workflow).

## Source map

| File | Responsibility |
| --- | --- |
| `src/lib.rs` | Public safe API and module boundary |
| `src/model.rs` | Borrowed runtime snapshots, quality IDs, frame input and one-shot intents |
| `src/panel.rs` | Controls, pending selection, provider/multiplier/Reflex/limiter layout and HUD |
| `src/overlay.rs` | egui state, pointer transitions, mesh flattening, clipping and texture queues |
| `src/performance.rs` | QPC/counter FPS windows and discontinuity handling |
| `src/abi.rs` | Field-for-field Rust representation of C structures and constants |
| `src/ffi.rs` | Five C exports, size checks, borrowed outputs and panic containment |
| `include/rescaleframe/overlay.h` | Native callers' version, units, ownership and call ordering |
| `Cargo.toml` | Rust library and loadable DLL outputs; workspace dependencies/lints |

The current settings panel offers upscaling enablement, SR and independent FG provider choices,
quality, supported generation multipliers, Reflex, frame limiting and FPS HUD visibility.
Legacy dump/debug/reinsertion/scale fields remain in the ABI; the current panel does not expose
all of them. Widget availability follows the host snapshot, including implemented provider bits.
A provider choice does not establish GPU compatibility or generation activity.
Live switching is advertised through a host capability bit; unsupported requests retain the
provider. AC7 and Unity save FG selection in their own preference paths. Other settings may be
session-only. [Current status](../../docs/current-status.md) records acceptance limits.

## Native call sequence

1. Create a handle with the exact ABI version. Use exclusive access on one owning thread for
   every call, including destruction; pass collected window input to that thread.
2. Set `struct_size` on the frame input, stats, draw-data and intent structures. Larger structures
   are accepted when their known prefix is complete; shorter structures are refused before frame
   state advances.
3. Call `rsf_overlay_frame` with input/stats. Either output may be null. Consume outputs only
   after `RSF_OVERLAY_OK`; errors do not guarantee that old output structures were cleared.
4. Drain `rsf_overlay_texture_updates` until zero. Apply full uploads and partial patches before
   drawing. Entries are copied into the caller's array; pixel buffers borrow the handle until its
   next frame call or destruction.
5. Draw meshes in reported order, then drain `rsf_overlay_textures_to_free` and release those
   textures. Each entry drains once; undrained work is discarded by the next frame.
6. Destroy the handle after `RSF_OVERLAY_ERROR_PANICKED`. Further frame calls report the poison;
   collectors return zero. Panic containment requires unwinding and cannot catch allocator aborts.

Foreign pointers must be aligned, valid for their stated sizes and nonoverlapping. Null/size
checks do not establish memory validity. Texture collectors have no error channel: zero can
mean empty, invalid arguments or a poisoned handle. `create` reports version mismatch with null;
the ABI mismatch result constant remains reserved.

## Representation and state

Vertices carry premultiplied RGBA with red in the low byte (`R8G8B8A8_UNORM`). Indices are local
to each draw call: add `vertex_offset`, or use `BaseVertexLocation`. Positions and scissor extents
are physical pixels. Texture IDs reserve bit 63 for user textures; managed atlas IDs keep it clear.

The first visible frame may measure layout and return no draws. Visible settings frames use host
deltas for animation, capped at one second; invalid/nonpositive deltas fall back to 1/60 second.
Hidden hint/HUD frames use egui's default timing instead of advancing that host-time accumulator.
The atlas cap is 2048 pixels. Display scale follows height: 1x through 1080 pixels, 2x at 2160,
capped at 3x.

Quality/enable requests persist until the runtime acknowledges them in stats. Each click emits
a change once. The panel keeps requested and effective FG state separate and gates mode controls
while provider selection is pending. Mouse history tracks both host state and events delivered
to egui so hidden presses are not replayed as new clicks and stale button holds can be released.
Input is sampled mouse position/buttons/wheel; keyboard/controller events are absent and short
clicks between frame samples can be lost.

FPS uses observed 64-bit application/SDK presentation counters over windows of at least half a
second on host QPC. Counter rollback, frequency/source changes or invalid clock values reset
the baseline. Without a valid SDK aggregate both rates use application presents. Requested
multipliers do not synthesize FPS; latency and image quality are not measured here.
SDK/DXGI presented totals do not establish physical scanout.

## Validation

```bash
cargo test -p rescaleframe-overlay --locked
cargo clippy -p rescaleframe-overlay --all-targets --locked -- -D warnings
cargo fmt --all -- --check
```

Embedded fixtures cover click intents, pending selections, independent SR/FG intent, multiplier
mapping, hidden input, mesh bounds, atlas draining, rate resets and Rust FFI/layout rules.
Recorded Windows/MSVC fixtures load the real Rust DLL and
check Insert, hint/panel pixels and resize; native verification alone uses a panel fixture and does
not build the Rust release DLL. Actual game use and user acceptance are recorded separately in
[AC7](../../docs/research/ac7-consumer-session.md) and
[Unity/shared FG](../../docs/research/shared-fg-20261004.md). Fixture prerequisites and manual paths
are mapped in [tests/README.md](../../tests/README.md). Documentation adds no new device result.

The input contract supplies pointer position, button state and wheel input, without keyboard text
or controller navigation. Short clicks between samples remain a review concern. Use mouse/drag
controls for frame limits; typed input is unavailable. The panic path has no deliberate panic test.
Broader focus, scrolling and display-scaling coverage remains separate validation.
