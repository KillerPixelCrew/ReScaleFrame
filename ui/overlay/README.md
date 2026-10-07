# In-game overlay

Rust/egui behind the C ABI in [overlay.h](include/rescaleframe/overlay.h). It produces meshes, texture updates, and settings intents. The C++ D3D11 renderer in `runtime/graphics` handles GPU work.

Current overlay ABI is **9**. AC7's proxy host and Unity's shared runtime host load the matched
Rust DLL; D3D11 and the D3D11-on-12 overlay route handle GPU drawing. **Insert** toggles the panel.
The AC7 startup hint lasts eight seconds and fades during the last second; opening Insert dismisses it.
The old F5 action and ABI 3 plan are historical. Planned inspection/capture views remain in
[the tracker](../../docs/implementation.md#egui-development-and-validation-workflow).

| Layer | Files | Responsibility |
| --- | --- | --- |
| Safe core | `model.rs`, `panel.rs`, `overlay.rs` | Status, layout, input, intents, meshes |
| C boundary | `abi.rs`, `ffi.rs` | ABI layouts, argument checks, panic handling, five exports |

The panel exposes SR enable/backend/quality, independent FG provider and SDK-limited multiplier,
Reflex, frame-limit and Show FPS controls when the host supports them. It reports requested,
effective and active state plus refusals and input/evaluation facts. Live FG switching is requested
through a host capability bit; unsupported choices retain the provider. AC7 and Unity save the FG
provider in their own preference paths. Other settings may be session-only.

The compact HUD separates rendered application FPS from SDK/DXGI aggregate presented FPS. It uses
observed counts, not a requested multiplier estimate. Neither count measures input latency or
physical scanout. [Current status](../../docs/current-status.md) records acceptance limits.

## Renderer contract

1. Use one thread for all calls on a handle. Pass collected window input to that thread.
2. Initialize `struct_size` on input and output structures. Set the ABI version as required by the header.
3. Run a frame, then drain texture updates until none remain. Apply whole-texture uploads and partial patches before drawing; process frees after the draws that need them.
4. Draw only after `RSF_OVERLAY_OK`. Empty output has null pointers and zero counts. Follow the header's borrowed-data lifetimes.
5. After `RSF_OVERLAY_ERROR_PANICKED`, destroy the handle. Panic containment requires unwinding; an aborting Rust panic profile cannot provide it.

Vertices use premultiplied RGBA with red in the low byte (`R8G8B8A8_UNORM`). Indices are local to each draw call: add `vertex_offset`, or use `BaseVertexLocation`. Positions and clip rectangles are already physical pixels.

The first frame may only measure layout and return no draws. Supply elapsed time; zero falls back to 1/60 second. The atlas limit is 2048 pixels. Display scale follows height: 1× through 1080 lines, 2× at 2160, capped at 3×.

## Verification and gaps

```bash
cargo test -p rescaleframe-overlay --locked
cargo clippy -p rescaleframe-overlay --all-targets --locked -- -D warnings
cargo fmt --all -- --check
```

Rust tests exercise controls, independent SR/FG intent, multiplier mapping, mesh bounds, texture
updates and FFI layout/argument rules. Recorded Windows/MSVC fixtures load the real Rust DLL and
check Insert, hint/panel pixels and resize; native verification alone uses a panel fixture and does
not build the Rust release DLL. Actual game use and user acceptance are recorded separately in
[AC7](../../docs/research/ac7-consumer-session.md) and
[Unity/shared FG](../../docs/research/shared-fg-20261004.md). This audit adds no new device test.

The input contract supplies pointer position, button state and wheel input, without keyboard text
or controller navigation. Short clicks between samples remain a review concern. Use mouse/drag
controls for frame limits; typed input is unavailable. The panic path has no deliberate panic test.
Broader focus, scrolling and display-scaling coverage remains separate validation.
